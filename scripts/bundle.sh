#!/bin/bash
# Assembles dist/helm.app from the release build (specs/update.md §3).
# Ad-hoc signature by default; override with CODESIGN_IDENTITY="Developer ID …".
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed -E 's/^version *= *"([^"]+)".*/\1/')"
BUNDLE_ID="io.github.davidbonan.helm"
IDENTITY="${CODESIGN_IDENTITY:--}"
APP="$ROOT/dist/helm.app"

[ -n "$VERSION" ] || { echo "error: version not found in Cargo.toml" >&2; exit 1; }

cargo build --release --manifest-path "$ROOT/Cargo.toml"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$ROOT/target/release/helm" "$APP/Contents/MacOS/helm"
cp "$ROOT/assets/brand/icon.icns" "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleExecutable</key>
	<string>helm</string>
	<key>CFBundleIconFile</key>
	<string>icon</string>
	<key>CFBundleIdentifier</key>
	<string>${BUNDLE_ID}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Helm</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>${VERSION}</string>
	<key>CFBundleVersion</key>
	<string>${VERSION}</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
PLIST

codesign --force --sign "$IDENTITY" "$APP"
codesign --verify --strict "$APP"

echo "bundled $APP (v$VERSION, $BUNDLE_ID, identity: $IDENTITY)"
