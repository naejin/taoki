#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Taoki — Multi-Agent Installer
#
# Interactive TUI installer for Claude Code, Gemini CLI, and OpenCode.
# =============================================================================

REPO="naejin/taoki"
MARKETPLACE_REPO="naejin/monet-plugins"
MARKETPLACE_NAME="monet-plugins"
PLUGIN_NAME="taoki"
BIN_DIR="$HOME/.local/bin"
BIN_PATH="$BIN_DIR/taoki"

# -----------------------------------------------------------------------------
# 1. Colors & output helpers
# -----------------------------------------------------------------------------

if [ -t 1 ]; then
  BOLD='\033[1m' GREEN='\033[0;32m' RED='\033[0;31m' YELLOW='\033[0;33m' RESET='\033[0m'
else
  BOLD='' GREEN='' RED='' YELLOW='' RESET=''
fi

info()  { echo -e "${BOLD}taoki:${RESET} $1"; }
warn()  { echo -e "${YELLOW}warning:${RESET} $1" >&2; }
error() { echo -e "${RED}error:${RESET} $1" >&2; }

# -----------------------------------------------------------------------------
# 2. TUI functions
# -----------------------------------------------------------------------------

# State
SELECTED_CLAUDE=1   # default on
SELECTED_GEMINI=0
SELECTED_OPENCODE=0
SCOPE="global"
CURSOR=0            # 0=Claude, 1=Gemini, 2=OpenCode

OLD_STTY=""
IN_TUI=0

tui_setup() {
  OLD_STTY="$(stty -g 2>/dev/null || true)"
  stty raw -echo 2>/dev/null || true
  printf '\033[?25l'  # hide cursor
  IN_TUI=1
}

tui_cleanup() {
  stty sane 2>/dev/null || true
  if [ -n "${OLD_STTY:-}" ]; then
    stty "$OLD_STTY" 2>/dev/null || true
  fi
  printf '\033[?25h'  # show cursor
  IN_TUI=0
}

# EXIT trap: always restore terminal state
cleanup_on_exit() {
  stty sane 2>/dev/null || true
  if [ -t 1 ]; then
    printf '\033[?25h' 2>/dev/null || true
  fi
}
trap cleanup_on_exit EXIT

