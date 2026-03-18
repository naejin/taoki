# Taoki

MCP (Model Context Protocol) server that provides structural code intelligence tools. Exposes three tools over stdio JSON-RPC: `radar` (repo-level visible API summary with heuristic tags and blake3-based caching — includes `pub(crate)` items), `xray` (file-level structural skeleton with line numbers, docstring extraction, test collapsing, and disk caching), and `ripple` (cross-file import/export graph with workspace-aware Rust resolution and depth-based blast radius). Used as a Claude Code plugin.

## Build & Test

```bash
export PATH="$HOME/.cargo/bin:$PATH"  # Rust toolchain not in default PATH
cargo build
cargo test                             # ~187 unit tests, all inline (#[cfg(test)])
cargo clippy                           # must pass with no warnings
```

### Benchmark

```bash
cargo run --bin benchmark --features benchmark           # run against 15 pinned repos
cargo run --bin benchmark --features benchmark -- --update-pins  # refresh pinned SHAs
```

Feature-gated binary (`tools/benchmark.rs`) that validates taoki against 15 real open-source projects. Not included in release artifacts — development tool only. Repos are pinned in `tools/repos.json`; results are injected into `README.md` between `BENCH:START`/`BENCH:END` markers. 17 benchmark-specific tests run only with `--features benchmark`.

There are no integration tests or test fixtures — tests use `tempfile` crate to create temporary directories with inline source code.

## Architecture

Six modules under `src/`:

