#!/usr/bin/env bash
# PreToolUse hook for Read — nudges toward mcp__taoki__xray for large source files.
# Only fires when xray would provide clear benefit: source files >= 300 lines
# without offset/limit (which indicates an already-targeted read).
# Any failure → silent allow (never disrupt the user).

ALLOW='{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}'

INPUT=$(cat)

# If offset or limit are present, this is a targeted read — stay silent
if echo "$INPUT" | grep -q '"offset"' || echo "$INPUT" | grep -q '"limit"'; then
  echo "$ALLOW"
  exit 0
fi

# Extract file_path
FILE_PATH=$(echo "$INPUT" | grep -o '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"file_path"[[:space:]]*:[[:space:]]*"//;s/"$//')

if [ -z "$FILE_PATH" ]; then
  echo "$ALLOW"
  exit 0
fi

# Check extension — only source files
EXT="${FILE_PATH##*.}"
case "$EXT" in
  rs|py|pyi|ts|tsx|js|jsx|mjs|cjs|go|java) ;;
  *)
    echo "$ALLOW"
    exit 0
    ;;
esac

# Check file size — only nudge for 300+ lines
LINE_COUNT=$( ( wc -l < "$FILE_PATH" ) 2>/dev/null )
if [ $? -ne 0 ] || [ -z "$LINE_COUNT" ]; then
  echo "$ALLOW"
  exit 0
fi

# Trim whitespace from wc output
LINE_COUNT=$(echo "$LINE_COUNT" | tr -d ' ')

if [ "$LINE_COUNT" -lt 300 ] 2>/dev/null; then
  echo "$ALLOW"
  exit 0
fi

# 300+ lines — nudge with actual line count
cat <<EOF
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"This file has ${LINE_COUNT} lines. xray shows the structural skeleton with line numbers in ~10% of the tokens — consider xray first, then Read the sections you need."}}
EOF
