# Taoki Improvements Design Spec

> Inspired by packs-pro patterns, adapted for zero-cost local-only execution.

## Constraint

All features must work without external API calls. Taoki is a Claude Code plugin — all intelligence comes from tree-sitter structural analysis, heuristics, and caching. No additional costs.

## Current State

Taoki exposes two MCP tools over stdio JSON-RPC:
- `code_map` — repo-level public API summary with blake3 caching
- `index` — file-level structural skeleton with line numbers

Two skills (`taoki-map`, `taoki-index`) and two commands guide Claude to use these tools. Supports Rust, Python, TypeScript, JavaScript, Go, Java.

---

## Phase 1: High-Impact Features

### 1. New `dependencies` Tool — Cross-File Dependency Graph

**Problem:** Claude has no way to know which files are related to each other. It guesses based on names and grep results.

**Solution:** A third MCP tool that, given a file path, returns what it imports and what imports it.

**How it works:**
- Reuse import extraction from each language extractor (already parsed for `index` skeleton)
- Build full dependency graph during `code_map` walk — for each file, extract import paths and resolve them to actual files in the repo
- Cache the graph alongside the code map (same blake3 invalidation, stored in `.cache/taoki/deps.json`)
- The `dependencies` tool queries this cached graph

**Import resolution strategy per language:**
- **Rust:** `use crate::foo::bar` → resolve `foo/bar.rs` or `foo/bar/mod.rs` relative to crate root. External crates (no `crate::`, `self::`, `super::` prefix) → marked as external, not resolved.
- **Python:** `from foo.bar import X` → resolve `foo/bar.py` or `foo/bar/__init__.py`. Relative imports use `.` prefix.
- **TypeScript/JavaScript:** `import X from './foo'` → resolve `./foo.ts`, `./foo.tsx`, `./foo/index.ts`, etc. Bare specifiers (no `./` or `../`) → external.
- **Go:** `import "github.com/org/repo/pkg"` → external. Only resolve imports matching the module path prefix.
- **Java:** `import com.example.Foo` → resolve based on directory structure convention (`com/example/Foo.java`).

**Resolution is best-effort.** Unresolvable imports are listed as external dependencies (useful context but not linked to files). Known limitation: Rust `pub use` re-exports are not followed — the import resolves to the file containing the `pub use`, not the file containing the original definition.

**Automatic graph building:** If `dependencies` is called and no cached graph exists, it triggers `build_code_map()` with default (empty) globs to build the graph. The dependency graph is always built for all supported files regardless of globs, since dependency edges can cross glob boundaries.

**MCP tool definition:**
```json
{
  "name": "dependencies",
  "description": "Show what a file imports and what imports it. Returns dependency and dependent files with the specific symbols used. Automatically builds the dependency graph if not cached. Use this to understand impact before modifying a file.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "file": {
        "type": "string",
        "description": "Absolute path to the source file to query"
      },
      "repo_root": {
        "type": "string",
        "description": "Absolute path to the repository root"
      }
    },
    "required": ["file", "repo_root"]
  }
}
```

**Example output:**
```
depends_on:
  src/codemap.rs (codemap::build_code_map)
  src/index/mod.rs (index::Language, index::index_source, index::extract_public_api)

used_by:
  src/main.rs (mcp::handle_request)

external:
  serde, serde_json, blake3
```

**Cache structure (`deps.json`):**
```json
{
  "version": 1,
  "graph": {
    "src/mcp.rs": {
      "imports": [
        { "path": "src/codemap.rs", "symbols": ["build_code_map"] },
        { "path": "src/index/mod.rs", "symbols": ["Language", "index_source"] }
      ],
      "external": ["serde", "serde_json"]
    }
  }
}
```

The graph is rebuilt alongside `code_map` — same file walk, same hash checks. Files that haven't changed keep their cached edges.

---

### 2. Heuristic `when_to_use` Tags in code_map Output

**Problem:** code_map lists types and functions, but Claude still has to guess which files are relevant to a task. packs-pro solves this with AI-generated summaries — we need a zero-cost alternative.

**Solution:** Heuristic tags derived from structural analysis, appended to each code_map line.

**Heuristic rules:**

