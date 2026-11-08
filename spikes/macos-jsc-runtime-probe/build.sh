#!/bin/bash
set -euo pipefail
umask 077
export PYTHONDONTWRITEBYTECODE=1
source_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
if [[ $# -gt 1 ]]; then echo 'usage: build.sh [new-absolute-output-directory]' >&2; exit 2; fi
if [[ $# -eq 1 ]]; then
  output_dir="$1"
  [[ "$output_dir" = /* && ! -e "$output_dir" && ! -L "$output_dir" ]] || { echo 'output must be a new absolute directory' >&2; exit 2; }
  mkdir -- "$output_dir"
else
  output_dir="$(mktemp -d /private/tmp/uipilot-jsc-probe.XXXXXX)"
fi
xcrun clang -arch arm64 -mmacosx-version-min=15.7.4 -fobjc-arc -fblocks -Wall -Wextra -Werror \
  -framework Foundation -framework JavaScriptCore "$source_dir/helper.m" -o "$output_dir/jsc-probe-helper"
/usr/bin/codesign --force --sign - --identifier com.uipilot.internal.jsc-probe "$output_dir/jsc-probe-helper"
/usr/bin/codesign --verify --strict "$output_dir/jsc-probe-helper"
(cd "$source_dir" && UIPILOT_JSC_HELPER="$output_dir/jsc-probe-helper" python3 -m unittest -v test_supervisor.py) > "$output_dir/tests.log" 2>&1
/usr/bin/file "$output_dir/jsc-probe-helper" > "$output_dir/macho.log"
/usr/bin/otool -L "$output_dir/jsc-probe-helper" > "$output_dir/frameworks.log"
(cd "$source_dir" && /usr/bin/shasum -a 256 helper.m supervisor.py test_supervisor.py build.sh README.md) > "$output_dir/source-SHA256SUMS"
/usr/bin/git -C "$source_dir" rev-parse HEAD > "$output_dir/source-base-commit.txt"
cat "$output_dir/tests.log"
printf 'Built and tested headless (no GUI/media): %s\n' "$output_dir"
