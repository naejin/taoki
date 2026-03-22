#!/usr/bin/env bash
# Taoki validation runner — verifies xray output against fixture expectations.
#
# Usage: ./validation/run.sh [language]
#   ./validation/run.sh          # run all languages
#   ./validation/run.sh rust     # run only Rust
#
# Exit code 0 = all pass, 1 = failures found.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TAOKI="$REPO_ROOT/target/release/taoki"

if [ ! -x "$TAOKI" ]; then
    printf 'Building taoki...\n' >&2
    (cd "$REPO_ROOT" && cargo build --release) >&2
fi

PASS=0
FAIL=0
ERRORS=""

# Start MCP server as a background process
start_server() {
    # Create a temp directory for server I/O
    SERVER_DIR=$(mktemp -d)
    mkfifo "$SERVER_DIR/stdin"
    mkfifo "$SERVER_DIR/stdout"

    "$TAOKI" < "$SERVER_DIR/stdin" > "$SERVER_DIR/stdout" 2>/dev/null &
    SERVER_PID=$!

    # Open file descriptors for writing/reading
    exec 3>"$SERVER_DIR/stdin"
    exec 4<"$SERVER_DIR/stdout"

    # Initialize
    local init_msg='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'
    printf '%s\n' "$init_msg" >&3
    read -r _ <&4  # consume response

    local notif='{"jsonrpc":"2.0","method":"notifications/initialized"}'
    printf '%s\n' "$notif" >&3
}

stop_server() {
    exec 3>&- 2>/dev/null || true
    exec 4<&- 2>/dev/null || true
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    rm -rf "$SERVER_DIR" 2>/dev/null || true
}

# Call xray on a file via the running MCP server
call_xray() {
    local file="$1"
    local id=$((RANDOM + 100))
    local abs_path
    abs_path="$(cd "$(dirname "$file")" && pwd)/$(basename "$file")"

    local msg
    msg=$(printf '{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":{"name":"xray","arguments":{"path":"%s"}}}' "$id" "$abs_path")
    printf '%s\n' "$msg" >&3

    local response
    read -r response <&4
    # Extract the text content from the response
    printf '%s' "$response" | python3 -c "
import sys, json
resp = json.load(sys.stdin)
content = resp.get('result', {}).get('content', [{}])
if content:
    print(content[0].get('text', ''))
" 2>/dev/null
}

check_file() {
    local file="$1"
    local lang_dir="$2"
    local rel="${file#$SCRIPT_DIR/}"

    # Get xray output
    local output
    output=$(call_xray "$file")
    local status="PASS"

    # Check "contains=" expectations
    while IFS= read -r line; do
        local expected="${line#*contains=}"
        if ! printf '%s' "$output" | grep -qF "$expected"; then
            status="FAIL"
            ERRORS="$ERRORS\n  $rel: expected to contain '$expected'"
        fi
    done < <(grep -i '# Expected:.*contains=' "$file" 2>/dev/null | grep -v 'not_contains' || true)
    while IFS= read -r line; do
        local expected="${line#*contains=}"
        if ! printf '%s' "$output" | grep -qF "$expected"; then
            status="FAIL"
            ERRORS="$ERRORS\n  $rel: expected to contain '$expected'"
        fi
    done < <(grep -i '// Expected:.*contains=' "$file" 2>/dev/null | grep -v 'not_contains' || true)

    # Check "not_contains=" expectations
    while IFS= read -r line; do
        local unexpected="${line#*not_contains=}"
        if printf '%s' "$output" | grep -qF "$unexpected"; then
            status="FAIL"
            ERRORS="$ERRORS\n  $rel: should NOT contain '$unexpected'"
        fi
    done < <(grep -i '# Expected:.*not_contains=' "$file" 2>/dev/null || true)
    while IFS= read -r line; do
        local unexpected="${line#*not_contains=}"
        if printf '%s' "$output" | grep -qF "$unexpected"; then
            status="FAIL"
            ERRORS="$ERRORS\n  $rel: should NOT contain '$unexpected'"
        fi
    done < <(grep -i '// Expected:.*not_contains=' "$file" 2>/dev/null || true)

    # Check "sections=" expectations
    local sections_line
    sections_line=$(grep -m1 -i 'Expected:.*sections=' "$file" 2>/dev/null | sed 's/.*sections=//' || true)
    if [ -n "$sections_line" ]; then
        IFS=',' read -ra SECS <<< "$sections_line"
        for sec in "${SECS[@]}"; do
            sec=$(printf '%s' "$sec" | tr -d ' ')
            if ! printf '%s' "$output" | grep -q "^${sec}:"; then
                status="FAIL"
                ERRORS="$ERRORS\n  $rel: expected section '$sec:' not found"
            fi
        done
    fi

    if [ "$status" = "PASS" ]; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
    fi
    printf '  %s %s\n' "$status" "$rel"
}

run_language() {
    local lang="$1"
    local lang_dir="$SCRIPT_DIR/$lang"

    if [ ! -d "$lang_dir" ]; then
        return
    fi

    printf '%s:\n' "$lang"

    for subdir in clean boundary bad; do
        if [ -d "$lang_dir/$subdir" ]; then
            for file in "$lang_dir/$subdir"/*; do
                [ -f "$file" ] || continue
                check_file "$file" "$lang_dir"
            done
        fi
    done
}

# Start the MCP server
start_server

# Run languages
LANGS="${1:-rust python typescript go java}"
for lang in $LANGS; do
    run_language "$lang"
done

# Stop the server
stop_server

printf '\n--- Results: %d pass, %d fail ---\n' "$PASS" "$FAIL"

if [ "$FAIL" -gt 0 ]; then
    printf '\nFailures:%b\n' "$ERRORS"
    exit 1
fi
