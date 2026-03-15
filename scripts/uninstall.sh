#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="$HOME/.claude/plugins/taoki"

# Colors
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

if [ ! -d "$INSTALL_DIR" ]; then
  error "Taoki is not installed at $INSTALL_DIR"
  exit 1
fi

# Unregister plugin
if command -v claude >/dev/null 2>&1; then
  info "Unregistering plugin from Claude Code..."
  claude plugin remove taoki 2>/dev/null || true
fi

# Remove install directory
info "Removing $INSTALL_DIR..."
rm -rf "$INSTALL_DIR"

echo ""
info "${GREEN}Taoki uninstalled successfully.${RESET}"
