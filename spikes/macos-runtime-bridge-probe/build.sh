#!/bin/bash
set -euo pipefail
umask 077
export PYTHONDONTWRITEBYTECODE=1 CARGO_NET_OFFLINE=true MACOSX_DEPLOYMENT_TARGET=15.7.4
source_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
[[ "$(uname -s)" = Darwin && "$(uname -m)" = arm64 ]] || { echo 'requires Apple Silicon macOS' >&2; exit 2; }
if [[ $# -gt 1 ]]; then echo 'usage: build.sh [new-absolute-output-directory]' >&2; exit 2; fi
if [[ $# -eq 1 ]]; then
  output_dir="$1"
  [[ "$output_dir" = /* && ! -e "$output_dir" && ! -L "$output_dir" ]] || { echo 'output must be a new absolute directory' >&2; exit 2; }
  mkdir -- "$output_dir"
else
  output_dir="$(mktemp -d /private/tmp/uipilot-bridge-probe.XXXXXX)"
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$output_dir/target}"
printf 'Output: %s\n' "$output_dir"
cargo build --manifest-path "$source_dir/Cargo.toml" --offline --locked > "$output_dir/build.log" 2>&1
cp "$CARGO_TARGET_DIR/debug/uipilot-runtime-bridge-probe" "$output_dir/helper"
/usr/bin/codesign --force --sign - --identifier com.uipilot.internal.runtime-bridge-probe "$output_dir/helper"
/usr/bin/codesign --verify --strict "$output_dir/helper"
# Process tests use THIS signed copy, not just Cargo's unsigned/differently signed target.
UIPILOT_BRIDGE_HELPER="$output_dir/helper" cargo test --manifest-path "$source_dir/Cargo.toml" --offline --locked -- --nocapture --test-threads=1 > "$output_dir/tests.log" 2>&1
cargo tree --manifest-path "$source_dir/Cargo.toml" --offline --locked -e features > "$output_dir/features.log"
cargo metadata --manifest-path "$source_dir/Cargo.toml" --offline --locked --format-version 1 --filter-platform aarch64-apple-darwin > "$output_dir/cargo-metadata.json"
/usr/bin/file "$output_dir/helper" > "$output_dir/macho.log"
/usr/bin/otool -L "$output_dir/helper" > "$output_dir/frameworks.log"
rustc --version --verbose > "$output_dir/rust-version.log"
/usr/bin/git -C "$source_dir" rev-parse HEAD > "$output_dir/source-base-commit.txt"
python3 "$source_dir/evidence.py" "$output_dir" "$CARGO_TARGET_DIR"
cat "$output_dir/tests.log"
printf 'Built and tested headless memory + private data services: %s\n' "$output_dir"
