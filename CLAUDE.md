# Taoki

MCP (Model Context Protocol) server that provides structural code intelligence tools. Exposes three tools over stdio JSON-RPC: `radar` (repo-level visible API summary with heuristic tags and blake3-based caching — includes `pub(crate)` items), `xray` (file-level structural skeleton with line numbers, docstring extraction, test collapsing, and disk caching), and `ripple` (cross-file import/export graph with workspace-aware Rust resolution and depth-based blast radius). Used as a Claude Code plugin.

## Build & Test

```bash
export PATH="$HOME/.cargo/bin:$PATH"  # Rust toolchain not in default PATH
cargo build
cargo test                             # ~186 unit tests, all inline (#[cfg(test)])
cargo clippy                           # must pass with no warnings
validation/run.sh                      # 12 validation fixtures across 5 languages
```

### Benchmark

```bash
cargo run --bin benchmark --features benchmark           # run against 15 pinned repos
cargo run --bin benchmark --features benchmark -- --update-pins  # refresh pinned SHAs
```

Feature-gated binary (`tools/benchmark.rs`) that validates all 3 taoki tools against 15 real open-source projects. Not included in release artifacts — development tool only. Validates:
- **Xray**: parse rate (>99.5%), empty skeleton rate (<1%), token reduction (>50%)
- **Radar**: code map produces non-empty output for every repo
- **Ripple**: `test_files` in `repos.json` have internal depends_on or used_by (not all-external) — catches resolution bugs that unit tests miss

Repos are pinned in `tools/repos.json` with `test_files` per repo for ripple validation; results are injected into `README.md` between `BENCH:START`/`BENCH:END` markers. 22 benchmark-specific tests run only with `--features benchmark`.

Tests use `tempfile` crate to create temporary directories with inline source code. Validation fixtures in `validation/` test xray output expectations across 5 languages — run `validation/run.sh` to verify.

## Architecture

Six modules under `src/`:

- **`cache.rs`** — Shared cache infrastructure. `CACHE_VERSION` is the single source of truth for all cache format versions (radar, xray, deps) — bump it when ANY format changes. `compute_fingerprint()` hashes the sorted file list + crate map + Go module map for deps cache invalidation. `prune_xray_cache()` removes dead xray cache entries during radar calls.
- **`main.rs`** — Entry point. Handles `--version` flag, creates tokio runtime, starts MCP server via `mcp::run_mcp_server()`.
- **`mcp/`** — MCP server module using the `rmcp` framework. `mod.rs` defines parameter types with `JsonSchema` derives (schemas generated automatically), tool routing via `#[tool_router]` and `#[tool_handler]` macros, and server startup. `tools.rs` contains tool implementations (`call_xray`, `call_radar`, `call_ripple`), xray disk cache management (`XrayDiskCache`, `XrayDiskEntry` with blake3 hash invalidation and fs2 file locking), test filename detection (`is_test_filename()`), and repo root discovery (`find_repo_root()`).
- **`codemap.rs`** — `build_code_map()` walks a repo (respecting .gitignore), hashes files with blake3, caches results in `.cache/taoki/radar.json` with file-level locking (fs2). Uses `rayon` for parallel per-file processing (hash, cache check, parse, tag computation). Computes heuristic tags per file (`[entry-point]`, `[tests]`, `[error-types]`, `[module-root]`, etc.). For repos with >100 files (`GROUPING_THRESHOLD`), switches to directory-grouped output with name-only API. Long API lists are truncated (`FN_TRUNCATE_THRESHOLD` = 8, `TYPE_TRUNCATE_THRESHOLD` = 12) with xray cue. Also triggers dependency graph building via `deps.rs`.
- **`deps.rs`** — Cross-file dependency graph. Extracts imports via tree-sitter, resolves them to repo files (best-effort, language-specific), and builds an incrementally-cached graph. Two-layer cache: per-file blake3 hashes skip re-parsing unchanged files; a fingerprint over file list + workspace config triggers re-resolution from cached `raw_imports`. Each language has its own resolver (Rust workspace-aware via `build_crate_map()`, Go module-aware via `build_go_module_map()`, Python with source root discovery, Java with suffix matching, TypeScript with relative path normalization). `query_deps()` accepts `depth` 1-3 for BFS expansion of `used_by`, renders imported symbols parenthetically, and detects cycles. Cache stored at `.cache/taoki/deps.json`.
- **`index/`** — `index_file()` and `index_source()` use tree-sitter to parse source files and extract structural skeletons (imports, types, functions, impls, modules). `extract_all()` returns both the public API and skeleton in a single parse pass. `extract_public_api()` returns only the public API (used by `codemap.rs`). Language-specific extractors live in `index/languages/` — one file per language. TypeScript and JavaScript share `typescript.rs`. Each extractor implements `is_test_node()` to detect and collapse test code. The `xray` tool outputs sections: `imports:`, `consts:`, `exprs:` (top-level expressions for Python/TypeScript), `types:`, `traits:`, `impls:`, `fns:`, `classes:`, `mod:`, `macros:`, and `tests:`. The first line of doc comments is extracted and rendered as `/// summary` between the entry header and its children, giving agents intent/contract information without reading source. Doc extraction uses `strip_doc_prefix()` and `extract_doc_line()` on the `LanguageExtractor` trait with a default sibling-walk implementation; Python overrides entirely (docstrings are body children, not siblings); Go adds an adjacency check (its `is_doc_comment` matches all comments, not just doc-specific syntax like `///` or `/**`). Doc lines are truncated at 120 chars. Functions and methods include body insights: `→ calls:` (free/scoped calls — domain orchestration), `→ methods:` (method calls with receiver context, e.g. `client.get`), `→ match:` (match/switch arms), and `→ errors:` (error returns and `?` count).
- **`index/body.rs`** — Body analysis for function/method declarations. `analyze_body()` walks function bodies using tree-sitter (skipping nested functions, closures, and class definitions) and extracts three kinds of insights: call graph (`extract_calls` — split into free/scoped calls and method calls), match/switch arms (`extract_match_arms`), and error return sites (`extract_error_returns`). Call extraction uses AST-based priority ordering: free functions and scoped calls (domain orchestration) are separated from method calls (plumbing), determined by the call-site's AST node kind (e.g., `identifier`/`scoped_identifier` vs `field_expression` in Rust). Method calls include receiver context for compound receivers (`self.client.get()` → `client.get`). Results are attached to `SkeletonEntry` via a `BodyInsights` struct and rendered by `format_lines()` as `→ calls:` and `→ methods:` lines. Supports all 6 languages.

