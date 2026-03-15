# Hooks & Adoption Design

## Problem

Taoki provides three code intelligence tools (code_map, index, dependencies) but Claude and subagents often skip them and go straight to Read/Glob/Grep. The plugin currently relies on a workflow skill to guide tool usage, but skills only trigger when Claude decides they're relevant. There's no mechanism to enforce or strongly nudge tool usage at the right moments.

## Goal

Make Claude and all agents/subagents consistently use Taoki tools before reading source files, using Claude Code's hooks system for automatic enforcement without blocking legitimate workflows.

## Components

### 1. hooks/hooks.json

Three hooks in a single `hooks/hooks.json` file:

**Hook 1: SessionStart — Tool awareness**

```json
{
  "matcher": "*",
  "hooks": [
    {
      "type": "prompt",
      "prompt": "You have structural code intelligence tools available via the taoki plugin. Before reading source files, use: mcp__taoki__code_map (repo overview with tags), mcp__taoki__index (file skeleton with line numbers — 70-90% fewer tokens than reading), mcp__taoki__dependencies (import/export graph for impact analysis). Always call code_map first when exploring a codebase, and index before reading any source file."
    }
  ]
}
```

Fires on session start, resume, clear, and compact. Ensures Claude knows about Taoki tools from the first message.

**Hook 2: PreToolUse on Read — Soft gate for source files**

```json
{
  "matcher": "Read",
  "hooks": [
    {
      "type": "prompt",
      "prompt": "The agent is about to Read a file. Check the file path in $TOOL_INPUT. If the file extension is one of: .rs, .py, .pyi, .ts, .tsx, .js, .jsx, .mjs, .cjs, .go, .java — then return a systemMessage suggesting: 'Consider calling mcp__taoki__index on this file first to get its structure with line numbers, then Read only the specific sections you need. This typically saves 70-90% of tokens.' Set permissionDecision to 'allow' regardless. If the file is not a source file (e.g., .md, .json, .toml, .yaml, .txt, .lock), return nothing — let it pass silently."
    }
  ]
}
```

Does not block — always returns `allow`. Only nudges for supported source file extensions. Non-code files pass through silently.

**Hook 3: PreToolUse on Glob/Grep — Nudge toward code_map**

```json
{
  "matcher": "Glob|Grep",
  "hooks": [
    {
      "type": "prompt",
      "prompt": "The agent is about to search the codebase with Glob or Grep. If this looks like an exploratory search (trying to understand project structure, find relevant files, or locate components), return a systemMessage: 'mcp__taoki__code_map gives you a one-line-per-file summary with heuristic tags like [entry-point], [tests], [data-models] — often faster and more structured than searching. Call it with the repo root path.' Set permissionDecision to 'allow'. If the search is for a specific string or pattern (e.g., searching for an error message, a variable name, or a known file path), return nothing — Grep/Glob is the right tool for that."
    }
  ]
}
```

Only nudges for exploratory searches, not targeted lookups. Always allows the operation.

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
