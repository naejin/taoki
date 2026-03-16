#!/usr/bin/env bash
# SessionStart hook — checks if enrichment cache is stale.
# Always exits 0. Outputs directive JSON if stale, nothing if fresh.

# Disable flag
[ -n "$TAOKI_NO_ENRICHMENT" ] && exit 0

# Find repo root by walking up from CWD
ROOT="$PWD"
while [ ! -d "$ROOT/.git" ] && [ "$ROOT" != "/" ]; do
    ROOT="$(dirname "$ROOT")"
done
# Fallback: look for .cache/taoki/ (supports non-git workspaces)
if [ ! -d "$ROOT/.git" ]; then
    ROOT="$PWD"
    while [ ! -d "$ROOT/.cache/taoki" ] && [ "$ROOT" != "/" ]; do
        ROOT="$(dirname "$ROOT")"
    done
fi

# Use taoki binary for definitive staleness check
TAOKI_BIN="$(dirname "$0")/../scripts/run.sh"
RESULT=$("$TAOKI_BIN" --check-enrichment "$ROOT" 2>/dev/null)

# If binary not available or command failed, do nothing (graceful degradation)
[ -z "$RESULT" ] && exit 0

if echo "$RESULT" | grep -q '"stale":true'; then
    echo '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"Code intelligence enrichment is outdated. You MUST dispatch the taoki-enrich agent and wait for it to complete before taking any action on the user'"'"'s request. Do this by using the Agent tool with the taoki-enrich agent."}}'
fi

exit 0