## Supported Languages

Rust (.rs), Python (.py, .pyi), TypeScript (.ts, .tsx), JavaScript (.js, .jsx, .mjs, .cjs), Go (.go), Java (.java).

## Key Conventions

- MCP transport uses `rmcp` framework with JSONL framing (not Content-Length). The `stdio()` transport handles framing automatically.
- rmcp macros: `#[tool_router]` on impl block generates tool registration; `#[tool]` on async methods registers individual tools; `#[tool_handler]` on `ServerHandler` impl handles initialization. The `tool_router` struct field appears dead to the compiler but is used by macros at runtime — `#[allow(dead_code)]` is required. `#[allow(clippy::new_without_default)]` goes on the `impl` block (not the struct) because `Default` would bypass tool registration.
- Tool parameter types derive `JsonSchema` (from `schemars`) for automatic schema generation. Doc comments on struct fields become parameter descriptions in the MCP schema.
- Adding a new MCP tool: define a params struct with `#[derive(Debug, Deserialize, Serialize, JsonSchema)]`, add a `#[tool]`-annotated async method to `TaokiMcpServer`, implement the sync helper in `tools.rs`.
- Radar uses `rayon::par_iter` for parallel per-file processing. Safety invariant: the old cache (`cache.files`) is read-only during the parallel phase — new results are collected into a `Vec` then inserted into `HashMap` sequentially after.
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
- **Static analysis boundary**: Ripple traces `use`/`import` statements only. Trait-based dispatch (e.g., `Language::extractor()` returning `&dyn LanguageExtractor`) and runtime polymorphism are invisible to the dependency graph. This is by design — no heuristic workarounds.

## Adding a New Language

1. Add `tree-sitter-<lang>` dependency to `Cargo.toml`.
2. Add variant to `Language` enum in `src/index/mod.rs`, update `from_extension()` and `ts_language()`.
3. Create `src/index/languages/<lang>.rs` implementing the `LanguageExtractor` trait.
4. Register the extractor in `Language::extractor()`.
5. Add a test in `src/index/mod.rs` (see existing `*_all_sections` tests).

