#!/usr/bin/env bash
# PreToolUse hook for Grep — gentle tip about code_map for structural exploration.
# Always allows the operation.

cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"If you're exploring the codebase structure, mcp__taoki__code_map gives you a tagged overview of all files with public APIs. For understanding a specific file's architecture, mcp__taoki__index returns a structural skeleton with line numbers."}}
EOF
