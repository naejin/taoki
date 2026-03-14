# Taoki Design Spec

## Overview

Taoki is a Claude Code plugin that bundles a Rust MCP server (stdio) providing two code-intelligence tools: single-file structural indexing and repo-wide code mapping. No external LLM calls — Claude Code does all the reasoning.

## Goals

Higher precision on the input leads to higher precision on the output. Taoki improves Claude Code's output quality by giving it focused, structural understanding of a codebase — instead of reading thousands of lines of implementation detail, Claude Code sees the architecture: what types exist, what functions are public, how files relate. This produces better-targeted code changes, fewer hallucinated references, and more accurate reasoning about where to make edits.

- Improve Claude Code's input signal with structural codebase understanding
- Repo-wide code maps that let Claude Code reason about which files matter before reading any of them
- Single-file skeletons that show architecture (types, signatures, line ranges) without implementation noise
- Token efficiency is a side benefit (~70-90% fewer tokens), not the primary goal
- Incremental caching (blake3 hash) so the tools stay fast on large repos
- Installable as a Claude Code plugin via `claude plugin add`
- Two slash commands (`/taoki-map`, `/taoki-index`) for discoverability

## Non-Goals

- No LLM calls from taoki itself (no API keys, no extra costs)
- No TUI or standalone CLI usage
- No "summary" or "when_to_use" fields (those require LLM generation — Claude Code can infer file relevance from type/function names, and use `index` to drill into ambiguous files like `utils.rs`)
- No auto-context tool (Claude Code reasons over the code-map output itself)

## Plugin Structure

```
taoki/
├── .claude-plugin/
│   └── plugin.json          ← minimal manifest (name, description, author)
├── .mcp.json                ← MCP server config
├── commands/
│   ├── taoki-map.md
│   └── taoki-index.md
├── scripts/
│   └── run.sh               ← builds if needed, then execs the binary
├── Cargo.toml
└── src/
    ├── main.rs              ← MCP stdio server
    ├── index.rs             ← single-file tree-sitter skeleton
    ├── codemap.rs           ← repo-wide structural map + caching
    └── languages/
        ├── mod.rs
        ├── rust.rs
        ├── python.rs
        ├── typescript.rs
        ├── go.rs
        └── java.rs
```

## Plugin Manifest

`.claude-plugin/plugin.json`:

```json
{
  "name": "taoki",
  "version": "0.1.0",
  "description": "Code indexing and structural mapping for Claude Code",
  "author": {
    "name": "Daylon"
  },
  "keywords": ["code-index", "code-map", "tree-sitter"]
}
```

`.mcp.json`:

```json
{
  "taoki": {
    "command": "${CLAUDE_PLUGIN_ROOT}/scripts/run.sh",
    "args": []
  }
}
```

## Build & Install Strategy

There is no `postInstall` hook in the Claude Code plugin system. The solution is a launcher script that builds the binary on first run:

`scripts/run.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$DIR/target/release/taoki"
if [ ! -f "$BIN" ]; then
  cargo build --release --manifest-path "$DIR/Cargo.toml" >&2
fi
exec "$BIN" "$@"
```

This means:
- First invocation after install triggers a `cargo build` (requires Rust toolchain)
- Subsequent invocations are instant (binary already exists)
- Build output goes to stderr so it doesn't interfere with MCP stdio protocol
- User must have `cargo` installed (reasonable for a Rust-based tool targeting developers)

## Tool 1: `index`

Single-file tree-sitter skeleton extraction.

### Input

```json
{
  "path": "/absolute/path/to/file.rs"
}
```

### Output

Compact markdown with line numbers. Example for a Rust file:

```
module doc:
  //! Parses source files into compact skeletons.

imports:
  std::path::Path
  tree_sitter::Parser

consts:
  MAX_FILE_SIZE: u64 [25]

types:
  pub enum IndexError [28-37]
  pub enum Language [39-53]

impls:
  Language [55-107]
    pub from_extension(ext: &str) -> Option<Self> [56-72]
    ts_language(&self) -> tree_sitter::Language [74-89]
    extractor(&self) -> &dyn LanguageExtractor [91-106]

fns:
  pub index_file(path: &Path) -> Result<String, IndexError> [109-124]
  pub index_source(source: &[u8], lang: Language) -> Result<String, IndexError> [126-164]

tests: lines 167, 189, 206, ...
```

### Behavior

- Supports: Rust (.rs), Python (.py, .pyi), TypeScript (.ts, .tsx), JavaScript (.js, .jsx, .mjs, .cjs), Go (.go), Java (.java)
- Returns error for unsupported file types
- Max file size: 2MB
- No caching — tree-sitter parsing is fast enough (~ms per file)
- Sections are ordered: module doc, imports, consts, types, traits, impls, fns, mod, macros, tests
- Sections with no entries are omitted
- Line numbers in `[N]` or `[N-M]` format
- Public items marked with `pub`/`export`
- Struct/class fields shown (truncated with `...` if >8)
- Test modules/functions collapsed to just line numbers

