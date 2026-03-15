#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$DIR/target/release/taoki"
if [ -f "$BIN" ]; then
  exec "$BIN" "$@"
elif [ -f "$DIR/Cargo.toml" ]; then
  cargo build --release --manifest-path "$DIR/Cargo.toml" >&2
  exec "$BIN" "$@"
else
  echo "Error: taoki binary not found. Re-run the install script to download it." >&2
  echo "  curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash" >&2
  exit 1
fi
