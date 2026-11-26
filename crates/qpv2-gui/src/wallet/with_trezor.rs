//! Trezor watch-only wallets: connect, new account, and the device-link
//! status.

use qpv2_core::types::{AuthMethod, SpxVariant};
use qpv2_core::KeyVault;

use super::StartingBlock;
use crate::types::{Status, TrezorAccountCreate, TrezorConnectUpdate};
use crate::App;

/// Cadence for the device-link probe. Short enough that unplugging or
/// closing Trezor Suite shows up while the user is still looking at the
/// screen; the probe itself is a USB enumeration plus one claim/release, so
/// the cost is negligible.
const DEVICE_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

impl App {
    /// Connect a Trezor and import its accounts as a new watch-only wallet.
    ///
    /// The device is queried for `account_count` addresses (indices 0..N),
    /// each shown on the device for confirmation, and stored as single-sig
    /// accounts with no vault seed. Signing later streams transactions to the
    /// device.
    ///
    /// Runs on a worker thread: a locked device holds the connect open until
    /// the user has typed their PIN, and every address needs an on-device
    /// confirmation after that, so the screen it was started from (the setup
    /// screen or the New Wallet dialog) has to keep painting to tell the user
    /// what the device is waiting for. `poll_trezor_wallet_setup`
    /// picks up the result and creates the wallet. No-op while an import is
    /// already in flight.
    pub(crate) fn start_create_trezor_watch_only_wallet(
        &mut self,
        variant: SpxVariant,
        account_count: u32,
    ) {
        if self.trezor_connect_rx.is_some() {
            return;
        }
        // Check the name now; the wallet is only created after the device steps.
        if let Err(e) = self.prepare_new_wallet() {
            tracing::error!("{}", e);
            self.status = Status::Error(e);
            return;
        }
        let is_mainnet = self.qp_client.is_mainnet();

        let (tx, rx) = std::sync::mpsc::channel();
        self.trezor_connect_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> TrezorConnectUpdate {
                let mut device = trezor_connect::open(&mut trezor_connect::PinentryPairing)
                    .map_err(|e| format!("Could not connect to Trezor: {}", e))?;
                let model = device.model();

                let mut accounts = Vec::with_capacity(account_count as usize);
                for i in 0..account_count {
                    let account = device.get_account(i, variant, is_mainnet).map_err(|e| {
                        format!("Failed to import account {} from Trezor: {}", i, e)
                    })?;
                    accounts.push(account);
                }
                Ok((model, variant, accounts))
            })();
            let _ = tx.send(result);
        });
    }

    /// Turn the accounts imported from a Trezor into a wallet. Called by
    /// `poll_trezor_wallet_setup` on the UI thread; the wallet id is allocated only
    /// now, so a failed or cancelled import leaves no empty wallet behind.
    pub(crate) fn finish_create_trezor_watch_only_wallet(
        &mut self,
        model: String,
        variant: SpxVariant,
        accounts: Vec<qpv2_core::types::SphincsPlusAccount>,
    ) {
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };

        if let Err(e) = KeyVault::add_hw_watch_only_wallet(
            wallet_id,
            &wallet_name,
            variant,
            &model,
            trezor_connect::TREZOR_CONVENTION,
            accounts,
        ) {
            let msg = format!("Failed to create Trezor wallet: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        self.finalize_wallet_setup(
            AuthMethod::Trezor { model },
            "Trezor wallet connected!",
            wallet_id,
            wallet_name,
            StartingBlock::Detect,
        );
    }

    /// Start importing the next account from a connected Trezor. Runs on a
    /// worker, as in `start_create_trezor_watch_only_wallet`: a locked device
    /// holds the connect open until its PIN is typed, and the address needs an
    /// on-device confirmation after that. `poll_trezor_new_account` hands the
    /// account to `finish_create_hw_wallet_account`. No-op while one is already
    /// in flight.
    ///
    /// Single-sig accounts are numbered from 0 with no gaps, so the next index
    /// is their count. Multisig accounts are stored apart and don't count.
    pub(super) fn start_create_hw_wallet_account(&mut self) {
        if self.trezor_new_account_rx.is_some() {
            return;
        }
        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("Failed to read wallet variant: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };
        let next_index = match KeyVault::get_singlesig_lock_args(self.wallet_id) {
            Ok(existing) => existing.len() as u32,
            Err(e) => {
                let msg = format!("Failed to read the wallet's accounts: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };
        let is_mainnet = self.qp_client.is_mainnet();

        let (tx, rx) = std::sync::mpsc::channel();
        self.trezor_new_account_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> TrezorAccountCreate {
                let mut device = trezor_connect::open(&mut trezor_connect::PinentryPairing)
                    .map_err(|e| format!("Could not connect to Trezor: {}", e))?;
                device
                    .get_account(next_index, variant, is_mainnet)
                    .map_err(|e| {
                        format!("Failed to import account {} from Trezor: {}", next_index, e)
                    })
            })();
            let _ = tx.send(result);
        });
    }

    /// Store the account `start_create_hw_wallet_account` read from the Trezor.
    /// Called by `poll_trezor_new_account` on the UI thread.
    pub(crate) fn finish_create_hw_wallet_account(
        &mut self,
        account: qpv2_core::types::SphincsPlusAccount,
    ) {
        if let Err(e) = KeyVault::add_hw_watch_only_account(self.wallet_id, account.clone()) {
            let msg = format!("Failed to store account: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        self.balances.insert(account.lock_args.clone(), Some(0));
        self.spendable_balances
            .insert(account.lock_args.clone(), Some(0));
        // A wallet's first account may hold history: `qpv2-cli init --trezor`
        // makes the wallet empty, and this makes its first account.
        let start = if self.accounts.is_empty() {
            StartingBlock::Detect
        } else {
            StartingBlock::Tip
        };
        self.register_lock_scripts_with_light_client(
            std::slice::from_ref(&account.lock_args),
            start,
        );
        self.accounts.push(account);
        self.refresh_wallet_cache();
        self.status = Status::Info("New Trezor account imported!".to_string());
    }

    /// True while one of our own operations owns the device.
    ///
    /// Read from live receivers rather than the last probe result: probe
    /// results travel through a channel, so one can be produced while the
    /// device is free and consumed after a session has claimed it. Anything
    /// that displays "busy" must consult this, not the probe, or it will show a
    /// device as available in the middle of a signature.
    pub(crate) fn trezor_operation_in_flight(&self) -> bool {
        self.trezor_connect_rx.is_some()
            || self.trezor_reconnect_rx.is_some()
            || self.trezor_new_account_rx.is_some()
            // Signing shares its channel with the other auth methods, so it
            // only implies device work on a device-backed wallet.
            || (matches!(self.auth_method, Some(AuthMethod::Trezor { .. }))
                && self.transaction_submission_rx.is_some())
    }

    /// Refresh the Trezor link status if the cadence is due.
    ///
    /// No-op unless the open wallet is device-backed, and single-flight — a
    /// wedged claim must not queue probe threads behind itself. The probe stands
    /// aside on its own when one of our sessions is open (see
    /// `trezor_connect::probe_link`), so an in-flight operation reports
    /// `Working` rather than a spurious `Busy`.
    pub(crate) fn probe_device_link(&mut self) {
        if !matches!(self.auth_method, Some(AuthMethod::Trezor { .. })) {
            return;
        }
        if self.device_status_rx.is_some() {
            return;
        }
        if let Some(at) = self.device_status_probed_at {
            if at.elapsed() < DEVICE_PROBE_INTERVAL {
                return;
            }
        }
        self.device_status_probed_at = Some(std::time::Instant::now());

        let (tx, rx) = std::sync::mpsc::channel();
        self.device_status_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(trezor_connect::probe_link());
        });
    }

    /// Re-establish a Trezor session on demand: the same connect the setup
    /// screen's button performs — PIN and pairing included — but it creates no
    /// wallet. Success only proves the device is reachable, so the handle is
    /// dropped immediately and the link status refreshed.
    pub(crate) fn reconnect_trezor(&mut self) {
        if self.trezor_reconnect_rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.trezor_reconnect_rx = Some(rx);
        self.status = Status::Info("Connecting to Trezor...".to_string());

        std::thread::spawn(move || {
            let result = trezor_connect::open(&mut trezor_connect::PinentryPairing)
                .map(|device| device.model())
                .map_err(|e| format!("Could not connect to Trezor: {}", e));
            let _ = tx.send(result);
        });
    }
}
