#!/usr/bin/env bash
if [[ "${1:-}" == "--release" ]]; then
	./crates/key-vault-gui/scripts/build-and-sign.sh --release --profile ~/Desktop/Quantum_Purse_Wallet_Developer_ID.provisionprofile
else
	./crates/key-vault-gui/scripts/build-and-sign.sh --profile ~/Desktop/Quantum_Purse_Wallet_Developer_ID.provisionprofile
fi
