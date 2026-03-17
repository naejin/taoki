# Taoki

MCP (Model Context Protocol) server that provides structural code intelligence tools. Exposes three tools over stdio JSON-RPC: `code_map` (repo-level visible API summary with heuristic tags and blake3-based caching — includes `pub(crate)` items), `index` (file-level structural skeleton with line numbers, docstring extraction, and test collapsing), and `dependencies` (cross-file import/export graph with workspace-aware Rust resolution). Used as a Claude Code plugin.

## Build & Test

```bash
export PATH="$HOME/.cargo/bin:$PATH"  # Rust toolchain not in default PATH
cargo build
cargo test                             # 135 unit tests, all inline (#[cfg(test)])
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

Five modules under `src/`:

- **`main.rs`** — MCP stdio transport. Auto-detects framing (Content-Length headers vs bare JSONL). Reads requests, dispatches to `mcp::handle_request`, writes responses. Supports `--version` flag (prints version from `Cargo.toml` and exits before MCP loop).
- **`mcp.rs`** — JSON-RPC dispatch. Routes `initialize`, `ping`, `tools/list`, `tools/call`. Tool calls dispatch to `call_index`, `call_code_map`, and `call_dependencies`. Also handles filename-based test file detection (collapses entire test files in `index` output). `is_test_filename()` checks both filename conventions and well-known test data directory paths (`testdata/`, `tests/data/`, `tests/fixtures/`, `__fixtures__/`, `src/test/resources/`).
- **`codemap.rs`** — `build_code_map()` walks a repo (respecting .gitignore), hashes files with blake3, caches results in `.cache/taoki/code-map.json` with file-level locking (fs2). Calls `index::extract_all` for each file to get both public API and structural skeleton in a single parse pass. Computes heuristic tags per file (`[entry-point]`, `[tests]`, `[error-types]`, `[module-root]`, etc.). Two modes: without `files`, builds a one-line-per-file overview with public API and heuristic tags; with `files`, switches to batch skeleton mode — returns full index-format skeletons for only the listed files (no overview), equivalent to calling `index` on each in one round trip. Also triggers dependency graph building via `deps.rs`.
- **`deps.rs`** — Cross-file dependency graph. Extracts imports from source files using tree-sitter, resolves them to actual files in the repo (best-effort, language-specific), and builds a cached graph. Workspace-aware Rust resolution: `build_crate_map()` scans Cargo.toml files to map crate names to directories; `crate::` imports resolve within each workspace crate, cross-crate imports (`crate_name::path`) resolve via the crate map. Go module resolution: `build_go_module_map()` scans `go.mod` files to map module paths to directories; import paths are matched against known modules to resolve to local package files. Python source root discovery: absolute imports are resolved by locating the top-level package's `__init__.py` in the file list — the directory prefix before it is the source root (e.g., finding `src/canopi/__init__.py` means source root is `src/`). No hardcoded directory names; falls back to flat layout for namespace packages. `query_deps()` deduplicates depends_on entries. Provides `query_deps()` to show depends_on/used_by/external for any file. Cache stored at `.cache/taoki/deps.json`.
- **`index/`** — `index_file()` and `index_source()` use tree-sitter to parse source files and extract structural skeletons (imports, types, functions, impls, modules). `extract_all()` returns both the public API and skeleton in a single parse pass (used by `codemap.rs`). Language-specific extractors live in `index/languages/` — one file per language. TypeScript and JavaScript share `typescript.rs`. Each extractor implements `is_test_node()` to detect and collapse test code. The `index` tool outputs sections: `imports:`, `consts:`, `exprs:` (top-level expressions for Python/TypeScript), `types:`, `traits:`, `impls:`, `fns:`, `classes:`, `mod:`, `macros:`, and `tests:`. The first line of doc comments is extracted and rendered as `/// summary` between the entry header and its children, giving agents intent/contract information without reading source. Doc extraction uses `strip_doc_prefix()` and `extract_doc_line()` on the `LanguageExtractor` trait with a default sibling-walk implementation; Python overrides entirely (docstrings are body children, not siblings); Go adds an adjacency check (its `is_doc_comment` matches all comments, not just doc-specific syntax like `///` or `/**`). Doc lines are truncated at 120 chars. Functions and methods include body insights: `→ calls:` (free/scoped calls — domain orchestration), `→ methods:` (method calls with receiver context, e.g. `client.get`), `→ match:` (match/switch arms), and `→ errors:` (error returns and `?` count).
- **`index/body.rs`** — Body analysis for function/method declarations. `analyze_body()` walks function bodies using tree-sitter (skipping nested functions, closures, and class definitions) and extracts three kinds of insights: call graph (`extract_calls` — split into free/scoped calls and method calls), match/switch arms (`extract_match_arms`), and error return sites (`extract_error_returns`). Call extraction uses AST-based priority ordering: free functions and scoped calls (domain orchestration) are separated from method calls (plumbing), determined by the call-site's AST node kind (e.g., `identifier`/`scoped_identifier` vs `field_expression` in Rust). Method calls include receiver context for compound receivers (`self.client.get()` → `client.get`). Results are attached to `SkeletonEntry` via a `BodyInsights` struct and rendered by `format_lines()` as `→ calls:` and `→ methods:` lines. Supports all 6 languages.

