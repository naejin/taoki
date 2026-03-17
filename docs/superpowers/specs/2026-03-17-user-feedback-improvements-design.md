# User Feedback Improvements

## Context

Real-world user feedback after testing all three taoki tools against a medium Python project (~60 files):

1. **`code_map` output too large** — 71KB for ~60 files overflowed context. The "one shot overview" advantage partially collapses when the output must be persisted to disk and read in chunks.
2. **`dependencies` is shallow** — only one hop deep, doesn't show which symbols are imported (just files), insufficient for real impact analysis.
3. **Tool role confusion** — not immediately obvious when to use `code_map` vs `index` vs `code_map(files=[...])` vs Grep/Glob.

User assessment: `index` is the standout tool. `dependencies` is "modestly useful." `code_map` is good for cold-start but outgrown quickly.

## Goal

Eliminate the overlap between `code_map` and `index` to give each tool a clear, distinct identity. Improve `dependencies` depth and symbol detail. Fix tool role confusion through hooks and clearer tool descriptions. Preserve backward compatibility for small repos.

**Design principle:** `code_map` is the navigator (what's where), `index` is the deep viewer (what's inside), `dependencies` is the impact analyzer (what's connected). No tool should try to do what another does.

---

## 1. Tool Separation: code_map and index

### Current overlap

`code_map` has two modes:
- Without `files`: repo overview (one line per file with public API + tags)
- With `files`: batch skeleton mode (`build_batch_skeletons`) — essentially runs `index_source` on each file

The `files` mode duplicates what `index` does, creating confusion about which to use. The user feedback explicitly flagged this: "batch skeleton mode and the index tool overlap — it's not immediately obvious when to use which."

### Changes

**Remove the `files` parameter from `code_map` entirely.** `code_map` becomes purely a navigator — no skeletons, no structural detail. Its job is answering "what's in this repo and where should I look?"

**Add file-based caching to `index`.** Currently `index` has only an in-memory cache (`INDEX_CACHE` thread_local in `mcp.rs`) that is lost between MCP sessions. Add persistent file-based caching so repeated calls on unchanged files are instant, even across sessions.

### code_map changes (src/codemap.rs)

**Deletions:**
- Remove `build_batch_skeletons()` function entirely (lines 340-371)
- Remove `detail_files` parameter from `build_code_map()` — new signature: `build_code_map(root: &Path, globs: &[String]) -> Result<String, CodeMapError>`
- Remove `skeleton` field from `CacheEntry` struct
- Remove the `files` parse block from `call_code_map()` in `mcp.rs`

**Switch from `extract_all()` to `extract_public_api()`:** `code_map` currently calls `index::extract_all()` which returns `(PublicApi, skeleton)` — then discards the skeleton for overview mode but stores it in cache for batch mode. With batch mode gone, switch to `index::extract_public_api()` which returns only `(Vec<String>, Vec<String>)` (types, functions). This avoids computing skeletons (including body analysis) that code_map never uses.

**Bump `CACHE_VERSION` to 7.** The `CacheEntry` struct changes (skeleton field removed), so existing caches will be invalidated and rebuilt on next call.

**Updated `CacheEntry`:**
```rust
struct CacheEntry {
    hash: String,
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
    tags: Vec<String>,
    // skeleton field removed — index has its own cache
}
```

**Updated `build_code_map` entry point:**
```rust
pub fn build_code_map(root: &Path, globs: &[String]) -> Result<String, CodeMapError> {
    // No more batch skeleton branch — always build overview
    let files = walk_files(root, globs)?;
    // ... rest of overview logic
}
```

### index caching changes (src/mcp.rs)

**New file-based cache** at `.cache/taoki/index.json`:
```rust
const INDEX_CACHE_VERSION: u32 = 1;
const INDEX_DISK_CACHE_FILE: &str = "index.json";

#[derive(Debug, Serialize, Deserialize)]
struct IndexDiskCache {
    version: u32,
    files: HashMap<String, IndexDiskEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct IndexDiskEntry {
    hash: String,
    skeleton: String,
}
```

Cache keys are **relative paths** (relative to repo root), matching code_map's cache key convention. This requires finding the repo root from the file path.

**Repo root discovery** — new helper in `mcp.rs`:
```rust
fn find_repo_root(file_path: &Path) -> Option<PathBuf> {
    let mut dir = file_path.parent()?;
    loop {
        // .git can be a directory (normal repos) or a file (worktrees).
        // exists() returns true for both, which is correct.
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}
```

**Updated `call_index` flow:**
1. Read the file into memory, compute blake3 hash (same as current)
2. Check in-memory cache (`INDEX_CACHE` thread_local) → return if hash matches
3. Find repo root via `find_repo_root()`. If found, check file-based cache → return if hash matches, also populate in-memory cache
4. Cache miss → parse with `index_source()`, write to both in-memory and file-based caches
5. If repo root not found (file outside a git repo), skip file-based caching, use in-memory only

**File locking:** Same pattern as code_map's cache — use `fs2::FileExt` for shared lock on read, exclusive lock on write. Load/save helpers follow the `load_cache`/`save_cache` pattern in codemap.rs (atomic write via temp file + rename).

**The existing in-memory cache stays.** It's a hot-path optimization that avoids JSON deserialization on repeated calls within the same session. The file-based cache provides persistence across sessions.

### MCP tool definition changes (src/mcp.rs)

**code_map tool:**
- Remove `files` property from `inputSchema`
- Update description: remove all mention of batch skeleton mode
- New description: `"Build a structural map of the codebase — one line per file with public types, function names, and heuristic tags. Use this first to orient in an unfamiliar repo or find which files are relevant. Results are cached (blake3) so repeated calls are near-instant. Supports globs to narrow scope."`

**index tool:**
- Update description: remove "For multiple files at once, use code_map with the files parameter"
- New description: `"Return a compact structural skeleton of a source file: imports, type definitions, function signatures with body insights, and line numbers. ~70-90% fewer tokens than reading the full file. Results are cached so repeated calls on unchanged files are instant. Use this to understand a file's architecture before reading specific sections with the Read tool. Supports: Rust, Python, TypeScript, JavaScript, Go, Java."`

### Testing

**code_map — tests to delete** (batch skeleton tests, now obsolete):
1. `code_map_with_files_returns_skeleton_only`
2. `code_map_files_normalizes_dot_slash_prefix`
3. `code_map_files_ignores_nonexistent`
4. `code_map_batch_returns_index_format`
5. `code_map_test_file_skeleton_collapsed`
6. `code_map_batch_matches_index_source`
7. `code_map_parse_error_no_skeleton`
8. `cache_stores_skeleton` (asserts skeleton field in cache JSON)

All remaining tests calling `build_code_map(root, &[], &[])` must have the third argument removed.

**code_map — new/updated tests:**
- Verify `extract_public_api` returns the same types/functions as `extract_all().0` (add a cross-check test)
- Verify `CACHE_VERSION` bump invalidates old caches (test loads a v6 cache, confirms it's discarded)

**Callers outside codemap.rs:** `benches/speed.rs` calls `build_code_map(dir.path(), &[], &[])` at 3 call sites — must remove the third argument.

**index caching:**
- Unit test: call `call_index` twice on the same file → second call returns cached result without re-parsing
- Unit test: modify file between calls → cache miss, re-parse
- Unit test: file outside git repo → works without file-based cache (in-memory only)
- Unit test: corrupt/missing cache file → graceful fallback to parse

---

## 2. Code Map: Output Size Scaling

Two complementary changes to code_map's output formatting, no new parameters.

### A) Directory grouping for large repos

When the filtered result set (post-glob, post-extension filtering) exceeds `GROUPING_THRESHOLD` (100 files), `build_code_map` switches from flat file listing to directory-grouped output:

```
src/enrichment/ (8 files, 4,200 lines)
  merge.py (814 lines) [data-models] - MergeResult, merge_records, deduplicate
  pipeline.py (320 lines) [entry-point] - run_pipeline, PipelineConfig
  conversions.py (200 lines) - EnrichmentRecord, convert_raw
src/db/ (5 files, 1,600 lines)
  models.py (848 lines) [data-models] - User, Session, Document
  queries.py (410 lines) - fetch_user, bulk_insert
```

Key differences from current flat output:
- **Directory headers** with aggregate file count and line count for orientation
- **Name-only API** — function/type names without signatures. `fetch_user(user_id: str) -> User` becomes `fetch_user`. Full signatures available via `index`.
- Files within each directory still sorted alphabetically
- Files at repo root (no directory prefix) grouped under a `(root)` header or listed first without a header

For result sets under `GROUPING_THRESHOLD`, current behavior is preserved — flat list with full signatures. If `code_map` is called with globs that narrow the result to <100 files, flat mode is used even if the full repo has 500+ files.

### B) Truncation of long API lists

Named constants control truncation: `FN_TRUNCATE_THRESHOLD` (8, show first 6) and `TYPE_TRUNCATE_THRESHOLD` (12, show first 10). When a file exceeds the threshold, the output shows `threshold - 2` items then `... +N more`.

This caps the worst-case per-file output. The user who sees `... +9 more` knows to call `index` on that file for the full picture.

### Implementation

Changes to `src/codemap.rs`:
- Add `GROUPING_THRESHOLD`, `FN_TRUNCATE_THRESHOLD`, `TYPE_TRUNCATE_THRESHOLD` constants
- After `results` are sorted, check `results.len() > GROUPING_THRESHOLD` and branch to grouped vs flat formatting
- Grouped formatter: collect files by directory prefix (first path component(s) before filename), emit directory headers with aggregate stats, format each file with name-only API
- Flat formatter: current logic with truncation added
- Truncation helper: applied in both grouped and flat modes
- No cache changes — formatting is output-time only

### Testing

- Synthetic test with >100 files: verify directory headers, name-only output, aggregate counts
- Synthetic test with <100 files: verify flat output preserved with full signatures
- Truncation: file with 15 public functions shows 6 + `... +9 more`
- Edge cases: files in repo root (no directory prefix), single-file directories

---

## 3. Dependencies: Depth + Symbols

### New parameter

Add `depth` (integer, optional, default 1, max 3) to the `dependencies` MCP tool.

### Behavior

**`used_by` direction** — expands to `depth` levels via BFS on the existing cached `DepsGraph`. Direct dependents are listed at the first indent level. Their dependents (depth 2) are shown beneath them with an arrow prefix. Rendered as an indented tree:

```
used_by:
  src/enrichment/pipeline.py (merge_records, deduplicate)
    → src/cli/enrich.py
    → src/cli/batch.py
  src/enrichment/ai_extractor.py (EnrichmentRecord)
```

At depth 1 (default), output is identical to current behavior — flat list under `used_by:`.

**Symbol sourcing for `used_by`:** Each entry shows the symbols that the *dependent* file imports from its parent in the tree. At depth 1, that's what the dependent imports from the queried file. At depth 2+, each entry shows what it imports from *its immediate parent in the tree*, not from the root file.

**`depends_on` direction** — stays at depth 1. Transitive dependencies of your dependencies are rarely useful.

**Cycle detection:** If a file appears twice in the same BFS chain, stop expanding that branch and append `(cycle)`.

### Symbol rendering

`ImportInfo` already has a `symbols: Vec<String>` field populated during extraction for Python, TypeScript/JavaScript, and Java. `query_deps` currently ignores it. Render symbols parenthetically when non-empty:

```
depends_on:
  src/db/models.py (User, Session)
  src/enrichment/conversions.py (EnrichmentRecord)
external:
  pandas
  sqlalchemy
```

Rust and Go extraction currently push empty symbol vecs (Go's import model is package-level, not symbol-level) — no change needed, those entries just show the file path with no parenthetical.

### Implementation

Changes to `src/deps.rs`:
- Modify `query_deps` signature: `query_deps(graph: &DepsGraph, file: &str, depth: u32) -> String`
- Add BFS helper for `used_by` expansion with visited set for cycle detection, producing indented tree output
- Render symbols from `ImportInfo.symbols` in both `depends_on` and `used_by` output
- For `used_by` symbol sourcing: when file B appears under file A, look up B's `FileImports` and find the `ImportInfo` where `path == A` to get the symbols B imports from A

Changes to `src/mcp.rs`:
- Add `depth` to `dependencies` tool definition (optional integer, default 1, min 1, max 3)
- Parse and pass `depth` to `query_deps`
- Update `dependencies` tool description to mention `depth` parameter. The existing description already claims "specific symbols used" — symbol rendering now fulfills that claim, no description change needed for symbols.

### Testing

**Baseline first:** There are currently no unit tests for `query_deps` output format in `deps.rs`. Before modifying the function, add a baseline snapshot test that captures the current depth-1 output format. This ensures the backward-compatibility claim is verifiable.

Unit tests with synthetic `DepsGraph`:
- Depth 1: identical to baseline output (backward compatibility)
- Depth 2: verify second-level dependents appear indented with arrow prefix
- Depth 3: verify third level with double indentation
- Cycle detection: A → B → A stops with `(cycle)` marker
- Symbol rendering: verify parenthetical format for files with symbols, clean format for files without
- Symbol sourcing in used_by: verify correct symbols at depth 2 (from parent, not root)

---

## 4. Hook Refinement: Tool Role Clarity

All hooks are text-only changes to shell scripts and `hooks.json`. No Rust changes.

**Files to modify** (5 total):
- `hooks/hooks.json` — SessionStart inline message (contains stale `files: [...]` reference)
- `hooks/check-read.sh` — PreToolUse Read (contains stale `files: [...]` reference)
- `hooks/check-agent.sh` — PreToolUse Agent (contains stale `files: [...]` reference)
- `hooks/check-glob.sh` — PreToolUse Glob
- `hooks/check-grep.sh` — PreToolUse Grep

### SessionStart

Replace the current wall-of-text with a decision tree. Updated to reflect the clear tool separation (no more batch skeletons):

```
Structural code intelligence available (taoki plugin):
- Exploring a new codebase? → code_map (no args) for tagged repo overview
- Understanding a specific file? → index (structural skeleton with line numbers, 70-90% fewer tokens than reading)
- About to modify a file? → dependencies (what depends on it, with depth for blast radius)
Always call code_map first when orienting in an unfamiliar repo, then index on files of interest.
```

### PreToolUse Read

Add dependencies nudge alongside existing index suggestion:

```
Consider calling mcp__taoki__index on this file first to get its structure
with line numbers, then Read only the sections you need. If you're about to
modify this file, mcp__taoki__dependencies shows what depends on it.
```

### PreToolUse Glob

Clarify when code_map beats Glob (structural exploration, not specific file lookup):

```
If you're exploring project structure (not searching for a specific file),
mcp__taoki__code_map gives a tagged overview with public APIs — one call
instead of glob + multiple reads.
```

### PreToolUse Grep

Acknowledge that Grep is sometimes the right tool:

```
For structural questions (what functions does this file export? what's the
class hierarchy?), mcp__taoki__index or code_map are more precise than
text search. For literal string lookups, Grep is the right tool.
```

### PreToolUse Agent

Update to remove the `files: [...]` reference (batch mode is gone). New subagent prompt text:

```
This subagent has access to Taoki MCP tools for code intelligence.
mcp__taoki__code_map (repo overview with tags), mcp__taoki__index
(single file skeleton), mcp__taoki__dependencies (import/export graph).
Call code_map first when exploring a codebase, then index on files of interest.
```

---

## Cache Impact

| Cache | Change | Version bump? |
|-------|--------|---------------|
| `.cache/taoki/code-map.json` | Remove `skeleton` field from `CacheEntry` | Yes — v6 → v7 |
| `.cache/taoki/index.json` | **New** — file-based cache for index skeletons | New file, starts at v1 |
| `.cache/taoki/deps.json` | No structural change — depth is query-time BFS, symbols already stored | No |

The code_map cache version bump means existing caches are rebuilt on first call after upgrade. This is a one-time cost, same as previous version bumps.

---

## Estimated Scope

| Area | Files | Change Size | Risk |
|------|-------|-------------|------|
| `codemap.rs` — remove batch skeletons, remove skeleton from cache, switch to `extract_public_api`, directory grouping, truncation, delete 8 tests, update remaining test signatures | 1 | ~200 lines changed + tests | Medium — cache version bump, output format change |
| `mcp.rs` — remove `files` parsing, add index disk cache, add `find_repo_root`, add `depth` parsing for deps | 1 | ~130 lines + tests | Low — additive caching, existing in-memory cache preserved |
| `deps.rs` — depth BFS + symbol rendering in `query_deps`, baseline test | 1 | ~100 lines + tests | Low — additive, default=1 preserves behavior |
| `benches/speed.rs` — remove third arg from `build_code_map` calls | 1 | ~3 lines | None — signature alignment |
| Hook scripts + hooks.json | 5 files | ~25 lines total | None — text only |
| `CLAUDE.md` | 1 | Documentation updates | None |

Total: ~430-450 lines of Rust changes, ~25 lines of shell. Backward-compatible except for code_map cache rebuild on upgrade.

## Post-Implementation

Update `CLAUDE.md` to reflect:
- `code_map` no longer has `files` parameter — tool separation with `index`
- `code_map` output format: directory grouping for >100 files, `GROUPING_THRESHOLD` constant
- `FN_TRUNCATE_THRESHOLD` and `TYPE_TRUNCATE_THRESHOLD` constants
- `code_map` now uses `extract_public_api()` instead of `extract_all()`
- `CacheEntry` no longer has `skeleton` field, `CACHE_VERSION` is 7
- New index file-based cache at `.cache/taoki/index.json` with `INDEX_CACHE_VERSION`
- `find_repo_root()` helper in `mcp.rs`
- New `query_deps` signature with `depth` parameter
- Updated hook descriptions
- Updated tool descriptions for all three tools
