# Hooks & Adoption Design

## Problem

Taoki provides three code intelligence tools (code_map, index, dependencies) but Claude and subagents often skip them and go straight to Read/Glob/Grep. The plugin currently relies on a workflow skill to guide tool usage, but skills only trigger when Claude decides they're relevant. There's no mechanism to enforce or strongly nudge tool usage at the right moments.

## Goal

Make Claude and all agents/subagents consistently use Taoki tools before reading source files, using Claude Code's hooks system for automatic enforcement without blocking legitimate workflows.

## Components

### 1. hooks/hooks.json

Three hooks in a single `hooks/hooks.json` file using the correct top-level structure:

```json
{
  "hooks": {
    "SessionStart": [ ... ],
    "PreToolUse": [ ..., ... ]
  }
}
```

**Hook 1: SessionStart — Tool awareness (command-based)**

SessionStart only supports `type: "command"` hooks (not prompt). The command's stdout is injected into Claude's context.

```json
{
  "matcher": "",
  "hooks": [
    {
      "type": "command",
      "command": "echo 'You have structural code intelligence tools available via the taoki plugin. Before reading source files, use: mcp__taoki__code_map (repo overview with tags), mcp__taoki__index (file skeleton with line numbers — 70-90% fewer tokens than reading), mcp__taoki__dependencies (import/export graph for impact analysis). Always call code_map first when exploring a codebase, and index before reading any source file.'"
    }
  ]
}
```

Fires on session start, resume, clear, and compact (empty matcher matches all session sources). Ensures Claude knows about Taoki tools from the first message.

**Hook 2: PreToolUse on Read — Soft gate for source files (command-based)**

```json
{
  "matcher": "Read",
  "hooks": [
    {
      "type": "command",
      "command": "bash ${CLAUDE_PLUGIN_ROOT}/hooks/check-read.sh"
    }
  ]
}
```

Uses a **command hook** (not prompt) for zero-latency, deterministic behavior. The `hooks/check-read.sh` script:

1. Reads the tool input JSON from stdin (contains `tool_input.file_path`)
2. Extracts the file extension
3. If the extension is a supported source file (`.rs`, `.py`, `.pyi`, `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs`, `.go`, `.java`): outputs JSON with `hookSpecificOutput.permissionDecision: "allow"` and `hookSpecificOutput.additionalContext` nudging toward `mcp__taoki__index`
4. If the extension is anything else: outputs JSON with `hookSpecificOutput.permissionDecision: "allow"` and no `additionalContext` (silent passthrough)

Output format:
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow",
    "additionalContext": "Consider calling mcp__taoki__index on this file first to get its structure with line numbers, then Read only the specific sections you need. This typically saves 70-90% of tokens."
  }
}
```

Always returns `allow` — never blocks. The nudge is injected via `additionalContext` only for source files.

**Hook 3: PreToolUse on Glob — Nudge toward code_map (command-based)**

```json
{
  "matcher": "Glob",
  "hooks": [
    {
      "type": "command",
      "command": "bash ${CLAUDE_PLUGIN_ROOT}/hooks/check-glob.sh"
    }
  ]
}
```

Uses a command hook. The `hooks/check-glob.sh` script:

1. Reads tool input from stdin
2. Always outputs JSON with `hookSpecificOutput.permissionDecision: "allow"` and `hookSpecificOutput.additionalContext`: "Tip: mcp__taoki__code_map gives you a one-line-per-file summary with heuristic tags — often faster than globbing for project structure."

**Note:** Grep is excluded from this hook. Grep is almost always a targeted search for a specific string — nudging toward code_map would be noise. Only Glob gets the nudge since it's more commonly used for exploratory file discovery.

**Design decisions:**
- PreToolUse hooks use **command type** (not prompt) to avoid LLM latency on every tool call. A shell script checking file extensions is near-instant.
- SessionStart uses **command type** (echo) because SessionStart only supports command hooks. The stdout is injected into Claude's context automatically.
- The Read hook fires on every source file Read, including after an index call. This is acceptable — the nudge is lightweight (a system message, not a block) and reinforces the habit without disrupting the flow.

### 2. Updated skill description

**File:** `skills/taoki-workflow.md`

Replace the `description` field in the frontmatter with:

```
"Use BEFORE reading source files. Call code_map for repo overview, index for file structure with line numbers, dependencies for impact analysis. Saves 70-90% tokens vs reading full files."
```

The rest of the skill content stays unchanged.

### 3. New /taoki-deps command

**File:** `commands/taoki-deps.md`

```markdown
---
allowed-tools: mcp__taoki__dependencies
description: Show what depends on a file and what it depends on
---

Call the `mcp__taoki__dependencies` tool on the specified file path.

After receiving the dependencies, present:
- Files this file depends on (imports)
- Files that depend on this file (used_by / reverse dependencies)
- External packages used
- Impact assessment: how many files would be affected by changes
```

### 4. Uninstall script

**File:** `scripts/uninstall.sh`

Removes the installed plugin from `~/.claude/plugins/taoki/`:
1. If `claude` is on PATH, run `claude plugin remove taoki` (or equivalent)
2. Remove `~/.claude/plugins/taoki/` directory
3. Print success message

Does not affect source clones — only removes binary installs created by `install.sh`.

### 5. CLAUDE.md update

- Add a "Hooks" subsection documenting the three hooks and their behavior
- Document the `exprs:` section added by the expression enhancement
- Update any stale line number references

### 6. Future improvements doc

**File:** `docs/superpowers/specs/future-improvements.md` (already saved)

Documents deferred items:
- Batch index mode (glob-filtered multi-file indexing)
- code_map summary mode for large repos
- Hook refinement based on usage data

## What Doesn't Change

- Rust codebase (`src/`)
- MCP tools / protocol
- Install scripts (`install.sh`, `install.ps1`)
- Release pipeline
- Existing commands and skill content (only the description frontmatter changes)
- Cache format

## Execution Order

1. hooks.json (core adoption mechanism)
2. Updated skill description (reinforces hooks)
3. /taoki-deps command (quick win)
4. Uninstall script (polish)
5. CLAUDE.md update (documentation)