## Supported Languages

Rust (.rs), Python (.py, .pyi), TypeScript (.ts, .tsx), JavaScript (.js, .jsx, .mjs, .cjs), Go (.go), Java (.java).

## Key Conventions

- All tree-sitter grammars pinned to 0.23, tree-sitter core at 0.26.
- Error types use `thiserror` derive macros.
- Cache is stored at `<repo>/.cache/taoki/` (gitignored): `code-map.json` (v6, with tags, skeletons, docstrings, and split calls/methods), `deps.json` (v2, with workspace-aware resolution).
- Files over 2MB are skipped (`MAX_FILE_SIZE` in `index/mod.rs`).
- Minified/bundled files are detected by `is_minified()` in `index/mod.rs` (average line length > 500 chars) and tagged `[minified]` in `code_map`.
- Struct fields are truncated after 8 fields (`FIELD_TRUNCATE_THRESHOLD`).
- Body insights have per-category limits: 12 calls (`MAX_CALLS`), 8 methods (`MAX_METHODS`), 10 match arms (`MAX_MATCH_ARMS`), 8 error returns (`MAX_ERRORS`). Call names truncated at 40 chars, match targets at 30, arms at 30, errors at 40.
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

- **SessionStart (tools reminder):** Injects a message at session start reminding Claude about the three code intelligence tools and when to use them.
- **PreToolUse (Read):** When Claude is about to Read a source file (`.rs`, `.py`, `.ts`, `.js`, `.go`, `.java`, etc.), injects a nudge suggesting `mcp__taoki__index` or `code_map(files=[...])` first. Does not block — always allows the Read.
- **PreToolUse (Glob):** When Claude uses Glob, injects a tip about `mcp__taoki__code_map` as an alternative for structural exploration. Does not block.
- **PreToolUse (Grep):** When Claude uses Grep, suggests `mcp__taoki__code_map` for structural exploration and `mcp__taoki__index` for file architecture. Does not block.
- **PreToolUse (Agent):** When Claude dispatches a subagent for code-related work (general-purpose, Explore, Plan, feature-dev, code-reviewer), reminds to include Taoki MCP tool instructions in the subagent prompt. Does not block.

All hooks use command type (shell scripts) for zero-latency, deterministic behavior. Hook scripts are in `hooks/`.

## Warning

There is one known compiler warning: `framing` initial assignment in `main.rs:98` is flagged as unused because it's overwritten on first message. This is intentional — it provides a default before the first read.
