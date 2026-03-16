# Code Map Skeleton Cache and Detailed Mode

## Problem

The agent's typical flow for understanding a codebase requires N+1 tool calls:

1. `code_map` → repo overview (one line per file)
2. `index` on file A → full skeleton
3. `index` on file B → full skeleton
4. ...repeat for each file of interest

Each tool call has latency and context overhead. The `code_map` cache already stores public API per file but discards the full structural skeleton. If skeletons were cached alongside the public API, `code_map` could return them on demand — collapsing the common pattern into two calls total regardless of how many files the agent needs.

## Design

### Cache expansion

`CacheEntry` gains a `skeleton` field storing the full `format_skeleton()` output:

```rust
#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    hash: String,
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    skeleton: String,  // NEW: full index skeleton
}
```

**Cache version bumps from 2 to 3.** First run after upgrade triggers a full rebuild (existing behavior on version mismatch).

The skeleton is computed via `index::index_source()` during `build_code_map()` — the same function `index` uses. This means the cached skeleton is identical to what `index` would return (minus enrichment, which is layered on separately).

**Test file skeletons:** Files matching `is_test_filename()` get a collapsed skeleton (`tests: [1-N]`) instead of a full structural skeleton, matching the `index` tool's behavior. This ensures consistency between `code_map(files=[...])` and `index` for test files.

**`serde(default)` on `skeleton`:** This is a safety net for forward compatibility, not a migration path. The version bump from 2 to 3 triggers a full cache rebuild, so old entries without `skeleton` are never deserialized.

### `build_code_map` changes

**Single parse pass:** Currently `build_code_map` calls `extract_public_api()` per file, which creates a parser and parses the source. Adding a separate `index_source()` call would parse the same file a second time. Instead, refactor to parse once and derive both the public API and skeleton from the same parse tree. This means extracting a new internal function that takes a parsed tree and returns `(PublicApi, String)` — or having `index_source()` also return the public API as a byproduct.

Concretely: introduce a function like `index::extract_all(source, lang) -> Result<(PublicApi, String), IndexError>` that parses once, runs `extract_public_api` and `format_skeleton` on the same root node, and returns both. `build_code_map` calls this instead of `extract_public_api` alone.

On cache hit (hash matches), both the public API and skeleton are already available from the cached entry — no parsing needed.

**Test file handling:** Before calling `extract_all`, check `is_test_filename()`. If true, store skeleton as `format!("tests: [1-{}]\n", lines)` and still extract the public API normally (test files can export public items used by non-test code).

**`FileResult` refactored to struct:**

```rust
struct FileResult {
    path: String,
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
    tags: Vec<String>,
    parse_error: bool,
    skeleton: String,
}
```

The current 6-element tuple is already hard to read. Adding a 7th element is untenable.

`build_code_map()` signature changes: it now accepts an optional `files` parameter (list of relative paths). When provided, the output includes `[skeleton]` blocks for matching files.

```rust
pub fn build_code_map(
    root: &Path,
    globs: &[String],
    detail_files: &[String],  // NEW
) -> Result<String, CodeMapError>
```

### Output format

**Without `files` parameter** — unchanged:

```
- src/main.rs (42 lines) [entry-point] - public_types: (none) - public_functions: main()
- src/lib.rs (156 lines) [module-root] - public_types: Config, Error - public_functions: setup(Path), run(Config) -> Result<()>
  [enriched] Library root. error_handling: Error enum with IO, Parse variants.
```

**With `files: ["src/lib.rs"]`** — skeleton appended for matched files:

```
- src/main.rs (42 lines) [entry-point] - public_types: (none) - public_functions: main()
- src/lib.rs (156 lines) [module-root] - public_types: Config, Error - public_functions: setup(Path), run(Config) -> Result<()>
  [enriched] Library root. error_handling: Error enum with IO, Parse variants.
  [skeleton]
  imports: [1-3]
    std::io
    crate::config

  fns:
    pub setup(Path) [5-10]
    pub run(Config) -> Result<()> [12-30]

```

