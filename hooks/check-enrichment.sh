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
    [ ! -d "$ROOT/.cache/taoki" ] && exit 0
fi

CACHE_DIR="$ROOT/.cache/taoki"
CODE_MAP="$CACHE_DIR/code-map.json"
ENRICHED="$CACHE_DIR/enriched.json"

# No code map yet — nothing to enrich
[ ! -f "$CODE_MAP" ] && exit 0

# No enrichment cache — stale
if [ ! -f "$ENRICHED" ]; then
    echo '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"Code intelligence enrichment is outdated. You MUST dispatch the taoki-enrich agent and wait for it to complete before taking any action on the user'"'"'s request. Do this by using the Agent tool with the taoki-enrich agent."}}'
    exit 0
fi

# Compare hash counts (heuristic). The pattern "hash": with literal quotes only
# matches JSON keys — not string values, where quotes are escaped as \"hash\".
# Uses flexible regex to handle both pretty-printed and compact JSON.
MAP_COUNT=$(grep -Ec '"hash":[[:space:]]*"' "$CODE_MAP" 2>/dev/null || echo 0)
ENRICHED_COUNT=$(grep -Ec '"hash":[[:space:]]*"' "$ENRICHED" 2>/dev/null || echo 0)

if [ "$ENRICHED_COUNT" -lt "$MAP_COUNT" ]; then
    echo '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"Code intelligence enrichment is outdated. You MUST dispatch the taoki-enrich agent and wait for it to complete before taking any action on the user'"'"'s request. Do this by using the Agent tool with the taoki-enrich agent."}}'
fi

exit 0
