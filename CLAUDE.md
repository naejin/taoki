# Taoki

MCP (Model Context Protocol) server that provides structural code intelligence tools. Exposes three tools over stdio JSON-RPC: `code_map` (repo-level public API summary with heuristic tags and blake3-based caching), `index` (file-level structural skeleton with line numbers and test collapsing), and `dependencies` (cross-file import/export graph). Used as a Claude Code plugin.

## Build & Test

```bash
export PATH="$HOME/.cargo/bin:$PATH"  # Rust toolchain not in default PATH
cargo build
cargo test                             # 29 unit tests, all inline (#[cfg(test)])
cargo clippy                           # must pass with no warnings
```

There are no integration tests or test fixtures — tests use `tempfile` crate to create temporary directories with inline source code.

## Architecture

Four modules under `src/`:

- **`main.rs`** — MCP stdio transport. Auto-detects framing (Content-Length headers vs bare JSONL). Reads requests, dispatches to `mcp::handle_request`, writes responses.
- **`mcp.rs`** — JSON-RPC dispatch. Routes `initialize`, `ping`, `tools/list`, `tools/call`. Tool calls dispatch to `call_index`, `call_code_map`, and `call_dependencies`. Also handles filename-based test file detection (collapses entire test files in `index` output).
- **`codemap.rs`** — `build_code_map()` walks a repo (respecting .gitignore), hashes files with blake3, caches results in `.cache/taoki/code-map.json` with file-level locking (fs2). Calls `index::extract_public_api` for each file. Computes heuristic tags per file (`[entry-point]`, `[tests]`, `[error-types]`, `[module-root]`, etc.). Also triggers dependency graph building via `deps.rs`.
- **`deps.rs`** — Cross-file dependency graph. Extracts imports from source files using tree-sitter, resolves them to actual files in the repo (best-effort, language-specific), and builds a cached graph. Provides `query_deps()` to show depends_on/used_by/external for any file. Cache stored at `.cache/taoki/deps.json`.
- **`index/`** — `index_file()` and `index_source()` use tree-sitter to parse source files and extract structural skeletons (imports, types, functions, impls, modules). Language-specific extractors live in `index/languages/` — one file per language. TypeScript and JavaScript share `typescript.rs`. Each extractor implements `is_test_node()` to detect and collapse test code.

## Supported Languages

Rust (.rs), Python (.py, .pyi), TypeScript (.ts, .tsx), JavaScript (.js, .jsx, .mjs, .cjs), Go (.go), Java (.java).

## Key Conventions

- All tree-sitter grammars pinned to 0.23, tree-sitter core at 0.26.
- Error types use `thiserror` derive macros.
- Cache is stored at `<repo>/.cache/taoki/` (gitignored): `code-map.json` (v2, with tags) and `deps.json` (dependency graph).
- Files over 2MB are skipped (`MAX_FILE_SIZE` in `index/mod.rs`).
- Struct fields are truncated after 8 fields (`FIELD_TRUNCATE_THRESHOLD`).
- The `ignore` crate handles directory walking (respects .gitignore, global gitignore, and git exclude).

## Adding a New Language

1. Add `tree-sitter-<lang>` dependency to `Cargo.toml`.
2. Add variant to `Language` enum in `src/index/mod.rs`, update `from_extension()` and `ts_language()`.
3. Create `src/index/languages/<lang>.rs` implementing the `LanguageExtractor` trait.
4. Register the extractor in `Language::extractor()`.
5. Add a test in `src/index/mod.rs` (see existing `*_all_sections` tests).

## Warning

There is one known compiler warning: `framing` initial assignment in `main.rs:96` is flagged as unused because it's overwritten on first message. This is intentional — it provides a default before the first read.