draw_multiselect() {
  local labels=("Claude Code" "Gemini CLI" "OpenCode")
  local selected=("$SELECTED_CLAUDE" "$SELECTED_GEMINI" "$SELECTED_OPENCODE")

  # Move cursor up to redraw (11 lines for the box)
  if [ "${FIRST_DRAW:-1}" = "0" ]; then
    printf '\033[11A'
  fi
  FIRST_DRAW=0

  printf ' \033[1m┌─────────────────────────────────────────┐\033[0m\r\n'
  printf ' \033[1m│\033[0m  Taoki — Structural Code Intelligence   \033[1m│\033[0m\r\n'
  printf ' \033[1m│\033[0m                                         \033[1m│\033[0m\r\n'
  printf ' \033[1m│\033[0m  Select coding agents to install:       \033[1m│\033[0m\r\n'
  printf ' \033[1m│\033[0m                                         \033[1m│\033[0m\r\n'

  for i in 0 1 2; do
    local check=" "
    if [ "${selected[$i]}" = "1" ]; then check="x"; fi
    local pointer="  "
    if [ "$CURSOR" = "$i" ]; then pointer="> "; fi
    # Pad label to fixed width
    local label="${labels[$i]}"
    local pad_len=$(( 25 - ${#label} ))
    local padding=""
    local p
    for (( p=0; p<pad_len; p++ )); do padding="$padding "; done
    printf ' \033[1m│\033[0m  %s[%s] %s%s\033[1m│\033[0m\r\n' "$pointer" "$check" "$label" "$padding"
  done

  printf ' \033[1m│\033[0m                                         \033[1m│\033[0m\r\n'
  printf ' \033[1m│\033[0m  SPACE toggle  ENTER confirm  ESC quit  \033[1m│\033[0m\r\n'
  printf ' \033[1m└─────────────────────────────────────────┘\033[0m\r\n'
}

draw_scope() {
  local scope_cursor="$1"

  if [ "${SCOPE_FIRST_DRAW:-1}" = "0" ]; then
    printf '\033[8A'
  fi
  SCOPE_FIRST_DRAW=0

  local global_ptr="  " project_ptr="  "
  if [ "$scope_cursor" = "0" ]; then global_ptr="> "; fi
  if [ "$scope_cursor" = "1" ]; then project_ptr="> "; fi

  printf ' \033[1m┌─────────────────────────────────────────┐\033[0m\r\n'
  printf ' \033[1m│\033[0m  Install scope:                         \033[1m│\033[0m\r\n'
  printf ' \033[1m│\033[0m                                         \033[1m│\033[0m\r\n'
  printf ' \033[1m│\033[0m  %sGlobal (all projects)                \033[1m│\033[0m\r\n' "$global_ptr"
  printf ' \033[1m│\033[0m  %sProject (this directory only)        \033[1m│\033[0m\r\n' "$project_ptr"
  printf ' \033[1m│\033[0m                                         \033[1m│\033[0m\r\n'
  printf ' \033[1m│\033[0m  ENTER confirm  ESC back                \033[1m│\033[0m\r\n'
  printf ' \033[1m└─────────────────────────────────────────┘\033[0m\r\n'
}

read_key() {
  # Read a single byte using dd (works in raw mode across bash/zsh/sh)
  local byte
  byte=$(dd bs=1 count=1 2>/dev/null | xxd -p 2>/dev/null || true)

  if [ "$byte" = "1b" ]; then
    # Escape byte — try to read arrow key sequence with a short timeout.
    # Bare ESC produces no follow-up bytes; timeout ensures we don't block.
    # Use dd in a background process with kill for portability (macOS lacks `timeout`).
    local seq tmpfile
    tmpfile=$(mktemp)
    dd bs=1 count=2 2>/dev/null > "$tmpfile" &
    local dd_pid=$!
    sleep 0.1
    if kill -0 "$dd_pid" 2>/dev/null; then
      kill "$dd_pid" 2>/dev/null
      wait "$dd_pid" 2>/dev/null || true
    fi
    seq=$(xxd -p < "$tmpfile" 2>/dev/null || true)
    rm -f "$tmpfile"
    case "$seq" in
      5b41) echo "UP" ;;
      5b42) echo "DOWN" ;;
      *)    echo "ESC" ;;
    esac
  elif [ "$byte" = "20" ]; then
    echo "SPACE"
  elif [ "$byte" = "0d" ] || [ "$byte" = "0a" ]; then
    echo "ENTER"
  elif [ "$byte" = "03" ]; then
    echo "ESC"  # Ctrl-C
  else
    echo "OTHER"
  fi
}

select_agents() {
  FIRST_DRAW=1
  tui_setup

  draw_multiselect

  while true; do
    set +e
    local key
    key=$(read_key)
    set -e

    case "$key" in
      UP)
        CURSOR=$(( (CURSOR + 2) % 3 ))
        draw_multiselect
        ;;
      DOWN)
        CURSOR=$(( (CURSOR + 1) % 3 ))
        draw_multiselect
        ;;
      SPACE)
        case "$CURSOR" in
          0) SELECTED_CLAUDE=$(( 1 - SELECTED_CLAUDE )) ;;
          1) SELECTED_GEMINI=$(( 1 - SELECTED_GEMINI )) ;;
          2) SELECTED_OPENCODE=$(( 1 - SELECTED_OPENCODE )) ;;
        esac
        draw_multiselect
        ;;
      ENTER)
        # At least one must be selected
        if [ "$SELECTED_CLAUDE" = "1" ] || [ "$SELECTED_GEMINI" = "1" ] || [ "$SELECTED_OPENCODE" = "1" ]; then
          tui_cleanup
          printf '\r\n'
          return 0
        fi
        # Nothing selected — ignore (user must select at least one)
        ;;
      ESC)
        tui_cleanup
        printf '\r\n'
        echo "Cancelled."
        exit 0
        ;;
    esac
  done
}

