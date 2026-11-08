#!/bin/bash
set -euo pipefail
umask 077
export PYTHONDONTWRITEBYTECODE=1
export CARGO_NET_OFFLINE=true
export MACOSX_DEPLOYMENT_TARGET=15.7.4
source_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
[[ "$(uname -s)" = Darwin && "$(uname -m)" = arm64 ]] || { echo 'requires Apple Silicon macOS' >&2; exit 2; }
if [[ $# -gt 1 ]]; then echo 'usage: build.sh [new-absolute-output-directory]' >&2; exit 2; fi
if [[ $# -eq 1 ]]; then
  output_dir="$1"
  [[ "$output_dir" = /* && ! -e "$output_dir" && ! -L "$output_dir" ]] || { echo 'output must be a new absolute directory' >&2; exit 2; }
  mkdir -- "$output_dir"
else
  output_dir="$(mktemp -d /private/tmp/uipilot-quickjs-probe.XXXXXX)"
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$output_dir/target}"
printf 'Output: %s\n' "$output_dir"
cargo test --manifest-path "$source_dir/Cargo.toml" --offline --locked > "$output_dir/unit-tests.log" 2>&1
cargo build --manifest-path "$source_dir/Cargo.toml" --offline --locked > "$output_dir/build.log" 2>&1
cargo tree --manifest-path "$source_dir/Cargo.toml" --offline --locked -e features > "$output_dir/features.log"
cargo metadata --manifest-path "$source_dir/Cargo.toml" --offline --locked --format-version 1 --filter-platform aarch64-apple-darwin > "$output_dir/cargo-metadata.json"
cp "$CARGO_TARGET_DIR/debug/uipilot-quickjs-runtime-probe" "$output_dir/helper"
/usr/bin/codesign --force --sign - --identifier com.uipilot.internal.quickjs-probe "$output_dir/helper"
/usr/bin/codesign --verify --strict "$output_dir/helper"
UIPILOT_QUICKJS_HELPER="$output_dir/helper" UIPILOT_QUICKJS_RECORDS="$output_dir/resources.json" python3 "$source_dir/verify.py" -v > "$output_dir/native-tests.log" 2>&1
UIPILOT_QUICKJS_HELPER="$output_dir/helper" python3 "$source_dir/protocol_test.py" -v > "$output_dir/protocol-tests.log" 2>&1
/usr/bin/file "$output_dir/helper" > "$output_dir/macho.log"
/usr/bin/otool -L "$output_dir/helper" > "$output_dir/frameworks.log"
rustc --version --verbose > "$output_dir/rust-version.log"
xcrun clang --version > "$output_dir/clang-version.log"
/usr/bin/git -C "$source_dir" rev-parse HEAD > "$output_dir/source-base-commit.txt"
python3 "$source_dir/evidence.py" "$output_dir"
cat "$output_dir/unit-tests.log" "$output_dir/native-tests.log" "$output_dir/protocol-tests.log"
printf 'Built and tested headless (no GUI/media/host permission calls): %s\n' "$output_dir"
