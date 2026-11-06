#!/bin/sh
# Read-only device check for candidate a79f612. No bundle is mounted or opened.
set -eu
expected=3c640e2b8da704d438f59afcb437cc06509273b1bcff05e792e44e04cede77a3
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
  echo 'Usage: sh preflight-macos-device.sh PATH_TO_CANDIDATE.dmg' >&2
  exit 2
fi
actual=$(shasum -a 256 < "$1")
actual=${actual%% *}
if [ "$actual" != "$expected" ]; then
  echo "FAIL: DMG digest mismatch: $actual" >&2
  exit 1
fi
arch=$(uname -m)
chip=$(sysctl -n machdep.cpu.brand_string)
os=$(sw_vers -productVersion)
build=$(sw_vers -buildVersion)
printf 'Candidate: a79f612\nDMG SHA-256: %s\nChip: %s\nArchitecture: %s\nmacOS: %s (%s)\n' "$actual" "$chip" "$arch" "$os" "$build"
family=$(printf '%s\n' "$chip" | sed -nE 's/^Apple M([1-9][0-9]*)( .*)?$/\1/p')
if [ "$arch" != arm64 ] || [ -z "$family" ]; then
  echo 'FAIL: run on native Apple Silicon in an arm64 shell' >&2
  exit 1
fi
if [ "$family" -eq 1 ] && [ "$os" = 15.7.4 ]; then
  echo 'Device row: M1 family, minimum OS'
elif [ "$family" -gt 1 ] && printf '%s\n' "$os" | awk -F. '{ exit !(($1+0)>15 || (($1+0)==15 && (($2+0)>7 || (($2+0)==7 && ($3+0)>4)))) }'; then
  echo 'Device row: later Apple Silicon, newer OS'
else
  echo 'This device does not qualify for either required release row.' >&2
  exit 1
fi
echo 'Preflight passed. Installation, launch and interaction results must be recorded separately.'
