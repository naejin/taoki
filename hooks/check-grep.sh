#!/usr/bin/env bash
# PreToolUse hook for Grep — gentle tip about code_map for structural exploration.
# Always allows the operation.

cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"For structural questions (what functions does this file export? what's the class hierarchy?), mcp__taoki__xray or radar are more precise than text search. For literal string lookups, Grep is the right tool."}}
EOF
