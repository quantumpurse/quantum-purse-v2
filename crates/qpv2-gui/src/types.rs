//! Shared types, constants, and utility functions for the GUI.

use eframe::egui;
use node_manager::CkbRpc;
use qpv2_core::types::SpxVariant;

/// Computes the SPHINCS+ witness lock size for a given variant.
///
/// The lock field format is: `[4-byte config] + [1-byte flag] + [pubkey] + [signature]`.
pub(crate) fn spx_witness_lock_size(variant: SpxVariant) -> usize {
    let param_id: ckb_fips205_utils::ParamId = (variant as u8)
        .try_into()
        .expect("SpxVariant and ParamId use the same discriminants");
    let (pk_len, sig_len) = ckb_fips205_utils::verifying::lengths(param_id);
    5 + pk_len + sig_len
}

/// Result of a single account balance fetch from a background thread.
pub(crate) type BalanceResult = (String, Result<u64, String>);

/// Result type for transaction building (unsigned tx, input cells, lock_args).
pub(crate) type TxBuildResult = Result<
    (
        ckb_types::core::TransactionView,
        Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        String,
    ),
    String,
>;

/// Result type for DAO cell queries across all accounts.
pub(crate) type DaoQueryResult = Result<
    (
        Vec<(String, node_manager::DepositedCell)>,
        Vec<(String, node_manager::PreparedCell)>,
    ),
    String,
>;

/// Sidebar navigation tabs matching the mockup layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Tab {
    Dashboard,
    Transfer,
    DaoOperations,
    Accounts,
}

/// Application state machine.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Screen {
    /// No wallet exists yet — user chooses variant and creates one.
    Setup,
    /// Wallet exists — waiting for Touch ID to unlock.
    Locked,
    /// Wallet unlocked — show wallet info.
    Unlocked,
}

/// Status messages shown to the user.
#[derive(Debug, Clone)]
pub(crate) enum Status {
    None,
    Info(String),
    Error(String),
}

/// Tracks in-flight passkey operations so the UI doesn't block.
#[cfg(target_os = "macos")]
pub(crate) enum PendingOp {
    /// Waiting for passkey registration to complete.
    Registration {
        pending: passkey_prf::PendingRegistration,
        variant: SpxVariant,
        window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    },
    /// Registration done; waiting for PRF assertion to get the encryption key.
    PostRegistrationAssert {
        pending: passkey_prf::AssertionRequest,
        variant: SpxVariant,
        credential_id: Vec<u8>,
    },
    /// Waiting for unlock credential assertion (no PRF).
    UnlockAssert {
        pending: passkey_prf::AssertionRequest,
    },
    /// Waiting for PRF assertion to create a new account.
    NewAccountAssert {
        pending: passkey_prf::AssertionRequest,
    },
    /// Waiting for PRF assertion to sign a transfer transaction.
    SignTransferAssert {
        pending: passkey_prf::AssertionRequest,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    },
    /// Waiting for PRF assertion to sign a DAO transaction.
    SignDaoAssert {
        pending: passkey_prf::AssertionRequest,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    },
}

/// Tracks the state of an in-progress transfer transaction.
#[derive(Debug, Clone)]
pub(crate) enum TransferStatus {
    /// No transfer in progress.
    Idle,
    /// Building the unsigned transaction.
    Building,
    /// Waiting for Touch ID to sign.
    AwaitingSignature,
    /// Sending the signed transaction.
    Sending,
    /// Transaction sent successfully.
    Success(String),
    /// An error occurred.
    Error(String),
}

/// Tracks the state of an in-progress DAO transaction.
#[derive(Debug, Clone)]
pub(crate) enum DaoStatus {
    /// No DAO operation in progress.
    Idle,
    /// Building the unsigned transaction.
    Building,
    /// Waiting for Touch ID to sign.
    AwaitingSignature,
    /// Sending the signed transaction.
    Sending,
    /// Transaction sent successfully.
    Success(String),
    /// An error occurred.
    Error(String),
}

/// Sub-view within the DAO tab.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DaoView {
    /// Show overview with stats, action cards, and positions table.
    Overview,
    /// Deposit form.
    Deposit,
}

