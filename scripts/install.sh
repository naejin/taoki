#!/usr/bin/env bash
set -euo pipefail

REPO="naejin/taoki"
INSTALL_DIR="$HOME/.claude/plugins/taoki"

# Colors (only if terminal supports it)
if [ -t 1 ]; then
  BOLD='\033[1m'
  GREEN='\033[0;32m'
  RED='\033[0;31m'
  RESET='\033[0m'
else
  BOLD='' GREEN='' RED='' RESET=''
fi

info()  { echo -e "${BOLD}taoki:${RESET} $1"; }
error() { echo -e "${RED}error:${RESET} $1" >&2; }

# Cleanup temp directory on exit
TMPDIR_INSTALL=""
cleanup() { [ -n "$TMPDIR_INSTALL" ] && rm -rf "$TMPDIR_INSTALL"; }
trap cleanup EXIT

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *) error "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)   ARCH="x86_64" ;;
  arm64|aarch64)   ARCH="aarch64" ;;
  *) error "Unsupported architecture: $ARCH"; exit 1 ;;
esac

ARTIFACT="taoki-${PLATFORM}-${ARCH}.tar.gz"

# Determine version
VERSION="${1:-}"
if [ -z "$VERSION" ]; then
  info "Fetching latest release..."
  API_URL="https://api.github.com/repos/${REPO}/releases/latest"
  CURL_ARGS=(-sSL -w "\n%{http_code}")
  [ -n "${GITHUB_TOKEN:-}" ] && CURL_ARGS+=(-H "Authorization: token $GITHUB_TOKEN")

  HTTP_RESPONSE=$(curl "${CURL_ARGS[@]}" "$API_URL" 2>/dev/null) || {
    error "Failed to connect to GitHub API."
    error "Check your internet connection or specify a version:"
    error "  curl -fsSL ... | bash -s -- v0.2.0"
    exit 1
  }

  HTTP_CODE=$(echo "$HTTP_RESPONSE" | tail -1)
  RESPONSE_BODY=$(echo "$HTTP_RESPONSE" | sed '$d')

  if [ "$HTTP_CODE" = "403" ]; then
    error "GitHub API rate limit exceeded."
    error "Set GITHUB_TOKEN env var or specify a version directly:"
    error "  curl -fsSL ... | bash -s -- v0.2.0"
    exit 1
  fi

  if [ "$HTTP_CODE" != "200" ]; then
    error "GitHub API returned HTTP $HTTP_CODE."
    error "Specify a version directly:"
    error "  curl -fsSL ... | bash -s -- v0.2.0"
    exit 1
  fi

  VERSION=$(echo "$RESPONSE_BODY" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')
  if [ -z "$VERSION" ]; then
    error "Could not determine latest version. Specify one manually:"
    error "  curl -fsSL ... | bash -s -- v0.2.0"
    exit 1
  fi
fi

info "Installing taoki ${VERSION} (${PLATFORM}-${ARCH})..."

# Download
TMPDIR_INSTALL="$(mktemp -d)"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARTIFACT}"
CHECKSUM_URL="https://github.com/${REPO}/releases/download/${VERSION}/checksums.txt"

curl -fsSL -o "$TMPDIR_INSTALL/$ARTIFACT" "$DOWNLOAD_URL" || {
  error "Failed to download ${ARTIFACT} for version ${VERSION}"
  error "Check that the version exists: https://github.com/${REPO}/releases"
  exit 1
}

# Verify checksum
curl -fsSL -o "$TMPDIR_INSTALL/checksums.txt" "$CHECKSUM_URL" || {
  error "Failed to download checksums. Aborting for safety."
  exit 1
}

EXPECTED_SUM=$(grep "$ARTIFACT" "$TMPDIR_INSTALL/checksums.txt" | awk '{print $1}')
if [ -z "$EXPECTED_SUM" ]; then
  error "Checksum for ${ARTIFACT} not found in checksums.txt"
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SUM=$(sha256sum "$TMPDIR_INSTALL/$ARTIFACT" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SUM=$(shasum -a 256 "$TMPDIR_INSTALL/$ARTIFACT" | awk '{print $1}')
else
  error "Neither sha256sum nor shasum found. Cannot verify checksum."
  exit 1
fi

if [ "$EXPECTED_SUM" != "$ACTUAL_SUM" ]; then
  error "Checksum verification failed!"
  error "  Expected: $EXPECTED_SUM"
  error "  Got:      $ACTUAL_SUM"
  exit 1
fi

info "Checksum verified."

# Extract to staging
STAGING="$TMPDIR_INSTALL/staging"
mkdir -p "$STAGING"
tar xzf "$TMPDIR_INSTALL/$ARTIFACT" -C "$STAGING"

# Atomic swap into install directory
mkdir -p "$(dirname "$INSTALL_DIR")"
if [ -d "$INSTALL_DIR" ]; then
  mv "$INSTALL_DIR" "${INSTALL_DIR}.bak"
fi
mv "$STAGING/taoki" "$INSTALL_DIR"
rm -rf "${INSTALL_DIR}.bak"

# Verify binary
if ! "$INSTALL_DIR/target/release/taoki" --version >/dev/null 2>&1; then
  error "Binary verification failed. The download may be corrupted."
  exit 1
fi

INSTALLED_VERSION=$("$INSTALL_DIR/target/release/taoki" --version 2>/dev/null || echo "unknown")
info "Installed ${INSTALLED_VERSION}"

# Register plugin with Claude Code
if command -v claude >/dev/null 2>&1; then
  info "Registering plugin with Claude Code..."
  claude plugin add "$INSTALL_DIR" 2>/dev/null && {
    info "Plugin registered successfully."
  } || {
    info "Plugin may already be registered. Run manually if needed:"
    info "  claude plugin add $INSTALL_DIR"
  }
else
  info "Claude Code not found on PATH. Register the plugin manually:"
  info "  claude plugin add $INSTALL_DIR"
fi

echo ""
info "${GREEN}Taoki installed successfully!${RESET}"
info "It will be available in your next Claude Code session."
