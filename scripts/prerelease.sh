#!/usr/bin/env bash
set -euo pipefail

# Pre-release validation for taoki.
# Run before tagging a new release to catch issues early.
#
# Usage: ./scripts/prerelease.sh [--skip-build]

DIR="$(cd "$(dirname "$0")/.." && pwd)"
ERRORS=0
WARNINGS=0

# --- Colors ---
if [ -t 1 ]; then
  BOLD='\033[1m' DIM='\033[2m'
  RED='\033[0;31m' GREEN='\033[0;32m' YELLOW='\033[0;33m' CYAN='\033[0;36m'
  RESET='\033[0m'
else
  BOLD='' DIM='' RED='' GREEN='' YELLOW='' CYAN='' RESET=''
fi

pass()  { printf "  ${GREEN}✓${RESET} %s\n" "$1"; }
fail()  { printf "  ${RED}✗${RESET} %s\n" "$1"; ERRORS=$((ERRORS + 1)); }
warn()  { printf "  ${YELLOW}!${RESET} %s\n" "$1"; WARNINGS=$((WARNINGS + 1)); }
header(){ printf "\n${BOLD}${CYAN}  %s${RESET}\n" "$1"; }

SKIP_BUILD=false
for arg in "$@"; do
  case "$arg" in
    --skip-build) SKIP_BUILD=true ;;
  esac
done

# ── 1. Version consistency ──────────────────────────────────────────
header "Version consistency"