| Priority | Signal | Tag |
|----------|--------|-----|
| 1 | Has `main()` or equivalent entry point | `entry-point` |
| 2 | Filename matches `*_test.*`, `test_*.*`, `*_spec.*`, or file contains test functions | `tests` |
| 3 | Only defines types/structs, no function bodies | `data-models` |
| 4 | Defines traits/interfaces with no implementations | `interfaces` |
| 5 | Has HTTP handler patterns (see detection criteria below) | `http-handlers` |
| 6 | Defines error types (types with `Error`/`Exception` in name) | `error-types` |
| 7 | Mostly re-exports (`pub use`, `pub mod`, `export * from`) | `barrel-file` |
| 8 | Has CLI arg parsing patterns (clap derive, argparse, flag package) | `cli` |
| 9 | Filename is `mod.rs`, `__init__.py`, `index.ts`, `index.js` | `module-root` |
| 10 | Module doc extracted (first sentence) | Used as fallback if no other tag matches |

Multiple tags per file are allowed. Tags are derived from data already available after parsing (public types, function names, file content patterns).

**`http-handlers` detection criteria per language:**
- **Java:** Annotations containing `Mapping` (`@GetMapping`, `@PostMapping`, `@RequestMapping`, etc.) or `@Path` (JAX-RS)
- **Python:** Decorators containing `route`, `get`, `post`, `put`, `delete` (Flask/FastAPI patterns)
- **Go:** Functions with parameters of type `http.ResponseWriter` or `*http.Request`
- **TypeScript/JS:** Functions/methods with names like `get`, `post`, `put`, `delete`, `patch` inside classes/objects, or Express-style `.get()`, `.post()` call patterns
- **Rust:** Functions with `#[get]`, `#[post]`, etc. attributes (actix/rocket patterns)

Note: These are heuristic string matches on already-extracted data (type names, function signatures, attributes). They don't require additional parsing. False positives are acceptable — tags are hints, not guarantees.

**Updated code_map output format:**
```
- src/main.rs (131 lines) [entry-point] - public_types: (none) - public_functions: (none)
- src/mcp.rs (333 lines) [http-handlers] - public_types: JsonRpcRequest, JsonRpcResponse - public_functions: handle_request()
- src/codemap.rs (317 lines) - public_types: CodeMapError - public_functions: build_code_map()
- src/index/mod.rs (762 lines) [interfaces, module-root] - public_types: Language, IndexError - public_functions: index_file(), index_source()
```

**Implementation location:** Tag computation happens in `codemap.rs` after `extract_public_api()`, using the parsed source and filename. Tags are stored in `CacheEntry` and cached alongside types/functions.

**CacheEntry changes:**
```rust
struct CacheEntry {
    hash: String,
    mtime: u128,       // Nanoseconds since epoch. Added in Phase 2 but schema includes it now
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
    tags: Vec<String>,  // NEW
}
```

---

### 3. Test Detection for All Languages

**Problem:** Only Rust detects and collapses test code. Other languages show test functions inline, wasting tokens and cluttering the skeleton.

**Solution:** Update each language extractor's `is_test_node()` implementation.

**Detection works at two levels:**

**Level 1 — Top-level node detection via `is_test_node()` (in extractors):**
This works for constructs that appear as root-level children in the tree-sitter AST.

| Language | Top-level test patterns |
|----------|------------------------|
| **Rust** | Already works: `#[test]` functions, `#[cfg(test)]` mod |
| **Python** | Top-level function declarations named `test_*`. Class declarations named `Test*` (entire class collapsed — classes named `Test*` are assumed to be fully test classes). |
| **Go** | Function declarations named `Test*`, `Benchmark*`, `Example*` (stdlib testing convention). |
| **TypeScript/JS** | `describe()`, `it()`, `test()` call expressions at statement level (these are root-level `expression_statement` nodes in test files). |

**Level 2 — Filename-level detection (in `code_map`, not extractor):**
Catches entire test files by naming convention, independent of AST analysis.

- `*_test.go` → entire file is tests (Go convention)
- `test_*.py`, `*_test.py` → entire file is tests
- `*.test.ts`, `*.spec.ts`, `*.test.js`, `*.spec.js` → entire file is tests
- `*Test.java`, `*Tests.java` → entire file is tests

