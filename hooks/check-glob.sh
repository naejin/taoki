#!/usr/bin/env bash
# PreToolUse hook for Glob — nudges toward mcp__taoki__radar for broad exploration.
# Only fires when the pattern contains ** (indicating broad exploration).
# Targeted lookups (specific directory, no **) are left alone.
# Any failure → silent allow.

ALLOW='{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}'

INPUT=$(cat)

# Extract pattern from tool input
PATTERN=$(echo "$INPUT" | grep -o '"pattern"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"pattern"[[:space:]]*:[[:space:]]*"//;s/"$//')

if [ -z "$PATTERN" ]; then
  echo "$ALLOW"
  exit 0
fi

# Only nudge for broad patterns containing **
case "$PATTERN" in
  *\*\**)
    cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"If you're exploring project structure (not searching for a specific file), mcp__taoki__radar gives a tagged overview with public APIs — one call instead of glob + multiple reads."}}
EOF
    ;;
  *)
    echo "$ALLOW"
    ;;
esac
