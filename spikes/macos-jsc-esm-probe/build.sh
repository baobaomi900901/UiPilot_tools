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
  output_dir="$(mktemp -d /private/tmp/uipilot-jsc-esm.XXXXXX)"
fi
printf 'Output: %s\n' "$output_dir"
node --test "$source_dir/builder.test.mjs" > "$output_dir/builder-tests.log" 2>&1
node "$source_dir/generate.mjs" "$output_dir" > "$output_dir/bundle.log" 2>&1
for fixture_dir in "$output_dir"/*/; do
  xcrun clang -arch arm64 -mmacosx-version-min=15.7.4 -fobjc-arc -fblocks -Wall -Wextra -Werror \
    -framework Foundation -framework JavaScriptCore -I "$fixture_dir" "$source_dir/helper.m" -o "$fixture_dir/helper"
  /usr/bin/codesign --force --sign - --identifier com.uipilot.internal.jsc-esm-probe "$fixture_dir/helper"
  /usr/bin/codesign --verify --strict "$fixture_dir/helper"
done
UIPILOT_ESM_OUTPUT="$output_dir" python3 "$source_dir/verify.py" -v > "$output_dir/native-tests.log" 2>&1
/usr/bin/git -C "$source_dir" rev-parse HEAD > "$output_dir/source-base-commit.txt"
/usr/bin/file "$output_dir/demo-return/helper" > "$output_dir/macho.log"
/usr/bin/otool -L "$output_dir/demo-return/helper" > "$output_dir/frameworks.log"
python3 "$source_dir/evidence.py" "$output_dir"
cat "$output_dir/builder-tests.log" "$output_dir/native-tests.log"
printf 'Built and tested headless (no GUI/media/host permission calls): %s\n' "$output_dir"
