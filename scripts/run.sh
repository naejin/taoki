#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$DIR/target/release/taoki"

# 1. Binary exists — run it
if [ -f "$BIN" ]; then
  exec "$BIN" "$@"
fi

# 2. Source checkout with Rust — build from source
if [ -f "$DIR/Cargo.toml" ] && command -v cargo >/dev/null 2>&1; then
  cargo build --release --manifest-path "$DIR/Cargo.toml" >&2
  exec "$BIN" "$@"
fi

# 3. Download pre-built binary from GitHub Releases
REPO="naejin/taoki"
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *) echo "Error: unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)  ARCH="x86_64" ;;
  arm64|aarch64)  ARCH="aarch64" ;;
  *) echo "Error: unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

ARTIFACT="taoki-${PLATFORM}-${ARCH}.tar.gz"

# Get version from plugin.json if available, otherwise fetch latest
VERSION=""
if [ -f "$DIR/.claude-plugin/plugin.json" ] && command -v python3 >/dev/null 2>&1; then
  VERSION=$(python3 -c "import json; print('v'+json.load(open('$DIR/.claude-plugin/plugin.json'))['version'])" 2>/dev/null || true)
fi
if [ -z "$VERSION" ]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')
fi

if [ -z "$VERSION" ]; then
  echo "Error: could not determine taoki version to download." >&2
  exit 1
fi

echo "Downloading taoki ${VERSION} (${PLATFORM}-${ARCH})..." >&2
TMPDIR_DL="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_DL"' EXIT

URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARTIFACT}"
if ! curl -fsSL -o "$TMPDIR_DL/$ARTIFACT" "$URL" 2>/dev/null; then
  echo "Error: failed to download taoki binary from ${URL}" >&2
  echo "Install manually: curl -fsSL https://raw.githubusercontent.com/${REPO}/master/scripts/install.sh | bash" >&2
  exit 1
fi

tar xzf "$TMPDIR_DL/$ARTIFACT" -C "$TMPDIR_DL"
mkdir -p "$DIR/target/release"
cp "$TMPDIR_DL/taoki/target/release/taoki" "$BIN"
chmod +x "$BIN"
echo "Downloaded taoki ${VERSION} successfully." >&2

exec "$BIN" "$@"
