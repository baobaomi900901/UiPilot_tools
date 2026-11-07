#!/bin/sh
# Build a standalone test kit. The signed QA executable never launches UiPilot.
set -eu
if [ "$#" -ne 1 ] || [ -e "$1" ]; then
    echo 'Usage: sh scripts/build-macos-find-qa.sh NEW_OUTPUT.zip' >&2; exit 2
fi
output=$(cd -- "$(dirname -- "$1")" && pwd -P)/$(basename -- "$1")
root=$(CDPATH= cd -P -- "$(dirname -- "$0")/.." && pwd -P)
cd "$root"
stage=$(mktemp -d "${TMPDIR:-/tmp}/uipilot-spotlight-kit.XXXXXXXX")
trap 'rm -rf -- "$stage"' EXIT
target=aarch64-apple-darwin
export MACOSX_DEPLOYMENT_TARGET=15.7.4
cargo build --manifest-path src-tauri/Cargo.toml --release --locked --offline \
    --no-default-features --target "$target" --features macos-find-qa-probe --bin uipilot-find-system-qa
target_dir=$(cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 |
    node -pe "JSON.parse(require('fs').readFileSync(0, 'utf8')).target_directory")
kit="$stage/UiPilot Spotlight QA"
app="$kit/UiPilot Spotlight QA.app"
mkdir -p "$app/Contents/MacOS"
cp "$target_dir/$target/release/uipilot-find-system-qa" "$app/Contents/MacOS/"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIdentifier</key><string>com.uipilot.qa.spotlight</string>
<key>CFBundleExecutable</key><string>uipilot-find-system-qa</string>
<key>CFBundleName</key><string>UiPilot Spotlight QA</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>15.7.4</string>
</dict></plist>
PLIST
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
exe="$app/Contents/MacOS/uipilot-find-system-qa"
[ "$(lipo -archs "$exe")" = arm64 ]
[ "$(otool -l "$exe" | awk '$1 == "minos" {print $2}')" = "$MACOSX_DEPLOYMENT_TARGET" ]
[ -z "$(codesign --display --xml --entitlements - "$exe")" ]
cp scripts/run-spotlight-qa.command "$kit/run-spotlight-qa.command"
cp docs/releases/macos-spotlight-qa.md "$kit/使用说明.md"
chmod 755 "$kit/run-spotlight-qa.command"
ditto -c -k --keepParent "$kit" "$stage/kit.zip"
mv "$stage/kit.zip" "$output"
shasum -a 256 "$output"