select_scope() {
  SCOPE_FIRST_DRAW=1
  local scope_cursor=0

  tui_setup

  draw_scope "$scope_cursor"

  while true; do
    set +e
    local key
    key=$(read_key)
    set -e

    case "$key" in
      UP|DOWN)
        scope_cursor=$(( 1 - scope_cursor ))
        draw_scope "$scope_cursor"
        ;;
      ENTER)
        if [ "$scope_cursor" = "0" ]; then
          SCOPE="global"
        else
          SCOPE="project"
        fi
        tui_cleanup
        printf '\r\n'
        return 0
        ;;
      ESC)
        # Go back — re-show agent selection
        tui_cleanup
        printf '\r\n'
        select_agents
        # After agent selection, check if scope is still needed
        if [ "$SELECTED_GEMINI" = "1" ] || [ "$SELECTED_OPENCODE" = "1" ]; then
          select_scope
        fi
        return 0
        ;;
    esac
  done
}

# -----------------------------------------------------------------------------
# 3. JSON manipulation helpers
# -----------------------------------------------------------------------------

# Strip JSONC comments and trailing commas using a string-aware state machine.
# Safe for URLs containing // — only strips comments outside of string literals.
strip_jsonc() {
  python3 -c '
import sys, re

def strip_comments(text):
    result = []
    i = 0
    in_string = False
    while i < len(text):
        c = text[i]
        if in_string:
            result.append(c)
            if c == "\\" and i + 1 < len(text):
                i += 1
                result.append(text[i])
            elif c == "\"":
                in_string = False
            i += 1
        else:
            if c == "\"":
                in_string = True
                result.append(c)
                i += 1
            elif c == "/" and i + 1 < len(text):
                if text[i+1] == "/":
                    while i < len(text) and text[i] != "\n":
                        i += 1
                elif text[i+1] == "*":
                    i += 2
                    while i + 1 < len(text) and not (text[i] == "*" and text[i+1] == "/"):
                        i += 1
                    i += 2
                else:
                    result.append(c)
                    i += 1
            else:
                result.append(c)
                i += 1
    return "".join(result)

def strip_trailing_commas(text):
    return re.sub(r",\s*([\]}])", r"\1", text)

text = sys.stdin.read()
text = strip_comments(text)
text = strip_trailing_commas(text)
print(text)
' < "$1"
}

