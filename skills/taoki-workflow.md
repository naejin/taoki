---
name: taoki-workflow
description: "Use when exploring a codebase, understanding code architecture, reviewing code, implementing features, fixing bugs, or before reading source files. Provides structural code intelligence: radar for repo overview with tags, xray for file skeletons with line numbers, ripple for import/export graphs with depth. Saves 70-90% tokens vs reading full files. Use this BEFORE Read, Glob, or Grep on source files."
allowed-tools: mcp__taoki__radar, mcp__taoki__xray, mcp__taoki__ripple
---

You have access to three structural code intelligence tools. Use them in this order:

## Workflow

### 1. RADAR — Sweep the repository

Call `mcp__taoki__radar` with the repository root path. This returns one line per file with:
- Line count
- **[tags]** like `[entry-point]`, `[tests]`, `[data-models]`, `[interfaces]`, `[error-types]`, `[module-root]`
- Public types and function names

Results are cached on disk (blake3 hash). Cached calls are near-instant. **Always call this first.**

Use the tags to narrow which files matter for your task:
- Fixing a bug? Look for `[error-types]` and related `[tests]` files
- Adding a feature? Look for `[interfaces]` and `[data-models]`
- Understanding entry points? Look for `[entry-point]`

### 2. RIPPLE — Check the blast radius

Call `mcp__taoki__ripple` with the file you plan to modify and the repo root. This shows:
- **depends_on:** files this file imports with symbols
- **used_by:** files that import this file (what will be affected by changes)
- **external:** third-party dependencies

Use `depth=2` or `depth=3` to see transitive impact. **Call this on every file you plan to modify.**

### 3. XRAY — See inside a file

Call `mcp__taoki__xray` on the file. This returns the structural skeleton:
- Imports, types, function signatures with body insights — all with line numbers
- 70-90% fewer tokens than reading the full file

Results are cached on disk — repeated calls on unchanged files are instant. **Never Read a source file without xraying it first.**

### 4. READ — Targeted reading

Use the `Read` tool with `offset` and `limit` parameters to read only the specific functions or sections identified by the xray. Don't read entire files when you only need a few functions.

### 5. PLAN + IMPLEMENT

With full structural understanding and dependency context, plan your changes and implement them.

## When NOT to use these tools

- For non-code files (config, markdown, JSON) — use Read directly
- When searching for a specific string — use Grep
- When you already know exactly which file and line to edit — skip to Read/Edit

## Tool reference

| Tool | Purpose | When |
|------|---------|------|
| `mcp__taoki__radar` | Repo overview with file tags | First, always |
| `mcp__taoki__ripple` | Impact analysis with depth | Before modifying any file |
| `mcp__taoki__xray` | Single file skeleton with line numbers | Before reading any source file |
