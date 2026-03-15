# Taoki

**Structural code intelligence for Claude Code.** Instead of reading entire files, Claude gets compact summaries — public APIs, function signatures, dependency graphs — and navigates large codebases faster with 70–90% fewer tokens.

[![Release](https://img.shields.io/github/v/release/naejin/taoki?style=flat-square)](https://github.com/naejin/taoki/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](https://github.com/naejin/taoki/blob/master/LICENSE)

## Demo

**`code_map`** — one-line-per-file summary with heuristic tags:

```
src/codemap.rs (537 lines) [error-types]
  public_types: CodeMapError
  public_functions: walk_files_public(...), build_code_map(...)

src/main.rs (136 lines) [entry-point]
  public_types: (none)
  public_functions: (none)

src/mcp.rs (479 lines) [error-types]
  public_types: JsonRpcRequest, JsonRpcResponse, JsonRpcError, ToolContent, ToolResult
  public_functions: tool_definitions(), handle_request(...)
```

**`index`** — structural skeleton with line numbers:

```
imports: [1-3]
  taoki::mcp
  std::io::{self, BufRead, Write}

types:
  #[derive(Clone, Copy, PartialEq)]
  enum Framing [6-9]
    ContentLength
    Jsonl

fns:
  read_message(reader: &mut impl BufRead) -> ... [11-37]
  read_content_length_message(reader: &mut impl BufRead) -> ... [39-71]
  write_message(writer: &mut impl Write, ...) [73-84]
  main() [86-135]
```

**`dependencies`** — cross-file import/export graph:

```
depends_on:
  src/index/mod.rs
used_by:
  src/codemap.rs
external:
  serde::Deserialize
  serde::Serialize
  std::collections::HashMap
  tree_sitter::Parser
```

## Features

- **Three tools** — `code_map` (repo overview), `index` (file skeleton), `dependencies` (import graph)
- **70–90% fewer tokens** — Claude reads structure, not source, then targets specific line ranges
- **Heuristic tags** — files auto-tagged as `[entry-point]`, `[tests]`, `[error-types]`, `[data-models]`, `[module-root]`, and more
- **Test collapsing** — test code detected and collapsed across all supported languages
- **Fast caching** — blake3 content hashing with file-level locking; repeated calls are near-instant
- **Tree-sitter parsing** — accurate, fast, no regex heuristics
- **6 languages** — Rust, Python, TypeScript, JavaScript, Go, Java

## Install

### Pre-built binary (recommended)

**Linux / macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.ps1 | iex
```

<details>
<summary>Install a specific version</summary>

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash -s -- v0.3.1
```

```powershell
$env:TAOKI_VERSION="v0.3.1"; irm https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.ps1 | iex
```

</details>

### From source

Requires [Rust](https://rustup.rs/).

```bash
git clone https://github.com/naejin/taoki.git
claude plugin add ./taoki
```

The plugin compiles automatically on first use — no manual build step.

## Usage

Once installed, Claude automatically has access to the three tools. Use them through natural language:

| You say | Claude calls |
|---------|-------------|
| "Map the codebase" | `code_map` |
| "Show me the structure of src/auth.ts" | `index` |
| "What depends on this file?" | `dependencies` |
| "Map just the API routes" | `code_map` with globs |

### Typical workflow

```
1. code_map     → understand architecture, find relevant files by [tags]
2. dependencies → check impact via used_by before modifying anything
3. index        → get structural skeleton with line numbers
4. Read         → read only the specific line ranges you need
5. Edit         → make targeted changes with full context
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

Taoki runs as an [MCP](https://modelcontextprotocol.io/) server over stdio. When Claude starts a session, it can call the three tools at any time:

- **`code_map`** walks the repo (respecting `.gitignore`), hashes each file with [blake3](https://github.com/BLAKE3-team/BLAKE3), and extracts public API summaries using [tree-sitter](https://tree-sitter.github.io/). Results cached at `.cache/taoki/code-map.json`.
- **`index`** parses a single file and returns its structural skeleton. Test code is automatically detected and collapsed — Python (`test_*`, `Test*`), Go (`Test*`, `Benchmark*`), TypeScript/JS (`describe`, `it`, `test`), Rust (`#[test]`, `#[cfg(test)]`). Files matching test naming patterns are collapsed entirely.
- **`dependencies`** queries a cached dependency graph (`.cache/taoki/deps.json`) showing internal imports, reverse dependencies, and external packages.

## Caching

Results are cached per-file using blake3 content hashes at `.cache/taoki/` in your repository. The cache is safe to delete at any time. Add `.cache/` to your `.gitignore`.

## Update

Re-run the install script to upgrade to the latest release.

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/uninstall.sh | bash
```

Or manually: `rm -rf ~/.claude/plugins/taoki`

## License

MIT
