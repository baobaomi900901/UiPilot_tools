#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
xcrun --sdk macosx clang -arch arm64 -mmacosx-version-min=15.7.4 -fobjc-arc -Wall -Werror \
    -framework Foundation -framework WebKit "$here/native.m" -o "$tmp/native-probe"
test "$(lipo -archs "$tmp/native-probe")" = arm64
mkdir -p "$tmp/root/测试 App.app/Contents" "$tmp/outside.app/Contents"
python3 - "$tmp" <<'PY'
import pathlib, plistlib, sys
tmp = pathlib.Path(sys.argv[1])
for bundle, identifier in [(tmp/"root/测试 App.app", "test.probe.inside"), (tmp/"outside.app", "test.probe.outside")]:
    (bundle/"Contents/Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": identifier, "CFBundlePackageType": "APPL"}))
(tmp/"root/Linked.app").symlink_to(tmp/"outside.app", target_is_directory=True)
PY
"$tmp/native-probe" "$tmp/root" > "$tmp/fixture.json"
"$tmp/native-probe" /System/Applications > "$tmp/system.json"
if "$tmp/native-probe" "$tmp/missing"; then exit 1; fi
python3 - "$tmp" <<'PY'
import json, pathlib, sys
tmp = pathlib.Path(sys.argv[1])
fixture, system = [json.loads((tmp / name).read_text()) for name in ["fixture.json", "system.json"]]
assert fixture == {"wkWebViewClassLinked": True, "bundleIDs": ["test.probe.inside"]}, fixture
assert system["wkWebViewClassLinked"] and "com.apple.calculator" in system["bundleIDs"], system
print("PASS: arm64 WebKit class linkage, Unicode .app identity, external link exclusion, missing root rejection")
print(f"PASS: {len(system['bundleIDs'])} system app bundles include Calculator")
PY
