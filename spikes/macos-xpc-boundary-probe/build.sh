#!/bin/bash
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1
here=$(cd "$(dirname "$0")" && pwd)
out=$(mktemp -d /private/tmp/uipilot-xpc-probe.XXXXXX)
app="$out/XpcBoundaryProbe.app"
service="$app/Contents/XPCServices/BoundaryService.xpc"
mkdir -p "$app/Contents/MacOS" "$service/Contents/MacOS"
cp "$here/Host-Info.plist" "$app/Contents/Info.plist"
cp "$here/Service-Info.plist" "$service/Contents/Info.plist"
sdk=$(xcrun --sdk macosx --show-sdk-path)
flags=(-isysroot "$sdk" -arch arm64 -mmacosx-version-min=15.7.4 -Wall -Wextra -Werror -fobjc-arc -fblocks)
common=("$here/native/protocol.m" "$here/native/adapter.m" "$here/native/preflight.m" "$here/native/policy.c")
xcrun clang "${flags[@]}" "${common[@]}" "$here/native/host.m" -framework Foundation -framework Security -o "$app/Contents/MacOS/XpcBoundaryProbe"
xcrun clang "${flags[@]}" "${common[@]}" "$here/native/service.m" -framework Foundation -framework Security -o "$service/Contents/MacOS/BoundaryService"
xcrun clang -arch arm64 -mmacosx-version-min=15.7.4 -Wall -Wextra -Werror "$here/native/policy.c" "$here/native/policy_test.c" -o "$out/policy-test"
xcrun clang "${flags[@]}" "$here/native/protocol.m" "$here/native/preflight.m" "$here/native/local_test.m" -framework Foundation -framework Security -o "$out/local-test"
"$out/policy-test" | tee "$out/policy.log"
"$out/local-test" | tee "$out/local.log"
# Build-only even with a future explicit identity. No --run/service execution.
identity=${UIPILOT_XPC_SIGN_IDENTITY:--}
codesign --force --sign "$identity" --entitlements "$here/Service.entitlements" "$service"
codesign --force --sign "$identity" "$app"
python3 "$here/validator.py" "$app" | tee "$out/validation.json"
UIPILOT_XPC_TEST_APP="$app" python3 "$here/validator_test.py" > "$out/validator-tests.log" 2>&1
set +e
"$app/Contents/MacOS/XpcBoundaryProbe" --preflight > "$out/preflight.json"
result=$?
"$app/Contents/MacOS/XpcBoundaryProbe" > "$out/default-preflight.json"
default_result=$?
set -e
test "$result" = "$default_result"
cmp "$out/preflight.json" "$out/default-preflight.json"
python3 - "$out/preflight.json" "$result" "$identity" <<'PY'
import json,sys
value=json.load(open(sys.argv[1]))
assert value['connectionsCreated']==0
if sys.argv[3]=='-':
    assert value['status']=='blockedSigningIdentity' and sys.argv[2]=='3'
else:
    assert (value['status'],int(sys.argv[2])) in [('eligibleNotConnected',0),('blockedSigningIdentity',3)]
PY
xcrun clang --version > "$out/toolchain.txt"
xcrun --sdk macosx --show-sdk-version >> "$out/toolchain.txt"
python3 --version >> "$out/toolchain.txt"
codesign -d --verbose=4 "$app" > "$out/host-signature.txt" 2>&1
codesign -d --verbose=4 "$service" > "$out/service-signature.txt" 2>&1
codesign -d --entitlements - --xml "$app" > "$out/host-entitlements.plist" 2>/dev/null
codesign -d --entitlements - --xml "$service" > "$out/service-entitlements.plist" 2>/dev/null
python3 - "$here" "$out" <<'PY'
from pathlib import Path
import hashlib,sys
source,out=map(Path,sys.argv[1:])
def records(root):
    return ''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(root)}\n' for p in sorted(root.rglob('*')) if p.is_file() and '__pycache__' not in p.parts)
(out/'source-SHA256SUMS').write_text(records(source))
(out/'artifact-SHA256SUMS').write_text(records(out))
PY
printf '%s\n' "$out"