# Upsert a key into a JSON object at a given path.
# Usage: upsert_json_mcp <file> <parent_key> <child_key> <value_json>
# Example: upsert_json_mcp settings.json "mcpServers" "taoki" '{"command":"taoki"}'
upsert_json_mcp() {
  local file="$1" parent_key="$2" child_key="$3" value_json="$4"

  if ! command -v python3 >/dev/null 2>&1; then
    warn "python3 not found. Add this manually to $file:"
    echo ""
    echo "  \"$parent_key\": {"
    echo "    \"$child_key\": $value_json"
    echo "  }"
    echo ""
    return 1
  fi

  # If file doesn't exist, create it
  if [ ! -f "$file" ]; then
    mkdir -p "$(dirname "$file")"
    echo '{}' > "$file"
  fi

  # Try to parse, backing up on failure
  local clean_json
  clean_json=$(strip_jsonc "$file" 2>/dev/null) || true

  if [ -z "$clean_json" ]; then
    clean_json="{}"
  fi

  local result exit_code
  result=$(python3 -c '
import json, sys

try:
    data = json.loads(sys.argv[1])
except json.JSONDecodeError as e:
    print(f"PARSE_ERROR: {e}", file=sys.stderr)
    sys.exit(1)

parent_key = sys.argv[2]
child_key = sys.argv[3]
value = json.loads(sys.argv[4])

if parent_key not in data:
    data[parent_key] = {}
data[parent_key][child_key] = value

print(json.dumps(data, indent=2))
' "$clean_json" "$parent_key" "$child_key" "$value_json" 2>/dev/null) || exit_code=$?

  if [ "${exit_code:-0}" -ne 0 ]; then
    # Parse error — back up and give manual instructions
    local backup="${file}.bak.$(date +%s)"
    cp "$file" "$backup"
    warn "Failed to parse $file (backed up to $backup)"
    warn "Add this manually to $file:"
    echo ""
    echo "  \"$parent_key\": {"
    echo "    \"$child_key\": $value_json"
    echo "  }"
    echo ""
    return 1
  fi

  # Atomic write: temp file + mv prevents data loss on interrupt
  local tmpfile="${file}.tmp.$$"
  echo "$result" > "$tmpfile" && mv "$tmpfile" "$file"
  return 0
}

# Upsert a value into a JSON array at a given key.
# Usage: upsert_json_array <file> <key> <value_string>
# Adds value_string to the array at key if not already present.
upsert_json_array() {
  local file="$1" key="$2" value="$3"

  if ! command -v python3 >/dev/null 2>&1; then
    warn "python3 not found. Add this manually to $file:"
    echo ""
    echo "  \"$key\": [\"$value\"]"
    echo ""
    return 1
  fi

  if [ ! -f "$file" ]; then
    mkdir -p "$(dirname "$file")"
    echo '{}' > "$file"
  fi

  local clean_json
  clean_json=$(strip_jsonc "$file" 2>/dev/null) || true

  if [ -z "$clean_json" ]; then
    clean_json="{}"
  fi

  local result exit_code
  result=$(python3 -c '
import json, sys

try:
    data = json.loads(sys.argv[1])
except json.JSONDecodeError as e:
    print(f"PARSE_ERROR: {e}", file=sys.stderr)
    sys.exit(1)

key = sys.argv[2]
value = sys.argv[3]

if key not in data:
    data[key] = []
if value not in data[key]:
    data[key].append(value)

print(json.dumps(data, indent=2))
' "$clean_json" "$key" "$value" 2>/dev/null) || exit_code=$?

  if [ "${exit_code:-0}" -ne 0 ]; then
    local backup="${file}.bak.$(date +%s)"
    cp "$file" "$backup"
    warn "Failed to parse $file (backed up to $backup)"
    warn "Add this manually to $file:"
    echo ""
    echo "  \"$key\": [\"$value\"]"
    echo ""
    return 1
  fi

  # Atomic write: temp file + mv prevents data loss on interrupt
  local tmpfile="${file}.tmp.$$"
  echo "$result" > "$tmpfile" && mv "$tmpfile" "$file"
  return 0
}

# -----------------------------------------------------------------------------
# 4. Instruction file copy helper
# -----------------------------------------------------------------------------

# Copy a template file to a destination. Looks for the template adjacent to this
# script first; falls back to downloading from GitHub.
# Usage: copy_instruction_file <template_name> <dest_path>
copy_instruction_file() {
  local template_name="$1" dest_path="$2"
  local script_dir
  script_dir="$(cd "$(dirname "$0")" && pwd)"

  # Try local copy first (source checkout)
  if [ -f "$script_dir/$template_name" ]; then
    mkdir -p "$(dirname "$dest_path")"
    cp "$script_dir/$template_name" "$dest_path"
    return 0
  fi

  # Fallback: download from GitHub
  local version
  version=$(get_latest_version 2>/dev/null || true)
  local ref="${version:-master}"
  local url="https://raw.githubusercontent.com/${REPO}/${ref}/scripts/${template_name}"

  mkdir -p "$(dirname "$dest_path")"
  if curl -fsSL -o "$dest_path" "$url" 2>/dev/null; then
    return 0
  fi

  # Last resort: try master
  if [ "$ref" != "master" ]; then
    url="https://raw.githubusercontent.com/${REPO}/master/scripts/${template_name}"
    if curl -fsSL -o "$dest_path" "$url" 2>/dev/null; then
      return 0
    fi
  fi

  warn "Could not download $template_name"
  return 1
}

# -----------------------------------------------------------------------------
# 5. Agent install functions
# -----------------------------------------------------------------------------

# --- Claude Code ---

install_claude_code() {
  info "Installing for Claude Code..."
  echo ""

  # Check claude command exists
  if ! command -v claude >/dev/null 2>&1; then
    error "Claude Code not found on PATH."
    error "Install it first: https://docs.anthropic.com/en/docs/claude-code"
    error ""
    error "Then run this script again, or install manually:"
    error "  claude plugin marketplace add ${MARKETPLACE_REPO}"
    error "  claude plugin install ${PLUGIN_NAME}@${MARKETPLACE_NAME}"
    return 1
  fi

  # Clean up legacy installations from older versions
  claude mcp remove taoki -s user 2>/dev/null || true
  if [ -d "$HOME/.claude/plugins/taoki-local" ]; then
    claude plugin uninstall "${PLUGIN_NAME}@taoki-local" 2>/dev/null || true
    claude plugin marketplace remove taoki-local 2>/dev/null || true
    rm -rf "$HOME/.claude/plugins/taoki-local"
    info "Cleaned up legacy local marketplace."
  fi
  if [ -d "$HOME/.claude/plugins/taoki" ]; then
    rm -rf "$HOME/.claude/plugins/taoki"
    info "Cleaned up legacy install directory."
  fi

  # Add marketplace if not already registered
  if ! claude plugin marketplace list 2>/dev/null | grep -q "$MARKETPLACE_NAME"; then
    info "Adding marketplace..."
    if ! claude plugin marketplace add "$MARKETPLACE_REPO" 2>&1; then
      error "Failed to add marketplace. Try manually:"
      error "  claude plugin marketplace add ${MARKETPLACE_REPO}"
      return 1
    fi
  fi

  # Install or update plugin
  if claude plugin list 2>/dev/null | grep -q "${PLUGIN_NAME}@${MARKETPLACE_NAME}"; then
    info "Updating plugin..."
    claude plugin marketplace update "$MARKETPLACE_NAME" 2>&1
    claude plugin update "${PLUGIN_NAME}@${MARKETPLACE_NAME}" 2>&1
  else
    info "Installing plugin..."
    if ! claude plugin install "${PLUGIN_NAME}@${MARKETPLACE_NAME}" 2>&1; then
      error "Failed to install plugin. Try manually:"
      error "  claude plugin install ${PLUGIN_NAME}@${MARKETPLACE_NAME}"
      return 1
    fi
  fi

  echo ""
  info "${GREEN}Claude Code: installed.${RESET}"
  return 0
}

# --- Binary download (shared by Gemini CLI and OpenCode) ---

get_latest_version() {
  local tag
  tag=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
    | grep '"tag_name"' | head -1 \
    | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')
  if [ -z "$tag" ]; then
    return 1
  fi
  echo "$tag"
}

BINARY_ENSURED=0

ensure_binary() {
  # Only download once per run (both Gemini and OpenCode share the binary)
  if [ "$BINARY_ENSURED" = "1" ]; then
    return 0
  fi

  # Check if binary already exists and is up to date
  if [ -f "$BIN_PATH" ]; then
    local current_version latest_version
    current_version=$("$BIN_PATH" --version 2>/dev/null | head -1 || true)
    latest_version=$(get_latest_version 2>/dev/null || true)

    if [ -n "$latest_version" ] && [ -n "$current_version" ]; then
      # Normalize: strip leading 'v' and 'taoki ' prefix for comparison
      local current_v latest_v
      current_v=$(echo "$current_version" | sed 's/^taoki //' | sed 's/^v//')
      latest_v=$(echo "$latest_version" | sed 's/^v//')
      if [ "$current_v" = "$latest_v" ]; then
        info "Binary already up to date ($latest_version)."
        BINARY_ENSURED=1
        return 0
      fi
      info "Updating binary: $current_v -> $latest_v"
    fi
  fi

  local version
  if [ -n "${TAOKI_VERSION:-}" ]; then
    version="$TAOKI_VERSION"
    info "Using pinned version: $version"
  else
    version=$(get_latest_version 2>/dev/null || true)
    if [ -z "$version" ]; then
      error "Could not determine latest taoki version from GitHub."
      error "If rate-limited, set TAOKI_VERSION=v1.3.0 to specify a version manually."
      return 1
    fi
  fi

  local os arch platform
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)  platform="linux" ;;
    Darwin) platform="macos" ;;
    *)
      error "Unsupported OS: $os"
      return 1
      ;;
  esac

  case "$arch" in
    x86_64|amd64)  arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *)
      error "Unsupported architecture: $arch"
      return 1
      ;;
  esac

  local artifact="taoki-${platform}-${arch}.tar.gz"
  local url="https://github.com/${REPO}/releases/download/${version}/${artifact}"

  info "Downloading taoki ${version} (${platform}-${arch})..."

  local tmpdir
  tmpdir=$(mktemp -d)

  if ! curl -fsSL -o "$tmpdir/$artifact" "$url" 2>/dev/null; then
    rm -rf "$tmpdir"
    error "Failed to download taoki binary from ${url}"
    return 1
  fi

  tar xzf "$tmpdir/$artifact" -C "$tmpdir"

  mkdir -p "$BIN_DIR"
  cp "$tmpdir/taoki/target/release/taoki" "$BIN_PATH"
  chmod +x "$BIN_PATH"
  rm -rf "$tmpdir"

  info "Installed taoki binary to $BIN_PATH"

  # Check PATH
  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
      warn "$BIN_DIR is not on your PATH."
      warn "Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
      echo ""
      echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
      echo ""
      ;;
  esac

  BINARY_ENSURED=1
  return 0
}

