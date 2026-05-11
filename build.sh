#!/usr/bin/env bash
# Build, bundle, and sign the QPV2 GUI app for macOS.
# Delegates to bundle.sh (build + .app creation) then sign.sh (codesign).
set -euo pipefail

if [[ "${1:-}" == "--release" ]]; then
	cargo build --release
	./crates/qpv2-gui/scripts/bundle.sh --release --profile ~/Desktop/Quantum_Purse_Wallet_Developer_ID.provisionprofile
	./crates/qpv2-gui/scripts/sign.sh --release
else
	cargo build
	./crates/qpv2-gui/scripts/bundle.sh --profile ~/Desktop/Quantum_Purse_Wallet_Developer_ID.provisionprofile
	./crates/qpv2-gui/scripts/sign.sh
fi
