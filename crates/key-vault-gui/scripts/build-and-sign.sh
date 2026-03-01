#!/usr/bin/env bash
# Build, bundle, and sign the Key Vault GUI app for macOS.
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
#   ./crates/key-vault-gui/scripts/build-and-sign.sh [--release] [--profile <path>]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GUI_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(cd "$GUI_DIR/../.." && pwd)"

# Configuration.
BINARY_NAME="qpkv-gui"
BUNDLE_ID="org.quantumpurse.wallet"
APP_NAME="Key Vault"
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
cargo build -p key-vault-gui $CARGO_FLAGS

echo "==> Creating app bundle at $APP_BUNDLE..."
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

# Copy binary.
cp "$TARGET_DIR/$BINARY_NAME" "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"

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
codesign --force --deep --sign "$SIGNING_IDENTITY" \
	--entitlements "$ENTITLEMENTS" \
	--options runtime \
	"$APP_BUNDLE"

echo "==> Verifying signature..."
codesign --verify --deep --strict "$APP_BUNDLE"
echo "==> Signature valid."

echo ""
echo "==> Done! App bundle: $APP_BUNDLE"
echo "    Run with: open \"$APP_BUNDLE\""
echo ""
echo "==> To check entitlements:"
echo "    codesign -d --entitlements - \"$APP_BUNDLE\""
