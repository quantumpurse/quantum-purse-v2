//! Device discovery, connection, and the `get_address` round-trip with the
//! account built from it.
//!
//! The device is driven over THP (see [`crate::thp`]); this module holds the
//! public handle and the shared address/convention types.

use std::net::SocketAddr;

use qpv2_core::types::{MultisigConfig, SingleSigConvention, SphincsPlusAccount, SpxVariant};

use crate::link::acquire_session_guard;
use crate::thp::pairing::{NoInteraction, PairingUx};
use crate::thp::transport::{
    scan_usb, Transport, UdpTransport, UsbLocation, UsbTransport, EMULATOR_PORT,
};
use crate::thp::ThpSession;
use crate::TrezorSignerError;

/// The single-sig convention a Trezor account uses. The firmware hard-codes the
/// config header `[0x80, 0x01, 0x01, 0x01, flag]` (`required_first_n = 1`), which
/// is QuantumPurse's [`SingleSigConvention::Standard`]. Using `V1`
/// (`required_first_n = 0`) here would derive a different lock script and address,
/// and `get_address`'s parity check would reject every account.
pub const TREZOR_CONVENTION: SingleSigConvention = SingleSigConvention::Standard;

/// Where a discovered device lives — determines the transport used to reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceLocation {
    /// The local firmware emulator, over UDP loopback.
    Emulator {
        /// Emulator UDP port (default 21324).
        port: u16,
    },
    /// A physical device on the USB bus.
    Usb {
        /// libusb bus number.
        bus: u8,
        /// libusb device address on that bus.
        address: u8,
    },
}

/// A device visible to the host.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Human-readable description.
    pub label: String,
    /// Model name.
    pub model: String,
    /// How to reach the device; pass to [`open_device`].
    pub location: DeviceLocation,
}

/// A SPHINCS+ address exported from the device.
#[derive(Debug, Clone)]
pub struct DeviceAddress {
    /// Bech32m CKB address.
    pub address: String,
    /// 32-byte blake2b lock script args.
    pub lock_args: Vec<u8>,
    /// SPHINCS+ public key bytes.
    pub pubkey: Vec<u8>,
    /// SPHINCS+ variant id (48..=59) the device used.
    pub variant: u32,
}

/// An open THP connection to a Trezor device.
///
/// Holds the device guard for its whole lifetime, so the status probe stands
/// aside instead of competing for the USB interface — a probe's momentary claim
/// landing mid-operation would otherwise fail the operation with "in use by
/// another application", blaming Trezor Suite for our own status indicator.
pub struct TrezorSession {
    pub(crate) session: ThpSession,
    /// Released on drop; never read.
    _guard: std::sync::MutexGuard<'static, ()>,
}

/// The network string the firmware expects.
pub(crate) fn network_name(is_mainnet: bool) -> &'static str {
    if is_mainnet {
        "Mainnet"
    } else {
        "Testnet"
    }
}

/// The devices the host can reach: every Trezor enumerated on the USB bus,
/// plus the local emulator endpoint (which is advertised unconditionally —
/// probing it would cost a full THP connect).
pub fn list_devices() -> Vec<DeviceInfo> {
    let mut devices: Vec<DeviceInfo> = scan_usb()
        .into_iter()
        .map(|loc| DeviceInfo {
            label: format!("Trezor (USB bus {} addr {})", loc.bus, loc.address),
            model: "Trezor".to_string(),
            location: DeviceLocation::Usb {
                bus: loc.bus,
                address: loc.address,
            },
        })
        .collect();
    devices.push(DeviceInfo {
        label: format!("Trezor emulator (UDP 127.0.0.1:{EMULATOR_PORT})"),
        model: "Trezor".to_string(),
        location: DeviceLocation::Emulator {
            port: EMULATOR_PORT,
        },
    });
    devices
}

/// Open the first reachable device: a physical Trezor if one is on the USB
/// bus, otherwise the local emulator. Set `QPV2_TREZOR_EMULATOR=1` to skip
/// the USB scan and force the emulator.
///
/// A first-time connection to production firmware runs code-entry pairing;
/// `ux` supplies the 6-digit code the device displays. Paired devices skip
/// the code entry via the stored credential, but still show a "Connect?"
/// confirmation on the device each session (the credential is not the
/// autoconnect kind).
pub fn open(ux: &mut dyn PairingUx) -> Result<TrezorSession, TrezorSignerError> {
    // Before the transport, which claims the USB interface.
    let guard = acquire_session_guard().ok_or_else(session_busy)?;
    if std::env::var_os("QPV2_TREZOR_EMULATOR").is_none() {
        if let Some(loc) = scan_usb().into_iter().next() {
            return open_transport(Box::new(UsbTransport::connect(loc)?), ux, guard);
        }
    }
    open_transport(emulator_transport(EMULATOR_PORT)?, ux, guard)
}

