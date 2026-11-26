//! Transaction builders for CKB.
//!
//! This module provides builders for constructing various types of CKB transactions:
//! - Transfer transactions (sending CKB)
//! - DAO deposit transactions
//! - DAO prepare transactions (withdraw phase 1)
//! - DAO withdraw transactions (withdraw phase 2)
//! - Signing utilities for quantum-resistant locks

pub mod dao;
pub mod signing;
pub mod transfer;
pub mod utils;

pub use dao::{QpDaoDepositBuilder, QpDaoPrepareBuilder, QpDaoWithdrawBuilder};
pub use signing::{
    fetch_hardware_signing_context, fetch_header_deps, fetch_input_cells, fetch_prev_txs,
    fill_witness, resubmit, send_transaction, SUBMIT_WAIT,
};
pub use transfer::QpTransferBuilder;

use ckb_types::{
    core::{HeaderView, TransactionView},
    H256,
};

/// Chain data a hardware signer needs to verify transaction inputs and DAO
/// header dependencies without trusting host-supplied capacities.
pub struct HardwareSigningContext {
    /// Full transaction for every input's previous-output transaction hash.
    pub prev_txs: std::collections::HashMap<H256, TransactionView>,
    /// Block that committed each previous transaction, when available.
    pub prev_tx_block_hashes: std::collections::HashMap<H256, H256>,
    /// Full headers in exactly the same order as the transaction's header deps.
    pub headers: Vec<HeaderView>,
}

/// What became of a signed transaction, as learned from sending it or
/// looking it up by hash.
#[derive(Debug)]
pub enum SendOutcome {
    /// The node has the transaction: it accepted it, or already had it.
    Accepted,
    /// The node refused the transaction.
    Rejected(String),
    /// No usable answer: the node may or may not have the transaction.
    Uncertain(String),
}
