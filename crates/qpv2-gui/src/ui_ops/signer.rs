//! DAO transaction building, signing, and sending.

use std::sync::mpsc;
use qpv2_core::KeyVault;
use crate::types::{TransactionKind, TransactionStatus};
use crate::App;

impl App {
    /// After Touch ID returns the PRF output for DAO, sign and send.
    pub(crate) fn sign_and_send(
        &mut self,
        kind: TransactionKind,
        prf_output: &qpv2_core::SecureVec,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    ) {
        use ckb_types::prelude::*;
        use qpv2_core::types::AuthKey;

        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Failed to read variant: {}", e));
                return;
            }
        };

        let packed_tx = unsigned_tx.data();
        let mut hasher = ckb_fips205_utils::Hasher::message_hasher();

        let gen_inputs: Vec<(
            ckb_gen_types::packed::CellOutput,
            ckb_gen_types::bytes::Bytes,
        )> = input_cells
            .iter()
            .map(|(output, data)| {
                let raw = output.as_slice();
                let gen_output =
                    ckb_gen_types::packed::CellOutput::from_slice(raw).expect("valid CellOutput");
                (
                    gen_output,
                    ckb_gen_types::bytes::Bytes::copy_from_slice(data),
                )
            })
            .collect();

        let gen_tx = ckb_gen_types::packed::Transaction::from_slice(packed_tx.as_slice())
            .expect("valid Transaction");

        if let Err(e) =
            ckb_fips205_utils::ckb_tx_message_all_from_mock_tx::generate_ckb_tx_message_all(
                &gen_tx,
                &gen_inputs,
                ckb_fips205_utils::ckb_tx_message_all_from_mock_tx::ScriptOrIndex::Index(0),
                &mut hasher,
            )
        {
            self.tx_status =
                TransactionStatus::Error(format!("Failed to compute tx message: {:?}", e));
            return;
        }
        let message = hasher.hash().to_vec();

        let vault = KeyVault::new(variant);
        let signature_bytes = match vault.ckb_sign(AuthKey::CryptoKey(key), lock_args, message) {
            Ok(sig) => sig,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Signing failed: {}", e));
                return;
            }
        };

        let signed_tx = match node_manager::fill_witness(unsigned_tx, 0, signature_bytes) {
            Ok(tx) => tx,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Failed to fill witness: {}", e));
                return;
            }
        };

        self.tx_status = TransactionStatus::Sending;
        let node_config = self.node_config.clone();
        let (tx_send, rx_send) = mpsc::channel();
        self.transaction_send_rx = Some(rx_send);

        // spawn a thread to handle transaction submission.
        std::thread::spawn(move || {
            let rpc = node_manager::connect(&node_config);
            let result = node_manager::send_transaction(rpc.as_ref(), &signed_tx)
                .map(|hash| format!("{:#x}", hash))
                .map_err(|e| format!("Failed to send transaction: {}", e));
            let _ = tx_send.send((kind, result));
        });
    }
}
