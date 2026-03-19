# Taoki — Structural Code Intelligence

Taoki is an MCP server exposing three tools (`radar`, `xray`, `ripple`) that use tree-sitter AST parsing to provide structural code intelligence across 6 languages: Rust, Python, TypeScript, JavaScript, Go, and Java. All results are cached on disk for instant repeat access.

## Tools

- **radar** — Repository overview. Call with the repo root path; no other args needed. Returns one line per file with line count, structural tags like `[entry-point]`, `[tests]`, `[data-models]`, and public API names. Cached on disk.
- **xray** — File skeleton with line numbers. Shows imports, types, function signatures with body insights (calls, methods, match arms, error returns), and doc summaries. Delivers the full structural picture at 70–90% fewer tokens than reading the raw file. Cached on disk.
- **ripple** — Dependency graph for any file. Shows `depends_on` (what it imports), `used_by` (reverse dependencies), and external deps. Use `depth=2` or `depth=3` to trace transitive blast radius before a refactor.

## Workflow

1. **radar** — Get the full repo map first.
2. **xray** — Inspect files of interest; read the skeleton before the source.
3. **Read** — Use `offset`/`limit` to read only the specific sections identified by xray.
4. **ripple** — Check dependencies before modifying any file.

## Rules

- Before reading any source file (`.rs`, `.py`, `.pyi`, `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs`, `.go`, `.java`): call **xray** first. The skeleton shows structure in roughly 10% of the tokens. Then read only the specific sections you need using `offset`/`limit`.
- Before exploring project structure with broad file searches or directory listings: call **radar** first. One call returns the full repo map.
- Before modifying any file: call **ripple** to understand its dependencies. This prevents breaking downstream code.
- Skip taoki for non-code files (config, markdown, JSON, YAML) — read them directly.
- Skip taoki for string searches — use search/grep tools.

## Tags

Radar annotates each file with zero or more structural tags:

- `[entry-point]` — main/binary entry points
- `[tests]` — test files and test directories
- `[data-models]` — files defining data structures
- `[interfaces]` — trait/interface definitions
- `[error-types]` — error type definitions
- `[module-root]` — module index files (`mod.rs`, `__init__.py`, `index.ts`)
- `[config]` — configuration-related files
- `[minified]` — minified/bundled files (skip these)