Each skeleton line is indented by 2 spaces to nest under the file entry. The `[skeleton]` marker appears after `[enriched]` (if present). Enrichment is NOT prepended inside the skeleton block (it's already shown as `[enriched]` on the summary line).

### MCP tool definition changes

**`code_map` input schema** — add `files` parameter:

```json
{
    "name": "code_map",
    "description": "Build an incremental structural map of a codebase. Returns one line per file with public types and public function signatures. Use this FIRST when you need to understand a repository's structure or find which files are relevant to a task. Pass `files` (array of relative paths) to include full structural skeletons inline for specific files — use this after identifying files of interest to avoid separate index calls. Results are cached (blake3 hash) so repeated calls are near-instant. Supports glob patterns to narrow scope.",
    "inputSchema": {
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
            },
            "files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional list of relative file paths to include full structural skeletons for. Use after an initial code_map call to get detailed structure for files of interest."
            }
        },
        "required": ["path"]
    }
}
```

**`index` description** — small addition:

```
"description": "Return a compact structural skeleton of a source file: imports, type definitions, function signatures, and their line numbers. ~70-90% fewer tokens than reading the full file. Use this to understand a file's architecture before reading specific sections with the Read tool. For multiple files, prefer code_map with the files parameter instead. Supports: Rust, Python, TypeScript, JavaScript, Go, Java."
```

### `call_code_map` changes in `mcp.rs`

Parse the new `files` parameter from arguments and pass to `build_code_map()`:

```rust
let detail_files: Vec<String> = args
    .get("files")
    .and_then(|v| v.as_array())
    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
    .unwrap_or_default();
```

### Hook updates

**SessionStart tool reminder** (`hooks/hooks.json` inline echo):

```
You have structural code intelligence tools available via the taoki plugin. Before reading source files, use: mcp__taoki__code_map (repo overview with tags — call with no args first, then pass files: [...] to get full skeletons for files of interest), mcp__taoki__index (single file skeleton with line numbers — 70-90% fewer tokens than reading), mcp__taoki__dependencies (import/export graph for impact analysis). Always call code_map first when exploring a codebase. When you need structure for multiple files, use code_map with the files parameter instead of calling index on each one separately.
```

**PreToolUse Read hook** (`hooks/check-read.sh`):

```
Consider calling mcp__taoki__index on this file first to get its structure with line numbers, then Read only the specific sections you need. If you need multiple files, use mcp__taoki__code_map with files: ["path1", "path2"] to get all skeletons in one call. This typically saves 70-90% of tokens.
```

**PreToolUse Glob hook** (`hooks/check-glob.sh`) — no change needed.

### Edge cases

- **Files in `files` param not in repo:** Silently ignored (no error, no skeleton block).
- **Files that failed to parse:** Skeleton stored as empty string, `(parse error)` line unchanged, no `[skeleton]` block.
- **Files over `MAX_FILE_SIZE`:** Already skipped, no skeleton cached.
- **Empty `files` array:** Same as no parameter (compact output only).
- **Cache size:** Skeletons are typically 10-50 lines per file at ~80 chars each. For a 500-file repo, adds ~200-400KB to cache. For large monorepos (5000+ files), could reach several MB. Acceptable for a disk cache but worth monitoring.
- **`globs` and `files` combined:** `[skeleton]` blocks are only emitted for files that appear in the overview output (the intersection of walked/glob-filtered files and the `files` param). If a file is filtered out by `globs`, no skeleton block is produced for it even if it's in the `files` list. This is the natural behavior when skeletons are appended during the output formatting loop.

### What does NOT change

- `index` tool — stays as-is, independent implementation, own in-memory cache
- `dependencies` tool — unrelated
- Enrichment system — `[enriched]` lines coexist with `[skeleton]` blocks, enrichment is NOT duplicated inside skeleton
- `extract_public_api()` — subsumed by the new `extract_all()` function but remains available for other callers (e.g., `index_file`)

## Testing

- Test `build_code_map` with no `files` param returns no skeletons (backward compatible)
- Test `build_code_map` with `files` param returns `[skeleton]` blocks for matched files only
- Test cached skeleton matches fresh `index_source()` output
- Test cache version migration (v2 cache triggers full rebuild to v3)
- Test nonexistent file paths in `files` produce no skeleton block
- Test parse error files produce no `[skeleton]` block
- Test `files` works independently of `globs`
- Test skeleton is properly indented (2 spaces per line)
- Test `[enriched]` appears before `[skeleton]` when both are present
- Test `files` entries with `./` prefix are normalized and still match

### Path normalization

The `files` parameter values are compared against relative paths produced by `strip_prefix(root)`, which yields paths like `src/lib.rs` (no leading `./`). Normalize `files` entries by stripping any leading `./` before matching. This prevents silent mismatches when the agent passes `./src/lib.rs` instead of `src/lib.rs`.