**Java special case:** Java tests are methods with `@Test` inside class bodies — they are NOT root-level AST nodes. Rather than modifying the extractor trait for this, Java relies on **filename-level detection only** (`*Test.java`, `*Tests.java`). When a Java file is filename-detected as tests, `code_map` applies the `tests` tag and `index` collapses the entire file to `tests: [1-N]`. Individual `@Test` method detection within non-test-named classes is a non-goal for Phase 1.

**When an entire file is detected as tests:**
- `index` output shows `tests: [1-N]` (entire file)
- `code_map` applies the `tests` tag

**When individual test nodes are detected (top-level):**
- `index` collapses them into `tests: [line-range]` (same as Rust today)

---

### 4. Orchestration Skill — Unified Workflow

**Problem:** Current skills are passive hints ("use code_map before Glob"). Claude often ignores them or uses tools in suboptimal order.

**Solution:** Replace both skills with a single opinionated skill that enforces a disciplined workflow.

**Skill file: `skills/taoki-workflow.md`**

```yaml
name: taoki-workflow
description: "Use when starting ANY coding task in a project. Triggers on:
  implementing features, fixing bugs, refactoring, understanding code,
  exploring a repo, finding files to modify, planning changes,
  investigating issues. Use BEFORE Glob, Grep, Read, or Edit."
allowed-tools: mcp__taoki__code_map, mcp__taoki__index, mcp__taoki__dependencies
```

**Workflow the skill enforces:**

```
1. MAP       → code_map(repo_root) to see all files with [tags]
2. FOCUS     → dependencies(target_file) to find related files
3. INDEX     → index(file) on each file you plan to modify or need to understand
4. READ      → Read only specific line ranges using index line numbers
5. PLAN      → form approach based on structural understanding
6. IMPLEMENT → edit files with full context of dependencies and structure
```

**Key instructions in the skill body:**
- `code_map` is cached — always call it, never skip to "save time"
- Use `[tags]` to filter relevant files (e.g., look for `[http-handlers]` when fixing an API bug)
- Call `dependencies` on every file you plan to modify — check `used_by` for impact
- Never `Read` a full source file without `index` first — use line numbers for targeted reads
- When the code_map shows a file with `[tests]` tag related to files you're modifying, index it too

**Removed skills:** `taoki-map.md` and `taoki-index.md` are deleted — the unified skill replaces both.

**Commands stay:** `/taoki-map` and `/taoki-index` remain as manual shortcuts.

---

## Phase 2: Performance and Polish

### 5. mtime-First Caching

**Problem:** code_map reads and hashes every file on every call, even when nothing changed.

**Solution:** Check file mtime before computing blake3.

**Algorithm:**
1. `stat()` the file → get mtime
2. If cached mtime exists and matches → skip entirely (no read, no hash)
3. If mtime differs → read file, compute blake3
4. If hash matches cached hash → content unchanged (file was touched), update cached mtime only
5. If hash differs → re-parse, update full cache entry

**CacheEntry with mtime:**
```rust
struct CacheEntry {
    hash: String,
    mtime: u128,  // nanoseconds since epoch (sub-second precision)
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
    tags: Vec<String>,
}
```

**Precision note:** mtime uses nanosecond precision (`u128`) to avoid false cache hits from rapid writes within the same second. The blake3 hash provides a second layer of integrity checking regardless.

**Impact:** On a 500-file repo where 3 files changed: 497 cheap `stat()` calls + 3 file reads, vs. 500 file reads + 500 blake3 hashes.

### 6. Parallel Parsing with rayon

**Problem:** First-time code_map on a large repo parses files sequentially.

**Solution:** Use `rayon::par_iter()` for the parse phase.

**Scope:** Only the parse loop (read → hash → extract_public_api) is parallelized. Cache read/write remains single-threaded (already locked).

**Dependency:** Add `rayon` to Cargo.toml.

### 7. Doc Comment Extraction

**Problem:** `when_to_use` heuristic tags are useful but sometimes a file's module doc describes its purpose better.

**Solution:** Extract the first sentence of module-level doc comments as a `doc_summary` field.

- Already have `detect_module_doc()` which finds the line range
- Extend to also capture the text content (first sentence, max 120 chars)
- Store in CacheEntry, display in code_map when no heuristic tag matches (or always, as supplementary info)

