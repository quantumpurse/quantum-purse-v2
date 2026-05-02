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
# Toggle: when "false", the pinentry-mac.app sub-bundle is NOT
# copied/signed into qpv2.app. The password flow will surface a
# runtime error pointing at the missing path; Touch ID is unaffected.
# Useful for testing the bundle's behavior in pinentry's absence.
BUNDLE_PINENTRY="true"

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

if [ "$BUNDLE_PINENTRY" = "true" ]; then
    # Locate the brew-installed pinentry-mac.app. The file at
    # /opt/homebrew/bin/pinentry-mac is a 111-byte shell wrapper that
    # exec's into a Cellar-versioned .app — bundling the wrapper alone
    # gives end users a binary that fails to find its hardcoded path.
    # We resolve the wrapper to the Cellar version dir and copy the full
    # .app, which carries the Mach-O plus its Resources (nibs, etc.).
    PINENTRY_NAME="pinentry-mac"
    PINENTRY_APP_NAME="pinentry-mac.app"
    if [ -x "/opt/homebrew/bin/$PINENTRY_NAME" ]; then
        PINENTRY_WRAPPER="/opt/homebrew/bin/$PINENTRY_NAME"
    elif [ -x "/usr/local/bin/$PINENTRY_NAME" ]; then
        PINENTRY_WRAPPER="/usr/local/bin/$PINENTRY_NAME"
    else
        echo "ERROR: $PINENTRY_NAME not found in /opt/homebrew/bin or /usr/local/bin."
        echo "       Run: brew install pinentry-mac"
        exit 1
    fi
    # `realpath`/`readlink -f` follows the symlink chain into the Cellar
    # version directory, where the .app sits as a sibling of bin/.
    PINENTRY_REAL="$(/usr/bin/python3 -c "import os,sys; print(os.path.realpath(sys.argv[1]))" "$PINENTRY_WRAPPER")"
    PINENTRY_VERSION_DIR="$(dirname "$(dirname "$PINENTRY_REAL")")"
    PINENTRY_APP_SRC="$PINENTRY_VERSION_DIR/$PINENTRY_APP_NAME"
    if [ ! -d "$PINENTRY_APP_SRC" ]; then
        echo "ERROR: $PINENTRY_APP_NAME not found at $PINENTRY_APP_SRC"
        echo "       Reinstall: brew reinstall pinentry-mac"
        exit 1
    fi
    echo "==> Using $PINENTRY_APP_NAME from $PINENTRY_APP_SRC"
else
    echo "==> Skipping pinentry-mac.app bundling (BUNDLE_PINENTRY=false)"
fi

echo "==> Creating app bundle at $APP_BUNDLE..."
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