# --- Gemini CLI ---

install_gemini_cli() {
  local scope="$1"
  info "Installing for Gemini CLI (${scope})..."
  echo ""

  # Ensure binary is available
  if ! ensure_binary; then
    error "Binary download failed. Install Gemini CLI support manually:"
    error "  Download from https://github.com/${REPO}/releases"
    error "  Place binary at $BIN_PATH"
    return 1
  fi

  local gemini_dir settings_file instruction_dest gemini_md
  if [ "$scope" = "global" ]; then
    gemini_dir="$HOME/.gemini"
  else
    gemini_dir=".gemini"
    info "Target directory: $(pwd)"
  fi
  settings_file="$gemini_dir/settings.json"
  instruction_dest="$gemini_dir/taoki.md"
  gemini_md="$gemini_dir/GEMINI.md"

  # MCP config — use absolute path so taoki works even if $BIN_DIR is not on PATH
  if upsert_json_mcp "$settings_file" "mcpServers" "taoki" "{\"command\":\"$BIN_PATH\",\"args\":[]}"; then
    info "MCP config written to $settings_file"
  fi

  # Instruction file
  if copy_instruction_file "taoki-gemini.md" "$instruction_dest"; then
    info "Instruction file written to $instruction_dest"
  else
    warn "Could not copy instruction file to $instruction_dest"
  fi

  # GEMINI.md — prepend @./taoki.md if not already present
  local import_line='@./taoki.md'
  if [ -f "$gemini_md" ]; then
    if ! grep -qF "$import_line" "$gemini_md"; then
      # Prepend using temp file in same directory (safe for cross-device)
      local tmpfile="${gemini_md}.tmp.$$"
      { echo "$import_line"; echo ""; cat "$gemini_md"; } > "$tmpfile"
      mv "$tmpfile" "$gemini_md"
      info "Added $import_line to $gemini_md"
    else
      info "$import_line already in $gemini_md"
    fi
  else
    mkdir -p "$(dirname "$gemini_md")"
    echo "$import_line" > "$gemini_md"
    info "Created $gemini_md with $import_line"
  fi

  echo ""
  info "${GREEN}Gemini CLI: installed.${RESET}"
  return 0
}

