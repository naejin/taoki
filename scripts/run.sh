#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$DIR/target/release/taoki"
if [ ! -f "$BIN" ]; then
  cargo build --release --manifest-path "$DIR/Cargo.toml" >&2
fi
exec "$BIN" "$@"
