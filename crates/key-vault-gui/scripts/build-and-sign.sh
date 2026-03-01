#!/usr/bin/env bash
# Build, bundle, and sign the Key Vault GUI app for macOS.
#
# Prerequisites:
#   1. Apple Developer identity: "Developer ID Application: Pham Tung (KPSL53752R)"
#   2. A Developer ID provisioning profile with Associated Domains capability,
#      installed at ~/Library/Developer/Xcode/Provisioning Profiles/ or
#      embedded manually (see PROVISIONING_PROFILE below).
#   3. The apple-app-site-association file must be deployed to:
#      https://quantumpurse.github.io/.well-known/apple-app-site-association
#
# Usage:
#   ./crates/key-vault-gui/scripts/build-and-sign.sh [--release]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GUI_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(cd "$GUI_DIR/../.." && pwd)"

# Configuration.
BINARY_NAME="qpkv-gui"
BUNDLE_ID="io.github.quantumpurse.key-vault-gui"
APP_NAME="Key Vault"
SIGNING_IDENTITY="Developer ID Application: Pham Tung (KPSL53752R)"
TEAM_ID="KPSL53752R"
ENTITLEMENTS="$GUI_DIR/entitlements.plist"

# Parse arguments.
BUILD_TYPE="debug"
CARGO_FLAGS=""
if [[ "${1:-}" == "--release" ]]; then
	BUILD_TYPE="release"
	CARGO_FLAGS="--release"
fi

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

# Embed provisioning profile if available.
PROVISIONING_PROFILE=""
for profile in ~/Library/Developer/Xcode/Provisioning\ Profiles/*.provisionprofile; do
	if [ -f "$profile" ]; then
		# Check if this profile matches our bundle ID.
		if security cms -D -i "$profile" 2>/dev/null | grep -q "$BUNDLE_ID"; then
			PROVISIONING_PROFILE="$profile"
			break
		fi
	fi
done

if [ -n "$PROVISIONING_PROFILE" ]; then
	echo "==> Embedding provisioning profile: $PROVISIONING_PROFILE"
	cp "$PROVISIONING_PROFILE" "$APP_BUNDLE/Contents/embedded.provisionprofile"
else
	echo "==> WARNING: No provisioning profile found for $BUNDLE_ID."
	echo "    Passkey operations will fail without a provisioning profile."
	echo "    Create one at https://developer.apple.com/account/resources/profiles/"
	echo "    with Associated Domains capability for App ID: $TEAM_ID.$BUNDLE_ID"
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