# Copy binaries.
cp "$TARGET_DIR/$BINARY_NAME" "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"
cp "$LIGHT_CLIENT_BIN" "$APP_BUNDLE/Contents/MacOS/$LIGHT_CLIENT_NAME"
cp "$FULL_NODE_BIN" "$APP_BUNDLE/Contents/MacOS/$FULL_NODE_NAME"
if [ "$BUNDLE_PINENTRY" = "true" ]; then
    # Copy the full .app tree. brew installs read-only (0555); use -R to
    # preserve the structure, then chmod -R u+w so codesign can rewrite
    # the signature in place.
    cp -R "$PINENTRY_APP_SRC" "$APP_BUNDLE/Contents/MacOS/$PINENTRY_APP_NAME"
    chmod -R u+w "$APP_BUNDLE/Contents/MacOS/$PINENTRY_APP_NAME"

    # pinentry-mac links against three brew dylibs (libassuan,
    # libgpg-error, libintl). Hardened-runtime + library validation
    # rejects them at load time when the loading binary is re-signed
    # under a different Team ID — they're brew-signed, we sign with our
    # Developer ID. Bundle them inside the .app's Contents/Frameworks/,
    # rewrite the install names to @executable_path-relative paths, and
    # codesign the chain bottom-up so all members share our Team ID.
    PINENTRY_APP_DST="$APP_BUNDLE/Contents/MacOS/$PINENTRY_APP_NAME"
    PINENTRY_FRAMEWORKS="$PINENTRY_APP_DST/Contents/Frameworks"
    PINENTRY_BIN="$PINENTRY_APP_DST/Contents/MacOS/$PINENTRY_NAME"
    mkdir -p "$PINENTRY_FRAMEWORKS"

    DYLIB_ASSUAN_SRC="/opt/homebrew/opt/libassuan/lib/libassuan.9.dylib"
    DYLIB_GPGERR_SRC="/opt/homebrew/opt/libgpg-error/lib/libgpg-error.0.dylib"
    DYLIB_INTL_SRC="/opt/homebrew/opt/gettext/lib/libintl.8.dylib"
    for src in "$DYLIB_ASSUAN_SRC" "$DYLIB_GPGERR_SRC" "$DYLIB_INTL_SRC"; do
        if [ ! -f "$src" ]; then
            echo "ERROR: required dylib missing: $src"
            echo "       Reinstall brew deps: brew reinstall libassuan libgpg-error gettext"
            exit 1
        fi
        cp "$src" "$PINENTRY_FRAMEWORKS/$(basename "$src")"
    done
    chmod -R u+w "$PINENTRY_FRAMEWORKS"

    DYLIB_ASSUAN="$PINENTRY_FRAMEWORKS/libassuan.9.dylib"
    DYLIB_GPGERR="$PINENTRY_FRAMEWORKS/libgpg-error.0.dylib"
    DYLIB_INTL="$PINENTRY_FRAMEWORKS/libintl.8.dylib"

    # Rewrite LC_ID on each dylib so subsequent dyld lookups follow the
    # new @executable_path path rather than the original brew install path.
    install_name_tool -id "@executable_path/../Frameworks/libassuan.9.dylib" "$DYLIB_ASSUAN"
    install_name_tool -id "@executable_path/../Frameworks/libgpg-error.0.dylib" "$DYLIB_GPGERR"
    install_name_tool -id "@executable_path/../Frameworks/libintl.8.dylib" "$DYLIB_INTL"

    # Rewrite LC_LOAD_DYLIB entries through the chain.
    # libassuan → libgpg-error
    install_name_tool -change \
        "$DYLIB_GPGERR_SRC" \
        "@executable_path/../Frameworks/libgpg-error.0.dylib" \
        "$DYLIB_ASSUAN"
    # libgpg-error → libintl
    install_name_tool -change \
        "$DYLIB_INTL_SRC" \
        "@executable_path/../Frameworks/libintl.8.dylib" \
        "$DYLIB_GPGERR"
    # pinentry-mac → libassuan + libgpg-error
    install_name_tool -change \
        "$DYLIB_ASSUAN_SRC" \
        "@executable_path/../Frameworks/libassuan.9.dylib" \
        "$PINENTRY_BIN"
    install_name_tool -change \
        "$DYLIB_GPGERR_SRC" \
        "@executable_path/../Frameworks/libgpg-error.0.dylib" \
        "$PINENTRY_BIN"
fi

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
if [ "$BUNDLE_PINENTRY" = "true" ]; then
    # Sign the bundled dylibs leaf-first (libintl has no bundled deps;
    # libgpg-error depends on libintl; libassuan depends on libgpg-error),
    # then the inner Mach-O, then the .app bundle. Library validation now
    # passes because every member shares our Team ID. No entitlements
    # anywhere: the dialog binary and its deps need nothing beyond
    # standard Cocoa + pipe I/O. Hardened runtime is required for
    # notarization eligibility of every nested binary.
    codesign --force --sign "$SIGNING_IDENTITY" \
        --options runtime \
        "$DYLIB_INTL"
    codesign --force --sign "$SIGNING_IDENTITY" \
        --options runtime \
        "$DYLIB_GPGERR"
    codesign --force --sign "$SIGNING_IDENTITY" \
        --options runtime \
        "$DYLIB_ASSUAN"
    codesign --force --sign "$SIGNING_IDENTITY" \
        --options runtime \
        "$PINENTRY_BIN"
    codesign --force --sign "$SIGNING_IDENTITY" \
        --options runtime \
        "$PINENTRY_APP_DST"
fi
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
