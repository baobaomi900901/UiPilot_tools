#!/bin/sh
set -eu
if [ "$#" -ne 1 ] || [ ! -d "$1" ]; then
  echo "Usage: build-native-window.sh existing-output-directory" >&2
  exit 2
fi
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
app="$1/UiPilot Native Focus Probe.app"
if [ -e "$app" ]; then echo "Output already exists" >&2; exit 2; fi
mkdir -p "$app/Contents/MacOS"
plutil -create xml1 "$app/Contents/Info.plist"
for pair in 'CFBundleExecutable=NativeFocusProbe' 'CFBundleIdentifier=com.uipilot.native-focus-probe'   'CFBundleName=UiPilot Native Focus Probe' 'CFBundlePackageType=APPL' 'CFBundleVersion=1'   'CFBundleShortVersionString=0.1' 'LSMinimumSystemVersion=15.7.4'; do
  plutil -insert "${pair%%=*}" -string "${pair#*=}" "$app/Contents/Info.plist"
done
xcrun --sdk macosx clang -arch arm64 -mmacosx-version-min=15.7.4 -O2 -fobjc-arc -Wall -Werror     -framework AppKit -framework WebKit -framework Carbon "$here/native-window.m" -o "$app/Contents/MacOS/NativeFocusProbe"
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict "$app"
test "$(lipo -archs "$app/Contents/MacOS/NativeFocusProbe")" = arm64
printf 'Built %s\n' "$app"
