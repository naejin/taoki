---
name: taoki-workflow
description: "Use BEFORE reading source files. Call code_map for repo overview, index for file structure with line numbers, dependencies for impact analysis. Saves 70-90% tokens vs reading full files."
allowed-tools: mcp__taoki__code_map, mcp__taoki__index, mcp__taoki__dependencies
---

You have access to three structural code intelligence tools. Use them in this order:

## Workflow

### 1. MAP — Understand the repository

Call `mcp__taoki__code_map` with the repository root path. This returns one line per file with:
- Line count
- **[tags]** like `[entry-point]`, `[tests]`, `[data-models]`, `[interfaces]`, `[error-types]`, `[module-root]`
- Public types and function signatures

Results are cached on disk (blake3 hash). Cached calls are near-instant — cheaper than a single Glob. **Always call this first.** Never skip it.

Use the tags to narrow which files matter for your task:
- Fixing a bug? Look for `[error-types]` and related `[tests]` files
- Adding a feature? Look for `[interfaces]` and `[data-models]`
- Understanding entry points? Look for `[entry-point]`

### 2. FOCUS — Find related files

Call `mcp__taoki__dependencies` with the file you plan to modify and the repo root. This shows:
- **depends_on:** files this file imports (what it needs)
- **used_by:** files that import this file (what will be affected by changes)
- **external:** third-party dependencies

**Call this on every file you plan to modify.** Check `used_by` to understand impact before making changes.

### 3. INDEX — Understand file architecture

Call `mcp__taoki__index` on each file you need to understand. This returns the structural skeleton:
- Imports, types, function signatures, impl blocks — all with line numbers
- 70-90% fewer tokens than reading the full file

**Never Read a source file without indexing it first.** Use the line numbers to Read only the specific sections you need.

### 4. READ — Targeted reading

Use the `Read` tool with `offset` and `limit` parameters to read only the specific functions or sections identified by the index. Don't read entire files when you only need a few functions.

### 5. PLAN + IMPLEMENT

With full structural understanding and dependency context, plan your changes and implement them.

## When NOT to use these tools

- For non-code files (config, markdown, JSON) — use Read directly
- When searching for a specific string — use Grep
- When you already know exactly which file and line to edit — skip to Read/Edit

## Tool reference

| Tool | Purpose | When |
|------|---------|------|
| `mcp__taoki__code_map` | Repo overview with file tags | First, always |
| `mcp__taoki__dependencies` | Impact analysis | Before modifying any file |
| `mcp__taoki__index` | File structure with line numbers | Before reading any source file |
