---
name: taoki-index
description: "Use BEFORE reading a source file with the Read tool. Use when you need to understand a file's structure, find specific functions or types within a file, or decide which lines to read. Triggers on: understanding file architecture, finding function signatures, checking what a file exports."
allowed-tools: mcp__taoki__index
---

You have access to the `index` tool which returns the structural skeleton of any source file.

## When to use this

BEFORE using `Read` on a source file, call `index` first. It returns imports, type definitions, function signatures, impl blocks, and module declarations — all with line numbers — using 70-90% fewer tokens than reading the full file.

Use the line numbers from the index to `Read` only the specific sections you need.

## How to use it

1. Call `mcp__taoki__index` with the absolute path to the source file.
2. Review the skeleton to understand the file's architecture.
3. Use `Read` with `offset` and `limit` to read only the relevant sections.

## Supported languages

Rust, Python, TypeScript, JavaScript, Go, Java.

## When NOT to use this

- For non-code files (config, markdown, JSON, etc.) — use `Read` directly.
- When you need to read the entire file anyway (small files, full rewrites).
