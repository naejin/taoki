# Taoki

MCP (Model Context Protocol) server that provides structural code intelligence tools. Exposes three tools over stdio JSON-RPC: `code_map` (repo-level public API summary with heuristic tags and blake3-based caching), `index` (file-level structural skeleton with line numbers and test collapsing), and `dependencies` (cross-file import/export graph). Used as a Claude Code plugin.

## Build & Test

```bash
export PATH="$HOME/.cargo/bin:$PATH"  # Rust toolchain not in default PATH
cargo build
cargo test                             # 39 unit tests, all inline (#[cfg(test)])
cargo clippy                           # must pass with no warnings
```

There are no integration tests or test fixtures — tests use `tempfile` crate to create temporary directories with inline source code.

## Architecture

Four modules under `src/`:

- **`main.rs`** — MCP stdio transport. Auto-detects framing (Content-Length headers vs bare JSONL). Reads requests, dispatches to `mcp::handle_request`, writes responses. Supports `--version` flag (prints version from `Cargo.toml` and exits before MCP loop).
- **`mcp.rs`** — JSON-RPC dispatch. Routes `initialize`, `ping`, `tools/list`, `tools/call`. Tool calls dispatch to `call_index`, `call_code_map`, and `call_dependencies`. Also handles filename-based test file detection (collapses entire test files in `index` output).
- **`codemap.rs`** — `build_code_map()` walks a repo (respecting .gitignore), hashes files with blake3, caches results in `.cache/taoki/code-map.json` with file-level locking (fs2). Calls `index::extract_all` for each file to get both public API and structural skeleton in a single parse pass. Computes heuristic tags per file (`[entry-point]`, `[tests]`, `[error-types]`, `[module-root]`, etc.). Supports optional `files` parameter to include full skeletons inline for specific files. Also triggers dependency graph building via `deps.rs`. Loads and merges LLM enrichment data from `.cache/taoki/enriched.json` when available.
- **`deps.rs`** — Cross-file dependency graph. Extracts imports from source files using tree-sitter, resolves them to actual files in the repo (best-effort, language-specific), and builds a cached graph. Provides `query_deps()` to show depends_on/used_by/external for any file. Cache stored at `.cache/taoki/deps.json`.
- **`index/`** — `index_file()` and `index_source()` use tree-sitter to parse source files and extract structural skeletons (imports, types, functions, impls, modules). Language-specific extractors live in `index/languages/` — one file per language. TypeScript and JavaScript share `typescript.rs`. Each extractor implements `is_test_node()` to detect and collapse test code. The `index` tool outputs sections: `imports:`, `consts:`, `exprs:` (top-level expressions for Python/TypeScript), `types:`, `traits:`, `impls:`, `fns:`, `classes:`, `mod:`, `macros:`, and `tests:`.

## Supported Languages

Rust (.rs), Python (.py, .pyi), TypeScript (.ts, .tsx), JavaScript (.js, .jsx, .mjs, .cjs), Go (.go), Java (.java).

## Key Conventions

- All tree-sitter grammars pinned to 0.23, tree-sitter core at 0.26.
- Error types use `thiserror` derive macros.
- Cache is stored at `<repo>/.cache/taoki/` (gitignored): `code-map.json` (v3, with tags and skeletons), `deps.json` (dependency graph), and `enriched.json` (LLM-generated semantic summaries).
- Files over 2MB are skipped (`MAX_FILE_SIZE` in `index/mod.rs`).
- Struct fields are truncated after 8 fields (`FIELD_TRUNCATE_THRESHOLD`).
- The `ignore` crate handles directory walking (respects .gitignore, global gitignore, and git exclude).

## Adding a New Language

1. Add `tree-sitter-<lang>` dependency to `Cargo.toml`.
2. Add variant to `Language` enum in `src/index/mod.rs`, update `from_extension()` and `ts_language()`.
3. Create `src/index/languages/<lang>.rs` implementing the `LanguageExtractor` trait.
4. Register the extractor in `Language::extractor()`.
5. Add a test in `src/index/mod.rs` (see existing `*_all_sections` tests).

## Distribution

Taoki is distributed via pre-built binaries on GitHub Releases and install scripts. No Rust toolchain required for end users.

- **Install scripts:** `scripts/install.sh` (Linux/macOS) and `scripts/install.ps1` (Windows). Both download the correct binary from GitHub Releases, verify SHA256 checksums, do an atomic swap install to `~/.claude/plugins/taoki/`, and register the plugin with Claude Code.
- **MCP entry points:** `scripts/run.sh` (Unix) and `scripts/run.cmd` (Windows). These have 3-way fallback: exec binary if present, `cargo build` if `Cargo.toml` exists (source clone), otherwise error with install hint.
- **Release pipeline:** `.github/workflows/release.yml` triggers on `v*` tags. Cross-compiles for 5 targets (linux x86_64/aarch64, macos x86_64/aarch64, windows x86_64) using `cross` for Linux ARM64. Packages binary + plugin files into tarballs/zips, generates `checksums.txt`, publishes a GitHub Release.
- **Release artifacts include:** `.claude-plugin/`, `commands/`, `skills/`, `hooks/`, `agents/`, `scripts/run.sh`, `scripts/run.cmd`, and the binary at `target/release/taoki`. Source code, docs, and install scripts are excluded.
- **To publish a release:** `git tag v0.x.0 && git push origin v0.x.0`

## Hooks

Four hooks in `hooks/hooks.json` enforce Taoki tool usage:

- **SessionStart (tools reminder):** Injects a message at session start reminding Claude about the three code intelligence tools and when to use them.
- **SessionStart (enrichment):** Runs `check-enrichment.sh` to detect stale LLM enrichment cache. If stale, directs Claude to dispatch the `taoki-enrich` agent before proceeding. Disabled via `TAOKI_NO_ENRICHMENT=1` env var.
- **PreToolUse (Read):** When Claude is about to Read a source file (`.rs`, `.py`, `.ts`, `.js`, `.go`, `.java`, etc.), injects a nudge suggesting `mcp__taoki__index` first. Does not block — always allows the Read.
- **PreToolUse (Glob):** When Claude uses Glob, injects a tip about `mcp__taoki__code_map` as an alternative for structural exploration. Does not block.

All hooks use command type (shell scripts) for zero-latency, deterministic behavior. Hook scripts are in `hooks/`.

## Warning

There is one known compiler warning: `framing` initial assignment in `main.rs:101` is flagged as unused because it's overwritten on first message. This is intentional — it provides a default before the first read.