## Distribution

Taoki is distributed as a Claude Code plugin via the `monet-plugins` marketplace hosted at `naejin/monet-plugins` on GitHub. No Rust toolchain required for end users.

- **Marketplace:** `naejin/monet-plugins` hosts `marketplace.json` pointing to `naejin/taoki` as a GitHub source. Claude Code users can install manually with `claude plugin marketplace add naejin/monet-plugins && claude plugin install taoki@monet-plugins`. Interactive install script also available: `curl -fsSL https://github.com/naejin/taoki/releases/latest/download/install.sh -o /tmp/taoki-install.sh && bash /tmp/taoki-install.sh` (Unix) or `irm https://github.com/naejin/taoki/releases/latest/download/install.ps1 -OutFile $env:TEMP\taoki-install.ps1; & $env:TEMP\taoki-install.ps1` (Windows). The script must be downloaded before running because the interactive TUI requires a TTY.
- **Install scripts:** `scripts/install.sh` (Unix) and `scripts/install.ps1` (Windows). Interactive TUI installers that support Claude Code, Gemini CLI, and OpenCode — detect installed agents and prompt for selection when multiple are found. Also clean up legacy installations (old local marketplaces, MCP-only registrations, stale directories).
- **MCP entry points:** `scripts/run.sh` (Unix) and `scripts/run.cmd` (Windows). These have 3-way fallback: exec binary if present, `cargo build` if Cargo.toml exists and Rust is installed (source clone), otherwise auto-download pre-built binary from GitHub Releases. The auto-download reads the version from `plugin.json` to fetch the matching release.
- **Release pipeline:** `.github/workflows/release.yml` triggers on `v*` tags. Cross-compiles for 5 targets (linux x86_64/aarch64, macos x86_64/aarch64, windows x86_64) using `cross` for Linux ARM64. Packages binary + plugin files into tarballs/zips, generates `checksums.txt`, publishes a GitHub Release.
- **Release artifacts include:** `.claude-plugin/`, `commands/`, `skills/`, `hooks/`, `agents/`, `scripts/run.sh`, `scripts/run.cmd`, `scripts/taoki-gemini.md`, `scripts/taoki-opencode.md`, and the binary at `target/release/taoki`. Install scripts (`install.sh`, `install.ps1`) are published as standalone release assets (not inside platform tarballs) so the `/releases/latest/download/` URL always serves the latest version. Source code and docs are excluded. `.mcp.json` is NOT included in artifacts — `plugin.json` inline `mcpServers` is the single source of truth for plugin MCP config. The Windows artifact's `plugin.json` is updated to reference `scripts/run.cmd` instead of `scripts/run.sh`.
- **Project-level `.mcp.json`:** The repo root `.mcp.json` is for development only (relative path `scripts/run.sh`). It is NOT shipped in release artifacts and is NOT used by the plugin system.
- **To publish a release:** `git tag v0.x.0 && git push origin v0.x.0`

## Hooks

Four hooks in `hooks/hooks.json` guide Taoki tool usage. Designed to avoid alarm fatigue — hooks only fire when there's clear, quantifiable benefit over the default tool:

- **SessionStart (workflow reminder):** Injects a decision-tree message at session start guiding Claude to the right tool, plus a workflow sequence: radar → xray → Read sections → ripple before modifying.
- **PreToolUse (Read):** Size-aware — only fires for source files >= 300 lines when no `offset`/`limit` is provided (indicating a full-file read, not a targeted section). Includes the actual line count in the message so the model can weigh the tradeoff. Silent for small files and targeted reads. Does not block.
- **PreToolUse (Glob):** Pattern-aware — only fires when the glob pattern contains `**` (broad exploration). Silent for targeted lookups like `hooks/*.sh`. Does not block.
- **PreToolUse (Agent):** When Claude dispatches a subagent for code-related work (general-purpose, Explore, Plan, feature-dev, code-reviewer), reminds to include Taoki MCP tool instructions in the subagent prompt. Does not block.

All hooks use command type (shell scripts) for zero-latency, deterministic behavior. Hook scripts are in `hooks/`. Error handling: any failure in a hook → silent allow (never disrupts the user).

## Warning

There are no known compiler warnings.