CARGO_VERSION=$(grep '^version' "$DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
PLUGIN_VERSION=$(python3 -c "import json; print(json.load(open('$DIR/.claude-plugin/plugin.json'))['version'])")

if [ "$CARGO_VERSION" = "$PLUGIN_VERSION" ]; then
  pass "Cargo.toml and plugin.json both at v${CARGO_VERSION}"
else
  fail "Version mismatch: Cargo.toml=${CARGO_VERSION}, plugin.json=${PLUGIN_VERSION}"
fi

# Check against git tag if we're on one
GIT_TAG=$(git -C "$DIR" describe --tags --exact-match 2>/dev/null || true)
if [ -n "$GIT_TAG" ]; then
  TAG_VERSION="${GIT_TAG#v}"
  if [ "$CARGO_VERSION" = "$TAG_VERSION" ]; then
    pass "Git tag ${GIT_TAG} matches source version"
  else
    # Normal pre-release: HEAD is tagged with the old version, new version not yet tagged.
    # Only fail if there are no uncommitted changes (i.e. we're on a final tagged commit
    # that should match). Otherwise it's just a reminder.
    if git -C "$DIR" diff --quiet HEAD 2>/dev/null && git -C "$DIR" diff --cached --quiet 2>/dev/null; then
      fail "Git tag ${GIT_TAG} != Cargo.toml version ${CARGO_VERSION}"
    else
      warn "HEAD is tagged ${GIT_TAG} but source is v${CARGO_VERSION} (expected before tagging)"
    fi
  fi
fi

# ── 2. Required files ───────────────────────────────────────────────
header "Required release files"

REQUIRED_FILES=(
  ".claude-plugin/plugin.json"
  "commands/taoki-radar.md"
  "commands/taoki-ripple.md"
  "commands/taoki-xray.md"
  "skills/taoki-workflow.md"
  "hooks/hooks.json"
  "hooks/check-read.sh"
  "hooks/check-glob.sh"
  "hooks/check-agent.sh"
  "scripts/run.sh"
  "scripts/run.cmd"
  "scripts/taoki-gemini.md"
  "scripts/taoki-opencode.md"
)

for f in "${REQUIRED_FILES[@]}"; do
  if [ -f "$DIR/$f" ]; then
    pass "$f"
  else
    fail "$f missing"
  fi
done

# Hook scripts must be executable
for f in hooks/check-read.sh hooks/check-glob.sh hooks/check-agent.sh scripts/run.sh; do
  if [ -f "$DIR/$f" ] && [ ! -x "$DIR/$f" ]; then
    fail "$f is not executable"
  fi
done

# ── 3. Shell script validation ──────────────────────────────────────
header "Shell scripts"

if command -v shellcheck >/dev/null 2>&1; then
  SHELL_SCRIPTS=(
    "scripts/install.sh"
    "scripts/run.sh"
    "hooks/check-read.sh"
    "hooks/check-glob.sh"
    "hooks/check-agent.sh"
  )
  for f in "${SHELL_SCRIPTS[@]}"; do
    if [ -f "$DIR/$f" ]; then
      if shellcheck -S warning "$DIR/$f" 2>/dev/null; then
        pass "shellcheck: $f"
      else
        fail "shellcheck: $f has warnings"
      fi
    fi
  done
else
  warn "shellcheck not installed -- skipping lint (install: apt/brew install shellcheck)"
fi

# Basic syntax check (always runs)
for f in scripts/install.sh scripts/run.sh; do
  if [ -f "$DIR/$f" ] && bash -n "$DIR/$f" 2>/dev/null; then
    pass "syntax ok: $f"
  elif [ -f "$DIR/$f" ]; then
    fail "syntax error: $f"
  fi
done

# ── 4. Rust checks ─────────────────────────────────────────────────
header "Rust"

if [ "$SKIP_BUILD" = false ]; then
  if cargo clippy --manifest-path "$DIR/Cargo.toml" 2>&1 | grep -q "^error"; then
    fail "cargo clippy has errors"
  else
    pass "cargo clippy clean"
  fi

  TEST_OUTPUT=$(cargo test --manifest-path "$DIR/Cargo.toml" 2>&1)
  if echo "$TEST_OUTPUT" | grep -q "test result: ok"; then
    TEST_COUNT=$(echo "$TEST_OUTPUT" | grep "test result: ok" | head -1 | sed 's/.*\. \([0-9]*\) passed.*/\1/')
    pass "cargo test -- ${TEST_COUNT} tests passed"
  else
    fail "cargo test has failures"
  fi
else
  warn "Skipped build checks (--skip-build)"
fi

# ── 5. MCP smoke test ──────────────────────────────────────────────
header "MCP protocol"

BIN="$DIR/target/release/taoki"
if [ "$SKIP_BUILD" = false ]; then
  cargo build --release --manifest-path "$DIR/Cargo.toml" 2>/dev/null
fi

if [ ! -f "$BIN" ]; then
  warn "No release binary -- skipping MCP smoke test"
else
  # Check --version
  BIN_VERSION=$("$BIN" --version 2>&1 || true)
  if echo "$BIN_VERSION" | grep -q "$CARGO_VERSION"; then
    pass "Binary reports v${CARGO_VERSION}"
  else
    fail "Binary version '${BIN_VERSION}' doesn't contain ${CARGO_VERSION}"
  fi

  # Send initialize request
  INIT_REQ='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"prerelease-test","version":"1.0"}}}'
  INIT_RESP=$(echo "$INIT_REQ" | timeout 5 "$BIN" 2>/dev/null | head -1 || true)

  if echo "$INIT_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); assert d['result']['serverInfo']['name']=='taoki'" 2>/dev/null; then
    pass "initialize -- server identifies as taoki"
  else
    fail "initialize -- bad response: ${INIT_RESP:0:100}"
  fi

  # Send tools/list request (needs initialize first, then initialized notification, then tools/list)
  TOOLS_REQ=$(printf '%s\n%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}')

  TOOLS_RESP=$(echo "$TOOLS_REQ" | timeout 5 "$BIN" 2>/dev/null || true)

  # Extract the tools/list response (id:2)
  TOOL_NAMES=$(echo "$TOOLS_RESP" | python3 -c "
import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        d = json.loads(line)
        if d.get('id') == 2 and 'result' in d:
            names = sorted(t['name'] for t in d['result']['tools'])
            print(' '.join(names))
    except:
        pass
" 2>/dev/null || true)

  if [ "$TOOL_NAMES" = "radar ripple xray" ]; then
    pass "tools/list -- all 3 tools registered (radar, ripple, xray)"
  else
    fail "tools/list -- expected 'radar ripple xray', got '${TOOL_NAMES}'"
  fi
fi

# ── 6. Plugin.json structure ────────────────────────────────────────
header "Plugin manifest"

python3 -c "
import json, sys
d = json.load(open('$DIR/.claude-plugin/plugin.json'))
errors = []
if 'name' not in d: errors.append('missing name')
if 'version' not in d: errors.append('missing version')
if 'mcpServers' not in d: errors.append('missing mcpServers')
elif 'taoki' not in d['mcpServers']: errors.append('missing taoki server entry')
elif 'command' not in d['mcpServers']['taoki']: errors.append('missing command in taoki server')
if errors:
    print('FAIL:' + ','.join(errors))
    sys.exit(1)
print('OK')
" 2>/dev/null && pass "plugin.json schema valid" || fail "plugin.json schema invalid"

# ── Summary ─────────────────────────────────────────────────────────
printf "\n"
if [ "$ERRORS" -eq 0 ]; then
  printf "${BOLD}${GREEN}  All checks passed.${RESET}"
  if [ "$WARNINGS" -gt 0 ]; then
    printf " ${DIM}(${WARNINGS} warning(s))${RESET}"
  fi
  printf "\n"
  printf "  Ready to tag: ${BOLD}git tag v${CARGO_VERSION} && git push origin v${CARGO_VERSION}${RESET}\n"
else
  printf "${BOLD}${RED}  ${ERRORS} check(s) failed.${RESET}"
  if [ "$WARNINGS" -gt 0 ]; then
    printf " ${DIM}(${WARNINGS} warning(s))${RESET}"
  fi
  printf "\n"
  printf "  Fix the issues above before releasing.\n"
fi
printf "\n"

exit "$ERRORS"
