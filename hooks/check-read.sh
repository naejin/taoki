#!/usr/bin/env bash
# PreToolUse hook for Read — nudges toward mcp__taoki__xray for source files.
# Reads tool input JSON from stdin, checks file extension, outputs hook response.
# Always allows the operation — never blocks.

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | grep -o '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"file_path"[[:space:]]*:[[:space:]]*"//;s/"$//')

if [ -z "$FILE_PATH" ]; then
  echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}'
  exit 0
fi

EXT="${FILE_PATH##*.}"

case "$EXT" in
  rs|py|pyi|ts|tsx|js|jsx|mjs|cjs|go|java)
    cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"Consider calling mcp__taoki__xray on this file first to get its structure with line numbers, then Read only the sections you need. If you're about to modify this file, mcp__taoki__ripple shows what depends on it."}}
EOF
    ;;
  *)
    echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}'
    ;;
esac