/// CKB uses 8 decimal places: 1 CKB = 100,000,000 shannons.
pub(crate) const CKB_DECIMAL_PLACES: u64 = 100_000_000;

/// Custom color scheme matching the quantum aesthetic mockup.
pub(crate) struct AppColors {
    pub(crate) bg: egui::Color32,
    pub(crate) surface: egui::Color32,
    pub(crate) surface2: egui::Color32,
    pub(crate) border: egui::Color32,
    pub(crate) border2: egui::Color32,
    pub(crate) accent: egui::Color32,
    pub(crate) accent2: egui::Color32,
    pub(crate) accent3: egui::Color32,
    pub(crate) danger: egui::Color32,
    pub(crate) warn: egui::Color32,
    pub(crate) text: egui::Color32,
    pub(crate) text_muted: egui::Color32,
}

impl Default for AppColors {
    fn default() -> Self {
        Self {
            bg: egui::Color32::from_rgb(8, 12, 16),        // #080c10
            surface: egui::Color32::from_rgb(13, 19, 24),  // #0d1318
            surface2: egui::Color32::from_rgb(17, 25, 32), // #111920
            border: egui::Color32::from_rgba_unmultiplied(0, 255, 180, 26), // rgba(0,255,180,0.10)
            border2: egui::Color32::from_rgba_unmultiplied(0, 255, 180, 56), // rgba(0,255,180,0.22)
            accent: egui::Color32::from_rgb(0, 255, 180),  // #00ffb4
            accent2: egui::Color32::from_rgb(0, 200, 255), // #00c8ff
            accent3: egui::Color32::from_rgb(155, 127, 212), // #9b7fd4
            danger: egui::Color32::from_rgb(255, 77, 109), // #ff4d6d
            warn: egui::Color32::from_rgb(255, 209, 102),  // #ffd166
            text: egui::Color32::from_rgb(232, 244, 240),  // #e8f4f0
            text_muted: egui::Color32::from_rgb(90, 122, 112), // #5a7a70
        }
    }
}

/// Fetch the balance (in shannons) for a single account by its lock_args.
pub(crate) fn fetch_account_balance(
    rpc: &dyn CkbRpc,
    lock_args: &str,
    is_mainnet: bool,
) -> Result<u64, node_manager::NodeManagerError> {
    let (code_hash, hash_type) = if is_mainnet {
        (
            qpv2_core::constants::CKB_MAINNET_CODE_HASH,
            qpv2_core::constants::CKB_MAINNET_HASH_TYPE,
        )
    } else {
        (
            qpv2_core::constants::CKB_TESTNET_CODE_HASH,
            qpv2_core::constants::CKB_TESTNET_HASH_TYPE,
        )
    };

    node_manager::fetch_lock_script_balance(rpc, code_hash, hash_type, lock_args)
}

/// Format shannons as a numeric CKB string without the unit suffix.
/// For example: 100_000_000 -> "1", 150_000_000 -> "1.5".
pub(crate) fn format_ckb(shannons: u64) -> String {
    let whole = shannons / CKB_DECIMAL_PLACES;
    let frac = shannons % CKB_DECIMAL_PLACES;
    if frac == 0 {
        format!("{}", whole)
    } else {
        let frac_str = format!("{:08}", frac);
        let trimmed = frac_str.trim_end_matches('0');
        format!("{}.{}", whole, trimmed)
    }
}

/// Format a number with thousands separators (e.g. `9999` -> `"9,999"`).
pub(crate) fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result
}

/// Format a balance in shannons to a human-readable CKB string.
/// 1 CKB = 100,000,000 shannons.
pub(crate) fn format_ckb_balance(shannons: u64) -> String {
    let whole = shannons / CKB_DECIMAL_PLACES;
    let frac = shannons % CKB_DECIMAL_PLACES;
    if frac == 0 {
        format!("{} CKB", format_with_commas(whole))
    } else {
        // Show first 2 decimal places.
        let frac_str = format!("{:08}", frac);
        format!("{}.{} CKB", format_with_commas(whole), &frac_str[..2])
    }
}