**Example:**
```
- src/codemap.rs (317 lines) [doc: "Repo-wide code map with blake3 caching"] - public_types: ...
```

### 8. Structure-Only Mode

**Problem:** For very large repos (1000+ files), even code_map output is too many tokens.

**Solution:** Optional `structure_only` parameter on code_map.

When `true`, returns only paths + line counts + tags — no types or functions:
```
- src/main.rs (131 lines) [entry-point]
- src/mcp.rs (333 lines) [http-handlers]
- src/codemap.rs (317 lines)
- src/index/mod.rs (762 lines) [interfaces, module-root]
```

This is a cheap scan (mtime-only if cached, no parsing needed) that gives Claude enough to decide where to look deeper.

### 9. NDJSON Crash Recovery

**Problem:** First-time code_map on a very large repo can take 30+ seconds. If interrupted, all progress is lost.

**Solution:** Write progress to `.cache/taoki/progress.jsonl` as each file completes.

- Each line is a JSON object: `{"path": "...", "entry": {...}}`
- On startup, if `progress.jsonl` exists, recover completed entries before starting the walk
- After successful completion, delete the progress file
- Only relevant for cold starts on large repos
- Concurrency: Claude Code runs one plugin instance at a time, so `progress.jsonl` does not need locking. If this assumption changes, it should inherit the same fs2 locking as `code-map.json`.

### 10. Named Maps

**Problem:** Claude sometimes needs different views of the same repo (just the API layer, just the data models, etc.).

**Solution:** Optional `name` parameter on code_map.

- `name: "api"` → caches to `.cache/taoki/code-map-api.json`
- Default (no name) → `.cache/taoki/code-map.json` as today
- The `name` determines only the cache filename. Globs can vary per call — if different globs are passed with the same name, the cache is fully rebuilt (glob set is stored in cache metadata and compared)

---

## Cache Schema Changes

**Bump `CACHE_VERSION` to 2** when implementing Phase 1. Old caches are automatically invalidated.

**New cache file structure:**
```
.cache/taoki/
├── code-map.json          # default map (v2 with tags, mtime)
├── code-map-{name}.json   # named maps (Phase 2)
├── deps.json              # dependency graph
├── progress.jsonl          # crash recovery (transient, Phase 2)
├── code-map.lock          # existing lock file
└── deps.lock              # lock for deps cache
```

---

## Plugin Changes

**Skills:**
- Delete `skills/taoki-map.md`
- Delete `skills/taoki-index.md`
- Create `skills/taoki-workflow.md` (unified orchestration skill)

**Commands:**
- Keep `commands/taoki-map.md` and `commands/taoki-index.md` unchanged

**MCP tool registration:**
- Add `dependencies` tool to `tool_definitions()` in `mcp.rs`
- Add `call_dependencies()` handler

---

## Module Impact

| Module | Phase 1 Changes | Phase 2 Changes |
|--------|-----------------|-----------------|
| `mcp.rs` | Add `dependencies` tool definition and handler | Add `structure_only` param to code_map |
| `codemap.rs` | Add tags computation, CacheEntry fields, dependency graph building | mtime field, rayon, NDJSON recovery, named maps |
| `index/mod.rs` | No changes (filename-based test detection handled in `mcp.rs::call_index()`) | Doc comment text extraction |
| `index/languages/python.rs` | Add `is_test_node()` implementation | — |
| `index/languages/go.rs` | Add `is_test_node()` implementation | — |
| `index/languages/typescript.rs` | Add `is_test_node()` implementation | — |
| `index/languages/java.rs` | Add `is_test_node()` implementation | — |
| New: `src/deps.rs` | Dependency graph building and resolution | — |
| `skills/taoki-workflow.md` | New unified skill | — |
| `skills/taoki-map.md` | Deleted | — |
| `skills/taoki-index.md` | Deleted | — |

---

## Testing Strategy

All tests remain inline `#[cfg(test)]` using `tempfile` crate (existing pattern).

**New tests needed:**
- `deps.rs`: Resolution tests per language (Rust, Python, TS/JS, Go, Java), external detection, graph caching
- `codemap.rs`: Tag computation tests (entry-point, tests, data-models, etc.), mtime caching
- Language extractors: `is_test_node()` tests for Python, Go, TS/JS, Java
- Integration: multi-file repos with cross-references, verify dependency output
