# Taoki

**Structural code intelligence for Claude Code, Gemini CLI, and OpenCode.** Instead of reading entire files, your coding agent gets compact summaries — public APIs, function signatures, dependency graphs — and navigates large codebases faster with 70–90% fewer tokens.

[![Release](https://img.shields.io/github/v/release/naejin/taoki?style=flat-square)](https://github.com/naejin/taoki/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](https://github.com/naejin/taoki/blob/master/LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Languages](https://img.shields.io/badge/languages-6-green?style=flat-square)](#supported-languages)
[![Tests](https://img.shields.io/badge/tests-186-brightgreen?style=flat-square)](#)
[![Claude Code](https://img.shields.io/badge/Claude_Code-plugin-blueviolet?style=flat-square)](https://docs.anthropic.com/en/docs/claude-code)
[![Gemini CLI](https://img.shields.io/badge/Gemini_CLI-supported-4285F4?style=flat-square)](https://github.com/google-gemini/gemini-cli)
[![OpenCode](https://img.shields.io/badge/OpenCode-supported-FF6F00?style=flat-square)](https://github.com/opencode-ai/opencode)

## Demo

**`radar`** — one-line-per-file summary with heuristic tags:

```
src/codemap.rs (814 lines) [error-types]
  public_types: CodeMapError
  public_functions: walk_files_public(...), build_code_map(...)

src/main.rs (25 lines) [entry-point]
  public_types: (none)
  public_functions: (none)

src/mcp/mod.rs (140 lines) [module-root]
  public_types: XrayParams, RadarParams, RippleParams, TaokiMcpServer
  public_functions: run_mcp_server(...)
```

**`xray`** — structural skeleton with line numbers and body insights:

```
imports: [4-15]
  rmcp::{schemars::JsonSchema, serde::{Deserialize, Serialize}, ...}

types:
  #[derive(Debug, Deserialize, Serialize, JsonSchema)]
  pub struct RadarParams [28-34]
    pub path: String
    pub globs: Vec<String>

impls:
  TaokiMcpServer [64-116]
    pub new() -> Self [65-69]
      → calls: Self::tool_router
    radar(
        &self,
        params: Parameters<RadarParams>,
    ) -> Result<CallToolResult, McpError> [90-99]
      → calls: CallToolResult::error, CallToolResult::success, Ok, tools::call_radar
      → match: result → Ok(text), Err(text)

fns:
  pub run_mcp_server() -> Result<(), Box<dyn std::error::Error>> [133-139]
    → calls: Ok, TaokiMcpServer::new, stdio
    → methods: serve, waiting
    → errors: 2× ?
```

**`ripple`** — cross-file import/export graph with symbols:

```
depends_on:
  src/index/mod.rs (Language, find_child, node_text)
used_by:
  src/codemap.rs
external:
  serde::Deserialize
  serde::Serialize
  std::collections::HashMap
  tree_sitter::Parser
```

## Features

- **Three tools** — `radar` (repo overview), `xray` (file skeleton), `ripple` (import graph with depth)
- **70–90% fewer tokens** — your agent reads structure, not source, then targets specific line ranges
- **Heuristic tags** — files auto-tagged as `[entry-point]`, `[tests]`, `[error-types]`, `[data-models]`, `[module-root]`, and more
- **Blast radius** — `ripple` shows transitive dependents with `depth=2` or `depth=3`, symbols shown inline
- **Docstring extraction** — first line of doc comments (`///`, `/** */`, Python docstrings) shown inline as `/// summary`
- **Body insights** — functions show `→ calls:` (free/scoped), `→ methods:` (with receiver context like `client.get`), `→ match:` (switch arms), and `→ errors:` (error sites). Calls and methods are separated: domain orchestration vs plumbing, so the signal is always visible first
- **Test collapsing** — test code detected and collapsed across all supported languages
- **Fast incremental caching** — blake3 content hashing with two-layer invalidation; file changes skip only the affected entries, new files are detected automatically
- **Tree-sitter parsing** — accurate, fast, no regex heuristics
- **Universal** — no name-based filtering or library-specific assumptions. Works the same on any codebase. Detection uses AST structure and language stdlib only
- **6 languages** — Rust, Python, TypeScript, JavaScript, Go, Java

## Install

### Pre-built binary (recommended)

The installer auto-detects installed coding agents (Claude Code, Gemini CLI, OpenCode), pre-selects them, and lets you toggle before confirming.

**Linux / macOS:**

```bash
curl -fsSL https://github.com/naejin/taoki/releases/latest/download/install.sh -o /tmp/taoki-install.sh && bash /tmp/taoki-install.sh
```

**Windows (PowerShell):**

```powershell
irm https://github.com/naejin/taoki/releases/latest/download/install.ps1 -OutFile $env:TEMP\taoki-install.ps1; & $env:TEMP\taoki-install.ps1
```

> The installer requires a TTY for the interactive agent selection prompt — that's why the script is downloaded first rather than piped directly.

**Claude Code users (non-interactive):**

```bash
claude plugin marketplace add naejin/monet-plugins && claude plugin install taoki@monet-plugins
```

### From source

Requires [Rust](https://rustup.rs/).

```bash
git clone https://github.com/naejin/taoki.git
claude plugin add ./taoki
```

The plugin compiles automatically on first use — no manual build step.

## Usage

Once installed, your coding agent automatically has access to the three tools. Use them through natural language:

| You say | Tool called |
|---------|-------------|
| "Map the codebase" | `radar` |
| "Show me the structure of src/auth.ts" | `xray` |
| "What depends on this file?" | `ripple` |
| "Map just the API routes" | `radar` with globs |

### Typical workflow

```
1. radar  → understand architecture, find relevant files by [tags]
2. ripple → check impact via used_by before modifying anything
3. xray   → get structural skeleton with line numbers
4. Read   → read only the specific line ranges you need
5. Edit   → make targeted changes with full context
```

## Supported Languages

| Language | Extensions |
|----------|------------|
| Rust | `.rs` |
| Python | `.py`, `.pyi` |
| TypeScript | `.ts`, `.tsx` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| Go | `.go` |
| Java | `.java` |

## How It Works

Taoki runs as an [MCP](https://modelcontextprotocol.io/) server over stdio. When your coding agent starts a session, it can call the three tools at any time:

- **`radar`** walks the repo (respecting `.gitignore`), hashes each file with [blake3](https://github.com/BLAKE3-team/BLAKE3), and extracts public API summaries using [tree-sitter](https://tree-sitter.github.io/). Results cached at `.cache/taoki/radar.json`. Large repos (>100 files) get directory-grouped output. Long API lists are truncated with xray cue.
- **`xray`** parses a single file and returns its structural skeleton. The first line of doc comments is extracted and shown inline (`/// summary`), giving agents intent/contract information without reading source. Function and method bodies are analyzed to show call graphs, match/switch arms, and error return sites as `→` insight lines. Test code is automatically detected and collapsed — Python (`test_*`, `Test*`), Go (`Test*`, `Benchmark*`), TypeScript/JS (`describe`, `it`, `test`), Rust (`#[test]`, `#[cfg(test)]`). Files matching test naming patterns are collapsed entirely. Results cached on disk at `.cache/taoki/xray.json`.
- **`ripple`** queries an incrementally-cached dependency graph (`.cache/taoki/deps.json`) showing internal imports with symbols, reverse dependencies with depth expansion (1-3 levels), and external packages. Resolution works across any project layout — Java suffix-based matching (no hardcoded source roots), Rust workspace-aware resolution with custom `[[bin]]/[lib]` path detection, Go cross-package resolution via module maps. For Go single-package libraries, a `co-package:` section lists sibling files. Cycle detection prevents infinite loops.

## Caching

Results are cached per-file using blake3 content hashes at `.cache/taoki/` in your repository. Caches automatically invalidate when files change, are added, or removed — no manual cache management needed. The dependency graph uses two-layer invalidation: per-file content hashes skip re-parsing, while a fingerprint over the file list and workspace config triggers re-resolution when the project structure changes. The cache is safe to delete at any time. Add `.cache/` to your `.gitignore`.

## Update

Re-run the install script to upgrade to the latest release.

## Uninstall

```bash
curl -fsSL https://github.com/naejin/taoki/releases/latest/download/uninstall.sh | bash
```

Or manually: `rm -rf ~/.claude/plugins/taoki`

## Benchmarks

Tested against 15 open-source projects (run `cargo run --bin benchmark --features benchmark` to reproduce):

<!-- BENCH:START -->
| Project | Language | Files | Parsed | Parse % | Empty Skeletons | Reduction | Radar | Ripple | Status |
|---------|----------|------:|-------:|--------:|----------------:|----------:|------:|-------:|--------|
| ripgrep | Rust | 100 | 100 | 100% | 0 | 79% | OK | 2/2 | PASS |
| tokio | Rust | 767 | 767 | 100% | 4 | 82% | OK | 2/2 | PASS |
| serde | Rust | 208 | 208 | 100% | 0 | 82% | OK | 2/2 | PASS |
| flask | Python | 83 | 83 | 100% | 0 | 85% | OK | 2/2 | PASS |
| fastapi | Python | 1122 | 1122 | 100% | 0 | 80% | OK | 2/2 | PASS |
| black | Python | 307 | 307 | 100% | 0 | 94% | OK | 2/2 | PASS |
| next.js | TypeScript | 20808 | 20808 | 100% | 20 | 81% | OK | 2/2 | PASS |
| zod | TypeScript | 393 | 393 | 100% | 0 | 84% | OK | 2/2 | PASS |
| trpc | TypeScript | 861 | 861 | 100% | 0 | 79% | OK | 2/2 | PASS |
| caddy | Go | 301 | 301 | 100% | 0 | 80% | OK | 2/2 | PASS |
| cobra | Go | 36 | 36 | 100% | 0 | 88% | OK | n/a | PASS |
| hugo | Go | 914 | 914 | 100% | 3 | 79% | OK | 2/2 | PASS |
| guava | Java | 3243 | 3243 | 100% | 0 | 72% | OK | 2/2 | PASS |
| spring-boot | Java | 8342 | 8342 | 100% | 3 | 61% | OK | 2/2 | PASS |
| deno | Rust, TS, JS | 5032 | 5032 | 100% | 21 | 73% | OK | 2/2 | FAIL |
<!-- BENCH:END -->

**Known limitation:** deno fails on empty skeletons due to `.d.ts` ambient declaration files (`declare namespace`, `declare function`). The TypeScript extractor does not yet handle `declare` blocks — these files parse successfully but produce no structural output. Tracked for a future extractor improvement.

*Results against pinned commits. Run `cargo run --bin benchmark --features benchmark -- --update-pins` to refresh pins.*

## License

MIT
