---
name: taoki-map
description: "Use BEFORE exploring or navigating any codebase. Use BEFORE Glob, Grep, or Read when you need to understand repository structure, find relevant files, or orient yourself in a project. Triggers on: understanding architecture, finding where code lives, exploring a new repo, figuring out which files to modify."
allowed-tools: mcp__taoki__code_map
---

You have access to the `code_map` tool which provides a cached structural map of any codebase.

## When to use this

BEFORE using Glob, Grep, or Read to explore a codebase, call `code_map` first. It returns one line per file with public types and function signatures — enough to identify which files are relevant to your task.

Results are cached on disk using blake3 content hashes, so repeated calls are near-instant. Do not avoid calling it to "save time" — cached calls are cheaper than a single Glob.

## How to use it

1. Call `mcp__taoki__code_map` with the repository root path.
2. Optionally pass `globs` to narrow scope (e.g. `["src/api/**/*.ts"]`).
3. Use the results to identify which files to `index` or `Read` next.

## When NOT to use this

- When searching for a specific string or regex in file contents (use Grep instead).
- When you already know exactly which file to read or edit.