# --- OpenCode ---

install_opencode() {
  local scope="$1"
  info "Installing for OpenCode (${scope})..."
  echo ""

  # Ensure binary is available
  if ! ensure_binary; then
    error "Binary download failed. Install OpenCode support manually:"
    error "  Download from https://github.com/${REPO}/releases"
    error "  Place binary at $BIN_PATH"
    return 1
  fi

  local config_file instruction_dest
  if [ "$scope" = "global" ]; then
    config_file="$HOME/.config/opencode/opencode.json"
    instruction_dest="$HOME/.config/opencode/taoki.md"
  else
    config_file="opencode.json"
    instruction_dest="taoki.md"
    info "Target directory: $(pwd)"
  fi

  # MCP config (OpenCode format differs from Gemini) — use absolute path
  if upsert_json_mcp "$config_file" "mcp" "taoki" "{\"type\":\"local\",\"command\":[\"$BIN_PATH\"]}"; then
    info "MCP config written to $config_file"
  fi

  # Instruction file
  if copy_instruction_file "taoki-opencode.md" "$instruction_dest"; then
    info "Instruction file written to $instruction_dest"
  else
    warn "Could not copy instruction file to $instruction_dest"
  fi

  # Add instruction file path to instructions array
  if upsert_json_array "$config_file" "instructions" "$instruction_dest"; then
    info "Added $instruction_dest to instructions array in $config_file"
  fi

  echo ""
  info "${GREEN}OpenCode: installed.${RESET}"
  return 0
}

