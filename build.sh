#!/usr/bin/env bash
if [[ "${1:-}" == "--release" ]]; then
	cargo build --release
	./crates/qpv2-gui/scripts/build-and-sign.sh --release --profile ~/Desktop/Quantum_Purse_Wallet_Developer_ID.provisionprofile
else
	cargo build
	./crates/qpv2-gui/scripts/build-and-sign.sh --profile ~/Desktop/Quantum_Purse_Wallet_Developer_ID.provisionprofile
fi