### Error Handling

- Unsupported file type → error with the extension name
- File too large (>2MB) → error with file size and max
- File not found / permission denied → IO error
- Tree-sitter parse failure → parse error (file may have syntax errors)

### Implementation

Port maki's `maki-code-index` crate. The architecture is:

1. `Language::from_extension()` — detect language from file extension
2. `tree_sitter::Parser` — parse the file into an AST
3. `LanguageExtractor` trait — per-language logic for which AST nodes matter and how to summarize them (same as maki's trait in `maki-code-index/src/common.rs`)
4. `format_skeleton()` — render the extracted entries as compact markdown

## Tool 2: `code_map`

Repo-wide structural map with incremental caching.

### Input

```json
{
  "path": "/repo/root",
  "globs": ["src/**/*.rs"]
}
```

- `path`: required, the repo root to scan
- `globs`: optional, include patterns only (no `!` negation — use specific globs to narrow scope). Defaults to all supported file types. Respects `.gitignore` via the `ignore` crate.

### Output

Clean markdown, one entry per file, sorted by path. Includes line count to help Claude Code estimate read cost:

```
- src/db.rs (120 lines) - public_types: Pool, Connection - public_functions: connect(url) -> Pool, query(sql) -> Rows
- src/internal.rs (45 lines) - public_types: (none) - public_functions: (none)
- src/server.rs (340 lines) - public_types: Config, Server, Route - public_functions: start(config) -> Server, handle(req) -> Response
```

Files with no public API are listed with `(none)` markers so Claude Code knows they exist.

### Caching

Cache location: `.cache/taoki/code-map.json` in the scanned directory.

Cache schema:

```json
{
  "version": 1,
  "files": {
    "src/main.rs": {
      "hash": "blake3hex...",
      "lines": 120,
      "public_types": ["Config", "Server"],
      "public_functions": ["start(config) -> Server", "handle(req) -> Response"]
    }
  }
}
```

The `version` field allows cache invalidation on schema changes.

On each call:
1. Walk files matching the globs (via `ignore` crate, respects .gitignore)
2. For each file, compute blake3 hash
3. If hash matches cache entry, use cached result
4. If hash differs or file is new, re-index with tree-sitter
5. Remove cache entries for files no longer in the walk
6. Write updated cache
- First run on a large repo may take seconds; subsequent runs are near-instant for unchanged files

### Error Handling

- Path does not exist → error
- Not a git repo → still works; `.gitignore` rules just don't apply
- Single file fails to parse → skip that file, include it in output with `(parse error)` marker, continue with the rest
- Cache file corrupted → discard cache, rebuild from scratch
- Cannot write cache (permissions) → return results anyway, log warning to stderr

### Parallelism

File hashing (blake3) runs in parallel across files. Tree-sitter parsers are not `Send`, so parsing uses a per-thread parser. The `ignore` crate's `WalkBuilder` supports parallel directory walking natively.

### Implementation

1. Walk the directory using the `ignore` crate
2. Filter by glob patterns using the `globset` crate
3. For each file, compute blake3 and check cache
4. For uncached/changed files, run tree-sitter and extract only public types and public function signatures
5. Serialize cache as JSON, output as markdown

## Slash Commands

### `/taoki-map`

`commands/taoki-map.md`:

```markdown
---
allowed-tools: mcp__taoki__code_map, mcp__taoki__index
description: Build a structural map of this repository
---

Call the `mcp__taoki__code_map` tool to build a structural map of this repository.

If arguments are provided, use them as glob patterns. Otherwise, default to all supported file types.

After receiving the code map, provide a concise summary of the repository's architecture:
- Key modules and their responsibilities
- Main types and how they relate
- Entry points and public API surface
```

### `/taoki-index`

`commands/taoki-index.md`:

```markdown
---
allowed-tools: mcp__taoki__index
description: Show the structural skeleton of a source file
---

Call the `mcp__taoki__index` tool on the specified file path.

After receiving the index, present the file structure and highlight:
- The main types and their purpose
- Key functions and what they do
- Notable patterns (traits, impls, test coverage)
```

## Supported Languages

| Language | Extensions | Tree-sitter grammar |
|----------|-----------|-------------------|
| Rust | .rs | tree-sitter-rust |
| Python | .py, .pyi | tree-sitter-python |
| TypeScript | .ts, .tsx | tree-sitter-typescript |
| JavaScript | .js, .jsx, .mjs, .cjs | tree-sitter-javascript |
| Go | .go | tree-sitter-go |
| Java | .java | tree-sitter-java |

## MCP Server

The binary (`taoki`) is an MCP server using the stdio transport. It:

1. Reads JSON-RPC messages from stdin
2. Registers two tools: `index` and `code_map`
3. Handles `initialize`, `tools/list`, and `tools/call` methods
4. Writes JSON-RPC responses to stdout
5. Logs to stderr

The MCP protocol implementation can use the `rmcp` crate or be hand-rolled (the stdio protocol is straightforward JSON-RPC over stdin/stdout).

## Workflow

How Claude Code uses taoki in practice:

1. **Understand the repo**: `/taoki-map` or `code_map(path: ".")` — gets the full structural map
2. **Zoom into a file**: `/taoki-index src/server.rs` or `index(path: "src/server.rs")` — gets the detailed skeleton
3. **Read what matters**: Uses built-in `Read` with offset/limit on the exact lines

Each step narrows Claude Code's focus: from repo-wide structure, to file-level architecture, to the exact lines that matter. The result is higher-precision input at each stage, which produces higher-precision output — better code changes, fewer mistakes, less wasted context on irrelevant code.

## MCP Tool Descriptions

These are the `description` fields registered with MCP. They tell Claude Code when to use each tool — critical for autonomous usage.

### `index`

```
Return a compact structural skeleton of a source file: imports, type definitions, function signatures, and their line numbers. ~70-90% fewer tokens than reading the full file. Use this to understand a file's architecture before reading specific sections with the Read tool. Supports: Rust, Python, TypeScript, JavaScript, Go, Java.
```

### `code_map`

```
Build an incremental structural map of a codebase. Returns one line per file with public types and public function signatures. Use this FIRST when you need to understand a repository's structure or find which files are relevant to a task. Results are cached (blake3 hash) so repeated calls are near-instant. Supports glob patterns to narrow scope.
```

## Reference Implementation

The `index` tool is a port of maki's `maki-code-index` crate. The reference source is at:

- `/home/daylon/projects/maki/maki/maki-code-index/src/lib.rs` — entry point (`index_file`, `index_source`, `Language` enum)
- `/home/daylon/projects/maki/maki/maki-code-index/src/common.rs` — `LanguageExtractor` trait, `SkeletonEntry` struct, `format_skeleton()`, section ordering
- `/home/daylon/projects/maki/maki/maki-code-index/src/rust.rs` — Rust extractor (representative example of a language module)
- `/home/daylon/projects/maki/maki/maki-code-index/src/python.rs` — Python extractor
- `/home/daylon/projects/maki/maki/maki-code-index/src/typescript.rs` — TypeScript/JavaScript extractor
- `/home/daylon/projects/maki/maki/maki-code-index/src/go.rs` — Go extractor
- `/home/daylon/projects/maki/maki/maki-code-index/src/java.rs` — Java extractor
- `/home/daylon/projects/maki/maki/maki-code-index/Cargo.toml` — dependencies and feature flags

The `code_map` tool is new (not in maki). It reuses the same tree-sitter extraction but only keeps public types and public function signatures, adds blake3 caching, and formats output as a flat list rather than a per-section skeleton.

## MCP Protocol Details

The server implements MCP over stdio using JSON-RPC 2.0. Required methods:

1. **`initialize`** — respond with server info and capabilities (`tools` capability)
2. **`notifications/initialized`** — client acknowledgment, no response needed
3. **`tools/list`** — return the two tool definitions with their JSON Schema inputs
4. **`tools/call`** — dispatch to `index` or `code_map`, return result as `text` content

Tool input schemas (JSON Schema):

**`index`**:
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Absolute path to the source file to index"
    }
  },
  "required": ["path"]
}
```

**`code_map`**:
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Absolute path to the repository root to scan"
    },
    "globs": {
      "type": "array",
      "items": { "type": "string" },
      "description": "Optional glob patterns to filter files (e.g. [\"src/**/*.rs\"]). Defaults to all supported file types."
    }
  },
  "required": ["path"]
}
```

Tool responses use MCP content format:
```json
{
  "content": [
    { "type": "text", "text": "the skeleton or code map output" }
  ]
}
```

Errors use `isError: true`:
```json
{
  "content": [
    { "type": "text", "text": "error message" }
  ],
  "isError": true
}
```

## Dependencies

- `tree-sitter` (0.26) + language grammars (same versions as maki: all 0.23)
- `blake3` for content hashing
- `ignore` crate for gitignore-aware directory walking
- `globset` crate for glob pattern matching
- `serde` / `serde_json` for cache serialization and MCP protocol
- MCP stdio transport — hand-roll with `serde_json` line-delimited reading from stdin (the protocol is simple enough; avoids pulling in a heavy MCP SDK dependency)

## Decisions

- Cache location is `.cache/taoki/` in the scanned directory — not configurable in v0.1.0
- Users should add `.cache/taoki/` to their `.gitignore`
- `code_map` includes line counts per file (free to compute, helps Claude Code estimate read cost)
- No `!` negation in globs — use specific include patterns instead
- Hand-roll MCP protocol instead of using `rmcp` — the stdio JSON-RPC surface is small (4 methods) and avoids a heavy dependency
