#!/usr/bin/env bash
# PreToolUse hook for Glob — nudges toward mcp__taoki__code_map for codebase exploration.
# Always allows the operation.

cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"Tip: mcp__taoki__code_map gives you a one-line-per-file summary with heuristic tags like [entry-point], [tests], [data-models] — often faster than globbing for project structure."}}
EOF