- **`cache.rs`** — Shared cache infrastructure. `CACHE_VERSION` is the single source of truth for all cache format versions (radar, xray, deps) — bump it when ANY format changes. `compute_fingerprint()` hashes the sorted file list + crate map + Go module map for deps cache invalidation. `prune_xray_cache()` removes dead xray cache entries during radar calls.
- **`main.rs`** — MCP stdio transport. Auto-detects framing (Content-Length headers vs bare JSONL). Reads requests, dispatches to `mcp::handle_request`, writes responses. Supports `--version` flag (prints version from `Cargo.toml` and exits before MCP loop).
- **`mcp.rs`** — JSON-RPC dispatch. Routes `initialize`, `ping`, `tools/list`, `tools/call`. Tool calls dispatch to `call_xray`, `call_radar`, and `call_ripple`. Also handles filename-based test file detection (collapses entire test files in `xray` output). `is_test_filename()` checks both filename conventions and well-known test data directory paths (`testdata/`, `tests/data/`, `tests/fixtures/`, `__fixtures__/`, `src/test/resources/`). `find_repo_root()` walks up from a file path to locate the `.git` directory for disk cache placement. Xray disk cache (`XrayDiskCache`, `XrayDiskEntry`) persists skeletons in `.cache/taoki/xray.json` with blake3 hash invalidation and fs2 file locking.
- **`codemap.rs`** — `build_code_map()` walks a repo (respecting .gitignore), hashes files with blake3, caches results in `.cache/taoki/radar.json` with file-level locking (fs2). Calls `index::extract_public_api` for each file to get public API. Computes heuristic tags per file (`[entry-point]`, `[tests]`, `[error-types]`, `[module-root]`, etc.). For repos with >100 files (`GROUPING_THRESHOLD`), switches to directory-grouped output with name-only API. Long API lists are truncated (`FN_TRUNCATE_THRESHOLD` = 8, `TYPE_TRUNCATE_THRESHOLD` = 12) with xray cue. Also triggers dependency graph building via `deps.rs`.
- **`deps.rs`** — Cross-file dependency graph. Extracts imports from source files using tree-sitter, resolves them to actual files in the repo (best-effort, language-specific), and builds an incrementally-cached graph. `build_deps_graph()` accepts an optional old cache: per-file blake3 content hashes skip tree-sitter re-parsing for unchanged files, and a fingerprint over the file list + workspace config detects when import resolution needs redoing (storing `raw_imports` enables re-resolution without re-parsing). Workspace-aware Rust resolution: `build_crate_map()` scans Cargo.toml files to map crate names to directories and detects non-standard source directories from `[[bin]]/[lib]` path declarations (e.g., ripgrep's `path = "crates/core/main.rs"` → source dir `crates/core/`); `crate::` imports resolve within each workspace crate using source_dir_map overrides, cross-crate imports (`crate_name::path`) resolve via the crate map. Java resolution uses universal suffix-based matching against the file list (no hardcoded source root prefixes), with progressive segment stripping for static imports and wildcard import support. Go module resolution: `build_go_module_map()` scans `go.mod` files to map module paths to directories; import paths are matched against known modules to resolve to local package files. Python source root discovery: absolute imports are resolved by locating the top-level package's `__init__.py` in the file list — the directory prefix before it is the source root (e.g., finding `src/canopi/__init__.py` means source root is `src/`). No hardcoded directory names; falls back to flat layout for namespace packages. `query_deps()` accepts a `depth` parameter (1-3) for BFS expansion of `used_by`, renders imported symbols parenthetically, and detects cycles. For Go files with no internal dependencies (single-package libraries), adds a `co-package:` section listing sibling `.go` files. Provides `query_deps()` to show depends_on/used_by/external for any file. Cache stored at `.cache/taoki/deps.json`.
- **`index/`** — `index_file()` and `index_source()` use tree-sitter to parse source files and extract structural skeletons (imports, types, functions, impls, modules). `extract_all()` returns both the public API and skeleton in a single parse pass. `extract_public_api()` returns only the public API (used by `codemap.rs`). Language-specific extractors live in `index/languages/` — one file per language. TypeScript and JavaScript share `typescript.rs`. Each extractor implements `is_test_node()` to detect and collapse test code. The `xray` tool outputs sections: `imports:`, `consts:`, `exprs:` (top-level expressions for Python/TypeScript), `types:`, `traits:`, `impls:`, `fns:`, `classes:`, `mod:`, `macros:`, and `tests:`. The first line of doc comments is extracted and rendered as `/// summary` between the entry header and its children, giving agents intent/contract information without reading source. Doc extraction uses `strip_doc_prefix()` and `extract_doc_line()` on the `LanguageExtractor` trait with a default sibling-walk implementation; Python overrides entirely (docstrings are body children, not siblings); Go adds an adjacency check (its `is_doc_comment` matches all comments, not just doc-specific syntax like `///` or `/**`). Doc lines are truncated at 120 chars. Functions and methods include body insights: `→ calls:` (free/scoped calls — domain orchestration), `→ methods:` (method calls with receiver context, e.g. `client.get`), `→ match:` (match/switch arms), and `→ errors:` (error returns and `?` count).
- **`index/body.rs`** — Body analysis for function/method declarations. `analyze_body()` walks function bodies using tree-sitter (skipping nested functions, closures, and class definitions) and extracts three kinds of insights: call graph (`extract_calls` — split into free/scoped calls and method calls), match/switch arms (`extract_match_arms`), and error return sites (`extract_error_returns`). Call extraction uses AST-based priority ordering: free functions and scoped calls (domain orchestration) are separated from method calls (plumbing), determined by the call-site's AST node kind (e.g., `identifier`/`scoped_identifier` vs `field_expression` in Rust). Method calls include receiver context for compound receivers (`self.client.get()` → `client.get`). Results are attached to `SkeletonEntry` via a `BodyInsights` struct and rendered by `format_lines()` as `→ calls:` and `→ methods:` lines. Supports all 6 languages.

## Supported Languages

Rust (.rs), Python (.py, .pyi), TypeScript (.ts, .tsx), JavaScript (.js, .jsx, .mjs, .cjs), Go (.go), Java (.java).

## Key Conventions

- All tree-sitter grammars pinned to 0.23, tree-sitter core at 0.26.
- Error types use `thiserror` derive macros.
- Cache is stored at `<repo>/.cache/taoki/` (gitignored): `radar.json`, `xray.json`, `deps.json`. All share a single version (`CACHE_VERSION` in `src/cache.rs`) — bump it when any format changes. Radar uses per-file blake3 hashes (full replacement each call). Xray uses per-file blake3 hashes (upsert per call, pruned during radar). Deps uses two-layer invalidation: per-file content hashes (skip tree-sitter re-parsing) + a fingerprint over file list, workspace config, and source dir map (trigger re-resolution from cached raw imports).
- Files over 2MB are skipped (`MAX_FILE_SIZE` in `index/mod.rs`).
- Minified/bundled files are detected by `is_minified()` in `index/mod.rs` (average line length > 500 chars) and tagged `[minified]` in `radar`.
- Struct fields are truncated after 8 fields (`FIELD_TRUNCATE_THRESHOLD`).
- Radar output truncates long API lists: `FN_TRUNCATE_THRESHOLD` (8), `TYPE_TRUNCATE_THRESHOLD` (12). Directory grouping activates above `GROUPING_THRESHOLD` (100 files).
- Body insights have per-category limits: 12 calls (`MAX_CALLS`), 8 methods (`MAX_METHODS`), 10 match arms (`MAX_MATCH_ARMS`), 8 error returns (`MAX_ERRORS`). Call names truncated at 40 chars, match targets at 30, arms at 30, errors at 40.
- Ripple symbol lists are truncated after 6 symbols (`SYMBOL_TRUNCATE_THRESHOLD`), showing first 6 then `... +N more`. Applies to both `depends_on` and `used_by` sections.
- **No name-based heuristics — AST structure and language stdlib only.** This is a deliberate design principle: Taoki must work universally across all projects and languages.
  - Call prioritization uses AST node kinds (`identifier`/`scoped_identifier` vs `field_expression`) to order free/scoped calls before method calls. `is_noise_call` always returns false — no calls are filtered by name.
  - Error detection uses language syntax (`raise`, `throw`, `try_expression`) and stdlib only (`Err()`, `panic!`/`todo!`/`unimplemented!`, Go `errors.New`/`fmt.Errorf`). Namespaced macros are only accepted from `std::`/`core::`. No third-party library patterns (e.g., no `anyhow::bail!`).
  - Top-level expressions in Python/TypeScript skeletons include all dotted calls regardless of receiver name — no `NOISY_RECEIVERS` filtering.
  - Tags (`[entry-point]`, `[error-types]`, etc.) are additive metadata that never suppress information.
- The `ignore` crate handles directory walking (respects .gitignore, global gitignore, and git exclude).

## Adding a New Language

1. Add `tree-sitter-<lang>` dependency to `Cargo.toml`.
2. Add variant to `Language` enum in `src/index/mod.rs`, update `from_extension()` and `ts_language()`.
3. Create `src/index/languages/<lang>.rs` implementing the `LanguageExtractor` trait.
4. Register the extractor in `Language::extractor()`.
5. Add a test in `src/index/mod.rs` (see existing `*_all_sections` tests).

## Distribution

Taoki is distributed as a Claude Code plugin via the `monet-plugins` marketplace hosted at `naejin/monet-plugins` on GitHub. No Rust toolchain required for end users.

- **Marketplace:** `naejin/monet-plugins` hosts `marketplace.json` pointing to `naejin/taoki` as a GitHub source. Users install with `claude plugin marketplace add naejin/monet-plugins && claude plugin install taoki@monet-plugins`. One-liner install script also available: `curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash`.
- **Install scripts:** `scripts/install.sh` (Unix) and `scripts/install.ps1` (Windows). Thin wrappers that add the marketplace (if not already registered) and install the plugin. Also clean up legacy installations (old local marketplaces, MCP-only registrations, stale directories). The marketplace add is skipped if already present to avoid disrupting other plugins installed from the same marketplace.
- **MCP entry points:** `scripts/run.sh` (Unix) and `scripts/run.cmd` (Windows). These have 3-way fallback: exec binary if present, `cargo build` if Cargo.toml exists and Rust is installed (source clone), otherwise auto-download pre-built binary from GitHub Releases. The auto-download reads the version from `plugin.json` to fetch the matching release.
- **Release pipeline:** `.github/workflows/release.yml` triggers on `v*` tags. Cross-compiles for 5 targets (linux x86_64/aarch64, macos x86_64/aarch64, windows x86_64) using `cross` for Linux ARM64. Packages binary + plugin files into tarballs/zips, generates `checksums.txt`, publishes a GitHub Release.
- **Release artifacts include:** `.claude-plugin/`, `commands/`, `skills/`, `hooks/`, `agents/`, `scripts/run.sh`, `scripts/run.cmd`, and the binary at `target/release/taoki`. Source code, docs, and install scripts are excluded. `.mcp.json` is NOT included in artifacts — `plugin.json` inline `mcpServers` is the single source of truth for plugin MCP config. The Windows artifact's `plugin.json` is updated to reference `scripts/run.cmd` instead of `scripts/run.sh`.
- **Project-level `.mcp.json`:** The repo root `.mcp.json` is for development only (relative path `scripts/run.sh`). It is NOT shipped in release artifacts and is NOT used by the plugin system.
- **To publish a release:** `git tag v0.x.0 && git push origin v0.x.0`

## Hooks

Five hooks in `hooks/hooks.json` enforce Taoki tool usage:

- **SessionStart (tools reminder):** Injects a decision-tree message at session start guiding Claude to the right tool: radar for exploration, xray for file structure, ripple for impact analysis.
- **PreToolUse (Read):** When Claude is about to Read a source file (`.rs`, `.py`, `.ts`, `.js`, `.go`, `.java`, etc.), suggests `mcp__taoki__xray` first and `mcp__taoki__ripple` if modifying. Does not block — always allows the Read.
- **PreToolUse (Glob):** When Claude uses Glob, suggests `mcp__taoki__radar` as an alternative for structural exploration. Does not block.
- **PreToolUse (Grep):** When Claude uses Grep, suggests `mcp__taoki__xray` or `radar` for structural questions. Does not block.
- **PreToolUse (Agent):** When Claude dispatches a subagent for code-related work (general-purpose, Explore, Plan, feature-dev, code-reviewer), reminds to include Taoki MCP tool instructions in the subagent prompt. Does not block.

All hooks use command type (shell scripts) for zero-latency, deterministic behavior. Hook scripts are in `hooks/`.

## Warning

There is one known compiler warning: `framing` initial assignment in `main.rs:98` is flagged as unused because it's overwritten on first message. This is intentional — it provides a default before the first read.
