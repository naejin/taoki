#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$DIR/target/release/taoki"

# Build if needed
if [ ! -f "$BIN" ]; then
  echo "Building taoki in release mode..." >&2
  export PATH="$HOME/.cargo/bin:$PATH"
  cargo build --release --manifest-path "$DIR/Cargo.toml" >&2
fi

TMPDIR_BENCH=""
cleanup() { [ -n "$TMPDIR_BENCH" ] && rm -rf "$TMPDIR_BENCH"; }
trap cleanup EXIT

# Timing helper (macOS date doesn't support %N)
if date +%s%N >/dev/null 2>&1; then
  now_ms() { echo $(( $(date +%s%N) / 1000000 )); }
else
  now_ms() { python3 -c 'import time; print(int(time.time()*1000))'; }
fi

TMPDIR_BENCH="$(mktemp -d)"

# Repos to benchmark
REPOS=(
  "pallets/flask"
  "expressjs/express"
  "BurntSushi/ripgrep"
)
LABELS=("flask" "express" "ripgrep")
LANGS=("Python" "JS" "Rust")

echo "## Taoki Benchmark Results"
echo ""
echo "| Repo | Language | Files | Source KB | Index KB | Byte Reduction | code_map Cold (ms) | code_map Cached (ms) |"
echo "|------|----------|-------|-----------|----------|----------------|---------------------|----------------------|"

for i in "${!REPOS[@]}"; do
  REPO="${REPOS[$i]}"
  LABEL="${LABELS[$i]}"
  LANG="${LANGS[$i]}"
  REPO_DIR="$TMPDIR_BENCH/$LABEL"

  # Clone
  echo "Cloning $REPO..." >&2
  git clone --depth 1 "https://github.com/$REPO.git" "$REPO_DIR" 2>/dev/null

  # Count source files and sizes
  EXTENSIONS=""
  case "$LANG" in
    Python) EXTENSIONS="py" ;;
    JS) EXTENSIONS="js|mjs|cjs" ;;
    Rust) EXTENSIONS="rs" ;;
  esac

  FILE_COUNT=0
  SOURCE_BYTES=0
  INDEX_BYTES=0

  while IFS= read -r file; do
    size=$(wc -c < "$file")
    SOURCE_BYTES=$((SOURCE_BYTES + size))
    FILE_COUNT=$((FILE_COUNT + 1))

    # Run index on each file via MCP
    INDEX_REQ=$(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{"path":"%s"}}}' "$file")
    RESULT=$(echo "$INDEX_REQ" | "$BIN" 2>/dev/null || true)
    CONTENT=$(echo "$RESULT" | grep -o '"text":"[^"]*"' | tail -1 | sed 's/"text":"//;s/"$//' || echo "")
    if [ -n "$CONTENT" ]; then
      INDEX_BYTES=$((INDEX_BYTES + ${#CONTENT}))
    fi
  done < <(find "$REPO_DIR" -type f | grep -E "\\.($EXTENSIONS)$" | head -100)

  # code_map cold
  MAP_REQ=$(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"code_map","arguments":{"path":"%s"}}}' "$REPO_DIR")

  # Remove cache for cold run
  rm -rf "$REPO_DIR/.cache/taoki"

  COLD_START=$(now_ms)
  echo "$MAP_REQ" | "$BIN" >/dev/null 2>&1 || true
  COLD_END=$(now_ms)
  COLD_MS=$(( COLD_END - COLD_START ))

  # Cached run
  CACHED_START=$(now_ms)
  echo "$MAP_REQ" | "$BIN" >/dev/null 2>&1 || true
  CACHED_END=$(now_ms)
  CACHED_MS=$(( CACHED_END - CACHED_START ))

  # Calculate reduction
  SOURCE_KB=$((SOURCE_BYTES / 1024))
  INDEX_KB=$((INDEX_BYTES / 1024))
  if [ "$SOURCE_BYTES" -gt 0 ]; then
    REDUCTION=$(( (SOURCE_BYTES - INDEX_BYTES) * 100 / SOURCE_BYTES ))
  else
    REDUCTION=0
  fi

  echo "| $LABEL | $LANG | $FILE_COUNT | $SOURCE_KB | $INDEX_KB | ${REDUCTION}% | $COLD_MS | $CACHED_MS |"
done