/// Another session of ours already owns the device. Distinct from the USB
/// `Busy` message, which blames a different application.
fn session_busy() -> TrezorSignerError {
    TrezorSignerError::Client(
        "the Trezor is busy with another operation — wait for it to finish and try again"
            .to_string(),
    )
}

/// Open a specific device previously returned by [`list_devices`].
pub fn open_device(
    info: &DeviceInfo,
    ux: &mut dyn PairingUx,
) -> Result<TrezorSession, TrezorSignerError> {
    let guard = acquire_session_guard().ok_or_else(session_busy)?;
    let transport: Box<dyn Transport> = match info.location {
        DeviceLocation::Usb { bus, address } => {
            Box::new(UsbTransport::connect(UsbLocation { bus, address })?)
        }
        DeviceLocation::Emulator { port } => emulator_transport(port)?,
    };
    open_transport(transport, ux, guard)
}

/// Open the local firmware emulator on the default port, without a pairing UX.
///
/// Requires an emulator this host has already paired with: pairing takes the
/// device's preferred method, which is code entry, and [`NoInteraction`] turns
/// that into a clear error rather than blocking. Pair once by hand (the GUI, or
/// the `thp_get_address` example) and the stored credential keeps every later
/// call here unattended.
pub fn open_emulator() -> Result<TrezorSession, TrezorSignerError> {
    let guard = acquire_session_guard().ok_or_else(session_busy)?;
    open_transport(
        emulator_transport(EMULATOR_PORT)?,
        &mut NoInteraction,
        guard,
    )
}

fn emulator_transport(port: u16) -> Result<Box<dyn Transport>, TrezorSignerError> {
    Ok(Box::new(UdpTransport::connect(SocketAddr::from((
        [127, 0, 0, 1],
        port,
    )))?))
}

fn open_transport(
    transport: Box<dyn Transport>,
    ux: &mut dyn PairingUx,
    guard: std::sync::MutexGuard<'static, ()>,
) -> Result<TrezorSession, TrezorSignerError> {
    Ok(TrezorSession {
        session: ThpSession::connect(transport, ux)?,
        _guard: guard,
    })
}

impl TrezorSession {
    /// Display label for the connected device: its product name, looked up
    /// from the internal model code it reports when the channel opens. The
    /// names are the firmware's own (`MODEL_FULL_NAME` in
    /// `vendor/trezor-firmware/core/embed/models/*/model_*.h`); a code not
    /// listed shows as itself rather than as a guessed name.
    pub fn model(&self) -> String {
        match self.session.internal_model().as_deref() {
            Some("T2B1" | "T3B1") => "Trezor Safe 3".to_string(),
            Some("T3T1") => "Trezor Safe 5".to_string(),
            Some("T3W1") => "Trezor Safe 7".to_string(),
            Some("T2T1") => "Trezor Model T".to_string(),
            Some("D001") => "Trezor DIY 1".to_string(),
            Some("D002") => "Trezor DIY 2".to_string(),
            Some(code) => format!("Trezor {}", code),
            None => "Trezor".to_string(),
        }
    }

    /// Export a SPHINCS+ address for `account_index` under `variant`. The
    /// returned `lock_args` are cross-checked against the wallet's own
    /// derivation inside the THP session.
    pub fn get_address(
        &mut self,
        account_index: u32,
        variant: SpxVariant,
        is_mainnet: bool,
        show_display: bool,
    ) -> Result<DeviceAddress, TrezorSignerError> {
        self.session
            .get_address(account_index, variant, is_mainnet, show_display)
    }

    /// The single-sig account at `account_index`, ready to store. The device
    /// derives it and shows its address for confirmation; `is_mainnet` only
    /// sets that address's prefix, as the account is the same on either
    /// network.
    pub fn get_account(
        &mut self,
        account_index: u32,
        variant: SpxVariant,
        is_mainnet: bool,
    ) -> Result<SphincsPlusAccount, TrezorSignerError> {
        let addr = self.get_address(account_index, variant, is_mainnet, true)?;
        let config = MultisigConfig::single_sig(variant, addr.pubkey, TREZOR_CONVENTION);
        Ok(SphincsPlusAccount {
            index: account_index,
            lock_args: hex::encode(&addr.lock_args),
            config,
        })
    }
}
