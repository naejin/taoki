#!/usr/bin/env bash
# PreToolUse hook for Read — nudges toward mcp__taoki__index for source files.
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
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"Consider calling mcp__taoki__index on this file first to get its structure with line numbers, then Read only the specific sections you need. If you need multiple files, use mcp__taoki__code_map with files: [\"path1\", \"path2\"] to get all skeletons in one call. This typically saves 70-90% of tokens."}}
EOF
    ;;
  *)
    echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}'
    ;;
esac
