#!/usr/bin/env bash
# Build, bundle, and sign the QPV2 GUI app for macOS.
#
# Prerequisites:
#   1. Apple Developer identity: "Developer ID Application: Pham Tung (KPSL53752R)"
#   2. A Developer ID provisioning profile with Associated Domains capability,
#      installed at ~/Library/Developer/Xcode/Provisioning Profiles/ or
#      embedded manually (see PROVISIONING_PROFILE below).
#   3. The apple-app-site-association file must be deployed to:
#      https://quantumpurse.org/.well-known/apple-app-site-association
#
# Usage:
#   ./crates/qpv2-gui/scripts/build-and-sign.sh [--release] [--profile <path>]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GUI_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(cd "$GUI_DIR/../.." && pwd)"

# Configuration.
BINARY_NAME="qpv2-gui"
BUNDLE_ID="org.quantumpurse.wallet"
APP_NAME="qpv2"
SIGNING_IDENTITY="Developer ID Application: Pham Tung (KPSL53752R)"
TEAM_ID="KPSL53752R"
ENTITLEMENTS="$GUI_DIR/entitlements.plist"

# Parse arguments.
BUILD_TYPE="debug"
CARGO_FLAGS=""
PROFILE_PATH=""
while [[ $# -gt 0 ]]; do
	case "$1" in
		--release)
			BUILD_TYPE="release"
			CARGO_FLAGS="--release"
			shift
			;;
		--profile)
			PROFILE_PATH="$2"
			shift 2
			;;
		*)
			echo "Unknown argument: $1"
			echo "Usage: $0 [--release] [--profile <path>]"
			exit 1
			;;
	esac
done

TARGET_DIR="$PROJECT_ROOT/target/$BUILD_TYPE"
APP_BUNDLE="$TARGET_DIR/$APP_NAME.app"

echo "==> Building $BINARY_NAME ($BUILD_TYPE)..."
cargo build -p qpv2-gui $CARGO_FLAGS

# Build the bundled ckb-light-client from the vendored submodule. It lives
# in a separate Cargo workspace so we invoke it via --manifest-path.
LIGHT_CLIENT_NAME="ckb-light-client"
LIGHT_CLIENT_SRC="$PROJECT_ROOT/vendor/ckb-light-client"
LIGHT_CLIENT_BIN="$LIGHT_CLIENT_SRC/target/$BUILD_TYPE/$LIGHT_CLIENT_NAME"
echo "==> Building $LIGHT_CLIENT_NAME ($BUILD_TYPE)..."
cargo build -p $LIGHT_CLIENT_NAME $CARGO_FLAGS \
    --manifest-path "$LIGHT_CLIENT_SRC/Cargo.toml"

if [ ! -f "$LIGHT_CLIENT_BIN" ]; then
    echo "ERROR: $LIGHT_CLIENT_NAME binary not found at $LIGHT_CLIENT_BIN"
    exit 1
fi

# Build the bundled ckb full-node binary. Same submodule pattern as the
# light client. Heavy build — minutes on a clean target dir, multi-GB
# build artifacts. Skipped automatically when re-running incrementally.
FULL_NODE_NAME="ckb"
FULL_NODE_SRC="$PROJECT_ROOT/vendor/ckb"
FULL_NODE_BIN="$FULL_NODE_SRC/target/$BUILD_TYPE/$FULL_NODE_NAME"
echo "==> Building $FULL_NODE_NAME ($BUILD_TYPE)..."
cargo build -p $FULL_NODE_NAME $CARGO_FLAGS \
    --manifest-path "$FULL_NODE_SRC/Cargo.toml"

if [ ! -f "$FULL_NODE_BIN" ]; then
    echo "ERROR: $FULL_NODE_NAME binary not found at $FULL_NODE_BIN"
    exit 1
fi

echo "==> Creating app bundle at $APP_BUNDLE..."
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

# Copy binaries.
cp "$TARGET_DIR/$BINARY_NAME" "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"
cp "$LIGHT_CLIENT_BIN" "$APP_BUNDLE/Contents/MacOS/$LIGHT_CLIENT_NAME"
cp "$FULL_NODE_BIN" "$APP_BUNDLE/Contents/MacOS/$FULL_NODE_NAME"

# Create Info.plist.
cat > "$APP_BUNDLE/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>${APP_NAME}</string>
	<key>CFBundleExecutable</key>
	<string>${BINARY_NAME}</string>
	<key>CFBundleIdentifier</key>
	<string>${BUNDLE_ID}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>${APP_NAME}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>0.1.0</string>
	<key>CFBundleVersion</key>
	<string>1</string>
	<key>LSMinimumSystemVersion</key>
	<string>15.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
PLIST

# Embed provisioning profile.
if [ -n "$PROFILE_PATH" ]; then
	if [ ! -f "$PROFILE_PATH" ]; then
		echo "ERROR: Provisioning profile not found: $PROFILE_PATH"
		exit 1
	fi
	echo "==> Embedding provisioning profile: $PROFILE_PATH"
	cp "$PROFILE_PATH" "$APP_BUNDLE/Contents/embedded.provisionprofile"
else
	echo "==> WARNING: No provisioning profile specified."
	echo "    Passkey operations will fail without a provisioning profile."
	echo "    Use --profile <path> to provide one."
fi

echo "==> Signing with identity: $SIGNING_IDENTITY"
# Sign inner binaries first, then the bundle (Apple recommends against
# --deep). The light client and full node are standalone TCP/HTTP daemons
# with no keychain or passkey access, so they get hardened-runtime but
# no entitlements.
codesign --force --sign "$SIGNING_IDENTITY" \
	--options runtime \
	"$APP_BUNDLE/Contents/MacOS/$LIGHT_CLIENT_NAME"
codesign --force --sign "$SIGNING_IDENTITY" \
	--options runtime \
	"$APP_BUNDLE/Contents/MacOS/$FULL_NODE_NAME"
codesign --force --sign "$SIGNING_IDENTITY" \
	--entitlements "$ENTITLEMENTS" \
	--options runtime \
	"$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"
codesign --force --sign "$SIGNING_IDENTITY" \
	--entitlements "$ENTITLEMENTS" \
	--options runtime \
	"$APP_BUNDLE"

echo "==> Verifying signature..."
codesign --verify --strict "$APP_BUNDLE"
echo "==> Signature valid."

echo ""
echo "==> Done! App bundle: $APP_BUNDLE"
echo "    Run with: open \"$APP_BUNDLE\""
echo ""
echo "==> To check entitlements:"
echo "    codesign -d --entitlements - \"$APP_BUNDLE\""
