#!/bin/bash
set -euo pipefail
umask 077
source_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
if [[ $# -gt 1 ]]; then echo 'usage: build.sh [new-absolute-output-directory]' >&2; exit 2; fi
if [[ $# -eq 1 ]]; then
  output_dir="$1"
  [[ "$output_dir" = /* && ! -e "$output_dir" && ! -L "$output_dir" ]] || { echo 'output must be a new absolute directory' >&2; exit 2; }
  mkdir -- "$output_dir"
else
  output_dir="$(mktemp -d /private/tmp/uipilot-media-probe.XXXXXX)"
fi
app_dir="$output_dir/UiPilot Media Probe.app"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
xcrun clang -std=c11 -Wall -Wextra -Werror "$source_dir/lifecycle.c" "$source_dir/lifecycle_test.c" -o "$output_dir/lifecycle-tests"
"$output_dir/lifecycle-tests" > "$output_dir/lifecycle-tests.log"
xcrun clang -arch arm64 -mmacosx-version-min=15.7.4 -fobjc-arc -fblocks -Wall -Wextra -Werror \
  -framework AppKit -framework WebKit "$source_dir/main.m" "$source_dir/lifecycle.c" -o "$app_dir/Contents/MacOS/media-probe"
cp "$source_dir/fixture.html" "$app_dir/Contents/Resources/fixture.html"
cat > "$app_dir/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.uipilot.internal.media-probe</string>
<key>CFBundleName</key><string>UiPilot Media Probe</string>
<key>CFBundleExecutable</key><string>media-probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1</string>
<key>LSMinimumSystemVersion</key><string>15.7.4</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
/usr/bin/plutil -lint "$app_dir/Contents/Info.plist"
/usr/bin/codesign --force --sign - "$app_dir"
/usr/bin/codesign --verify --strict --verbose=2 "$app_dir" 2> "$output_dir/signature-check.log"
/usr/bin/file "$app_dir/Contents/MacOS/media-probe" > "$output_dir/macho-check.log"
/usr/bin/otool -l "$app_dir/Contents/MacOS/media-probe" > "$output_dir/load-commands.log"
/usr/bin/ditto -c -k --keepParent "$app_dir" "$output_dir/UiPilot Media Probe.zip"
/usr/bin/shasum -a 256 "$output_dir/UiPilot Media Probe.zip" > "$output_dir/SHA256SUMS"
/usr/bin/git -C "$source_dir" rev-parse HEAD > "$output_dir/source-base-commit.txt"
(cd "$source_dir" && /usr/bin/shasum -a 256 main.m fixture.html lifecycle.c lifecycle.h lifecycle_test.c build.sh README.md) > "$output_dir/source-SHA256SUMS"
printf 'Built (not launched): %s\n' "$output_dir"
