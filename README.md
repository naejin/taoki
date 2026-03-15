# Taoki

A Claude Code plugin that gives Claude structural understanding of your codebase. Instead of reading entire files, Claude gets compact summaries — public types, function signatures, dependency graphs, and line numbers — so it can navigate large codebases faster and with far fewer tokens.

Taoki provides three MCP tools:

- **`code_map`** — Scans a repository and returns a one-line-per-file summary showing public types, function signatures, and heuristic tags like `[entry-point]`, `[tests]`, `[error-types]`, `[module-root]`, `[data-models]`, `[interfaces]`, `[http-handlers]`, `[barrel-file]`, and `[cli]`. Results are cached (blake3 hashes), so repeated calls are near-instant.
- **`index`** — Returns the structural skeleton of a single file: imports, type definitions, function signatures, impl blocks, and module declarations, all with line numbers. Test code is automatically detected and collapsed for all supported languages. Typically 70-90% fewer tokens than reading the full file.
- **`dependencies`** — Shows what a file imports and what imports it. Returns internal dependencies (`depends_on`), reverse dependencies (`used_by`), and external packages. Use this for impact analysis before modifying a file.

All tools use [tree-sitter](https://tree-sitter.github.io/) for accurate, fast parsing.

## Supported Languages

| Language | Extensions |
|----------|------------|
| Rust | `.rs` |
| Python | `.py`, `.pyi` |
| TypeScript | `.ts`, `.tsx` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| Go | `.go` |
| Java | `.java` |

## Install

Requires a Rust toolchain (install via [rustup](https://rustup.rs/) if needed).

```bash
claude plugin add /path/to/taoki
```

Or if you've cloned it to a standard location:

```bash
claude plugin add ~/projects/taoki
```

The first time Claude uses the plugin, it automatically compiles the binary (`cargo build --release`). No manual build step needed.

## Update

Pull the latest changes and the binary will be rebuilt automatically on next use:

```bash
cd /path/to/taoki
git pull
rm -f target/release/taoki  # forces rebuild on next invocation
```

## How It Works

When Claude starts a session, Taoki runs as an MCP server over stdio. Claude can call the three tools at any time during the conversation:

1. **`code_map`** walks the repository (respecting `.gitignore`), hashes each file with blake3, and extracts public API summaries using tree-sitter. Each file gets heuristic tags based on its role (entry point, tests, data models, etc.). Results are cached at `.cache/taoki/code-map.json`. Also builds a dependency graph cached at `.cache/taoki/deps.json`.

2. **`index`** parses a single file and returns its structural skeleton — everything you need to understand the file's architecture without reading every line. Test code is automatically detected and collapsed for Python (`test_*`, `Test*`), Go (`Test*`, `Benchmark*`, `Example*`), TypeScript/JavaScript (`describe`, `it`, `test`), and Rust (`#[test]`, `#[cfg(test)]`). Files matching test naming conventions (e.g., `test_auth.py`, `LoginTest.java`) are collapsed entirely.

3. **`dependencies`** queries the dependency graph to show what a file imports and what imports it. This enables impact analysis — before modifying a file, check `used_by` to see what will be affected.

## Usage in Practice

### Orient in an unfamiliar codebase

Ask Claude to map the project first:

> "Map the codebase structure"

Claude calls `code_map` and gets a full overview — every file, its public types, and exported functions — in a single response. This replaces several rounds of `find`, `grep`, and file reading.

### Understand a file before editing

> "Show me the structure of src/auth/middleware.ts"

Claude calls `index` and sees every function signature, type, and import with line numbers. It can then read only the specific sections it needs with the `Read` tool, using the line numbers from the index.

### Narrow scope with globs

> "Map just the API routes"

Claude calls `code_map` with a glob like `["src/routes/**/*.ts"]` to get a focused view of a subsystem.

### Check impact before editing

> "What files depend on src/auth/middleware.ts?"

Claude calls `dependencies` and sees which files import the middleware. This prevents breaking changes by showing the blast radius before any edits.

### Typical workflow

1. `code_map` on the repo — understand architecture, use `[tags]` to find relevant files
2. `dependencies` on files you plan to modify — check impact via `used_by`
3. `index` on key files — get structural skeleton with line numbers
4. `Read` specific line ranges identified from the index
5. Make targeted edits with full context

This pattern uses significantly fewer tokens than reading files end-to-end, and gives Claude a better mental model of how the code is organized.

## Caching

Taoki caches results per-file using blake3 content hashes. The cache lives at `.cache/taoki/` in the scanned repository (`code-map.json` for the structural map, `deps.json` for the dependency graph) and is safe to delete at any time. Add `.cache/taoki/` to your `.gitignore` (Taoki does not do this automatically).

## License

MIT
