#!/usr/bin/env bash
# Build QPV2 CLI or GUI.
#
# Usage:
#   ./build.sh cli              # Build CLI (debug)
#   ./build.sh cli --release    # Build CLI (release + codesign)
#   ./build.sh gui              # Build GUI (debug, bundle + sign)
#   ./build.sh gui --release    # Build GUI (release, bundle + sign)
set -euo pipefail

TARGET="${1:-}"
RELEASE="${2:-}"

SIGN_IDENTITY="Developer ID Application: Pham Tung (KPSL53752R)"
ENTITLEMENTS="crates/qpv2-gui/entitlements.plist"

case "$TARGET" in
	cli)
		if [[ "$RELEASE" == "--release" ]]; then
			cargo build -p qpv2-cli --release
			codesign -s "$SIGN_IDENTITY" --entitlements "$ENTITLEMENTS" --force target/release/qpv2-cli
			echo "Built and signed: target/release/qpv2-cli"
		else
			cargo build -p qpv2-cli
			codesign -s "$SIGN_IDENTITY" --entitlements "$ENTITLEMENTS" --force target/debug/qpv2-cli
			echo "Built and signed: target/debug/qpv2-cli"
		fi
		;;
	gui)
		if [[ "$RELEASE" == "--release" ]]; then
			./crates/qpv2-gui/scripts/bundle.sh --release --profile ~/Desktop/Quantum_Purse_Wallet_Developer_ID.provisionprofile
			./crates/qpv2-gui/scripts/sign.sh --release
		else
			./crates/qpv2-gui/scripts/bundle.sh --profile ~/Desktop/Quantum_Purse_Wallet_Developer_ID.provisionprofile
			./crates/qpv2-gui/scripts/sign.sh
		fi
		;;
	*)
		echo "Usage: ./build.sh <cli|gui> [--release]"
		exit 1
		;;
esac
