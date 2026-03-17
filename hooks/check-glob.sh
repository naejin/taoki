#!/usr/bin/env bash
# PreToolUse hook for Glob — nudges toward mcp__taoki__code_map for codebase exploration.
# Always allows the operation.

cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"If you're exploring project structure (not searching for a specific file), mcp__taoki__radar gives a tagged overview with public APIs — one call instead of glob + multiple reads."}}
EOF
