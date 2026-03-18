#!/usr/bin/env bash
# PreToolUse hook for Agent — reminds to include Taoki MCP tools when dispatching subagents.
# Only nudges for subagents that will work with code (general-purpose, Explore, code-reviewer).
# Always allows the operation.

INPUT=$(cat)
SUBAGENT_TYPE=$(echo "$INPUT" | grep -o '"subagent_type"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"subagent_type"[[:space:]]*:[[:space:]]*"//;s/"$//')

# Only nudge for agent types that work with code
case "$SUBAGENT_TYPE" in
  ""|general-purpose|Explore|Plan|feature-dev:*|superpowers:code-reviewer|code-simplifier:*)
    cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"This subagent has access to Taoki MCP tools for code intelligence. If it will explore or modify code, include in its prompt: 'You have MCP tools for code intelligence: mcp__taoki__radar (repo overview with tags), mcp__taoki__xray (single file skeleton), mcp__taoki__ripple (import/export graph with depth). Call radar first when exploring a codebase, then xray on files of interest.'"}}
EOF
    ;;
  *)
    echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}'
    ;;
esac
