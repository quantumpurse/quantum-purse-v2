//! Shared types, constants, and utility functions for the GUI.

use eframe::egui;
use qpv2_core::types::SpxVariant;
use serde::{Deserialize, Serialize};

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

/// Identifies which transaction flow owns a shared background operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransactionKind {
    Transfer,
    Dao,
}

/// Identifies which flow triggered a spendable capacity fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpendableCapacityTarget {
    Transfer,
    DaoDeposit,
}

/// Result type for transaction building (unsigned tx, input cells, lock_args).
pub(crate) type TxBuildResult = Result<
    (
        TransactionKind,
        ckb_types::core::TransactionView,
        Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        String,
    ),
    String,
>;

/// Result type for sending a signed transaction.
pub(crate) type TransactionSendResult = (TransactionKind, Result<String, String>);

/// Result type for DAO cell queries across all accounts.
pub(crate) type DaoQueryResult = Result<DaoQueryEvent, String>;

/// Streaming DAO query event from background thread.
pub(crate) enum DaoQueryEvent {
    Deposited(String, node_manager::DepositedCell),
    Prepared(String, node_manager::PreparedCell),
    Done,
}

/// Classification of a transaction from the wallet's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) enum TxKind {
    Incoming,
    Outgoing,
    DaoDeposit,
    DaoPrepare,
    DaoWithdraw,
}

/// A resolved transaction record for display on the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TxRecord {
    pub tx_hash: String,
    pub tx_kind: TxKind,
    /// Net amount in shannons (always positive).
    pub amount: u64,
    pub block_number: u64,
    /// Unix timestamp in seconds, resolved from block header.
    pub timestamp: u64,
    pub is_pending: bool,
    /// Lock args of the wallet account that owns this transaction.
    pub owner_lock_args: String,
    /// For internal transfers: lock args of the other wallet account involved.
    pub internal_counterparty_lock_args: Option<String>,
    /// For Outgoing to external addresses: the first external recipient's full
    /// bech32m address. Used to build the Address Book in the Transfer tab.
    pub external_recipient_address: Option<String>,
}

/// Streaming event from the transaction history background thread.
pub(crate) enum TxHistoryEvent {
    Record(TxRecord),
    /// Emitted when the sync thread has no more records to stream. The
    /// watermark is derived from the merged `tx_history` vector, so no
    /// payload is needed here.
    Done,
}

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    None,
    Info(String),
    Error(String),
}

/// Tracks in-flight passkey operations so the UI doesn't block.
#[cfg(target_os = "macos")]
pub(crate) enum PasskeyOp {
    /// Waiting for passkey registration to complete.
    Registration {
        op: passkey_prf::PendingRegistration,
        variant: SpxVariant,
        window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    },
    /// Registration done; waiting for PRF assertion to get the encryption key.
    PostRegistrationAssert {
        op: passkey_prf::AssertionRequest,
        variant: SpxVariant,
        credential_id: Vec<u8>,
    },
    /// Waiting for unlock credential assertion (no PRF).
    UnlockAssert { op: passkey_prf::AssertionRequest },
    /// Waiting for PRF assertion to create a new account.
    NewAccountAssert { op: passkey_prf::AssertionRequest },
    /// Waiting for PRF assertion to sign a transfer transaction.
    SignTransactionAssert {
        op: passkey_prf::AssertionRequest,
        // just relaying transaction type to the poller.
        kind: TransactionKind,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    },
}

/// Tracks the state of an in-progress transfer transaction.
#[derive(Debug, Clone)]
pub(crate) enum TransactionStatus {
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
    pub(crate) accent_tint: egui::Color32,
    pub(crate) accent2_tint: egui::Color32,
    /// Low-alpha warn fill used for pill/badge backgrounds.
    pub(crate) warn_tint: egui::Color32,
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
            accent_tint: egui::Color32::from_rgba_unmultiplied(0, 255, 180, 20), // rgba(0,255,180,0.08)
            accent2_tint: egui::Color32::from_rgba_unmultiplied(0, 200, 255, 20), // rgba(0,200,255,0.08)
            warn_tint: egui::Color32::from_rgba_unmultiplied(255, 209, 102, 26), // rgba(255,209,102,0.10)
            danger: egui::Color32::from_rgb(255, 77, 109),                        // #ff4d6d
            warn: egui::Color32::from_rgb(255, 209, 102),                         // #ffd166
            text: egui::Color32::from_rgb(232, 244, 240),                         // #e8f4f0
            text_muted: egui::Color32::from_rgb(90, 122, 112),                    // #5a7a70
        }
    }
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

/// Format a Unix timestamp as relative time ("3h ago", "1d ago").
pub(crate) fn format_relative_time(timestamp_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let diff = now.saturating_sub(timestamp_secs);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}