# -----------------------------------------------------------------------------
# 6. Main flow
# -----------------------------------------------------------------------------

main() {
  # Non-interactive detection
  if [ ! -t 0 ]; then
    echo "taoki: Interactive terminal required for the installer TUI."
    echo ""
    echo "Run this instead:"
    echo "  curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh -o /tmp/taoki-install.sh && bash /tmp/taoki-install.sh"
    exit 1
  fi

  # Agent selection
  select_agents

  # Scope prompt if Gemini or OpenCode selected
  if [ "$SELECTED_GEMINI" = "1" ] || [ "$SELECTED_OPENCODE" = "1" ]; then
    select_scope
  fi

  echo ""

  # Install each selected agent
  local had_error=0

  if [ "$SELECTED_CLAUDE" = "1" ]; then
    if ! install_claude_code; then
      had_error=1
    fi
    echo ""
  fi

  if [ "$SELECTED_GEMINI" = "1" ]; then
    if ! install_gemini_cli "$SCOPE"; then
      had_error=1
    fi
    echo ""
  fi

  if [ "$SELECTED_OPENCODE" = "1" ]; then
    if ! install_opencode "$SCOPE"; then
      had_error=1
    fi
    echo ""
  fi

  # Summary
  echo ""
  if [ "$had_error" = "0" ]; then
    info "${GREEN}All done!${RESET}"
  else
    info "${YELLOW}Completed with errors — see messages above.${RESET}"
  fi

  if [ "$SELECTED_CLAUDE" = "1" ]; then
    info "  Claude Code: restart Claude Code to start using taoki."
  fi
  if [ "$SELECTED_GEMINI" = "1" ]; then
    info "  Gemini CLI: restart Gemini to start using taoki."
  fi
  if [ "$SELECTED_OPENCODE" = "1" ]; then
    info "  OpenCode: restart OpenCode to start using taoki."
  fi
}

main "$@"
