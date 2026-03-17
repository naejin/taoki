# User Feedback Improvements

## Context

Real-world user feedback after testing all three taoki tools against a medium Python project (~60 files):

1. **`code_map` output too large** — 71KB for ~60 files overflowed context. The "one shot overview" advantage partially collapses when the output must be persisted to disk and read in chunks.
2. **`dependencies` is shallow** — only one hop deep, doesn't show which symbols are imported (just files), insufficient for real impact analysis.
3. **Tool role confusion** — not immediately obvious when to use `code_map` vs `index` vs `code_map(files=[...])` vs Grep/Glob.

User assessment: `index` is the standout tool. `dependencies` is "modestly useful." `code_map` is good for cold-start but outgrown quickly.

## Goal

Eliminate the overlap between tools, give each a clear identity with a distinctive name, and improve depth/symbol detail. This is a major overhaul — clean breaks over backward compatibility.

**Design principle — three sensing tools, each with a distinct purpose:**
- **`radar`** (was `code_map`) — wide sweep, sees everything at a distance (repo overview)
- **`xray`** (was `index`) — focused beam, sees inside one thing (file structure)
- **`ripple`** (was `dependencies`) — impact trace, what's connected and how far changes spread

No tool should try to do what another does.

---

## 0. Tool Renaming

All three MCP tools get new names:

| Old name | New name | Rationale |
|----------|----------|-----------|
| `code_map` | `radar` | Wide scan — sweep the repo and get back blips (files with tags and public API) |
| `index` | `xray` | Focused beam — see through a file to its structural bones |
| `dependencies` | `ripple` | Impact trace — change a file, see the ripple effect through the codebase |

This affects:
- Tool names in `tool_definitions()` in `mcp.rs`
- Dispatch in `handle_tools_call()` in `mcp.rs`
- Function names: `call_code_map` → `call_radar`, `call_index` → `call_xray`, `call_dependencies` → `call_ripple`
- All hook scripts and `hooks.json` (tool references)
- `CLAUDE.md` documentation
- Plugin skill files (`skills/taoki-map.md`, `skills/taoki-index.md`, `skills/taoki-deps.md`)
- Cache file names: `.cache/taoki/code-map.json` → `.cache/taoki/radar.json`, new `.cache/taoki/xray.json`

Internal function names (`build_code_map`, `query_deps`, etc.) can optionally be renamed for consistency, but this is lower priority than the external-facing names.

---

## 1. Tool Separation: radar and xray

### Current overlap

`code_map` (now `radar`) has two modes:
- Without `files`: repo overview (one line per file with public API + tags)
- With `files`: batch skeleton mode (`build_batch_skeletons`) — essentially runs `index_source` on each file

The `files` mode duplicates what `xray` does, creating confusion about which to use. The user feedback explicitly flagged this: "batch skeleton mode and the index tool overlap — it's not immediately obvious when to use which."

### Changes

**Remove the `files` parameter from `radar` entirely.** `radar` becomes purely a navigator — no skeletons, no structural detail. Its job is answering "what's in this repo and where should I look?"

**Add file-based caching to `xray`.** Currently `xray` has only an in-memory cache (`INDEX_CACHE` thread_local in `mcp.rs`) that is lost between MCP sessions. Add persistent file-based caching so repeated calls on unchanged files are instant, even across sessions.

### radar changes (src/codemap.rs)

**Deletions:**
- Remove `build_batch_skeletons()` function entirely
- Remove `detail_files` parameter from `build_code_map()` — new signature: `build_code_map(root: &Path, globs: &[String]) -> Result<String, CodeMapError>`
- Remove `skeleton` field from `CacheEntry` struct
- Remove the `files` parse block from `call_radar()` in `mcp.rs`

**Switch from `extract_all()` to `extract_public_api()`:** `radar` currently calls `index::extract_all()` which returns `(PublicApi, skeleton)` — then discards the skeleton for overview mode but stores it in cache for batch mode. With batch mode gone, switch to `index::extract_public_api()` which returns only `(Vec<String>, Vec<String>)` (types, functions). This avoids computing skeletons (including body analysis) that `radar` never uses.

**Reset `CACHE_VERSION` to 1.** This is a major overhaul — start fresh rather than incrementing from the old version scheme. Old caches are simply discarded.

**Updated `CacheEntry`:**
```rust
struct CacheEntry {
    hash: String,
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
    tags: Vec<String>,
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

### xray caching changes (src/mcp.rs)

**New file-based cache** at `.cache/taoki/xray.json`:
```rust
const XRAY_CACHE_VERSION: u32 = 1;
const XRAY_DISK_CACHE_FILE: &str = "xray.json";

#[derive(Debug, Serialize, Deserialize)]
struct XrayCiskCache {
    version: u32,
    files: HashMap<String, XrayDiskEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct XrayDiskEntry {
    hash: String,
    skeleton: String,
}
```

Cache keys are **relative paths** (relative to repo root), matching `radar`'s cache key convention. This requires finding the repo root from the file path.

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

**Updated `call_xray` flow:**
1. Read the file into memory, compute blake3 hash (same as current)
2. Check in-memory cache (`INDEX_CACHE` thread_local) → return if hash matches
3. Find repo root via `find_repo_root()`. If found, check file-based cache → return if hash matches, also populate in-memory cache
4. Cache miss → parse with `index_source()`, write to both in-memory and file-based caches
5. If repo root not found (file outside a git repo), skip file-based caching, use in-memory only

**File locking:** Same pattern as `radar`'s cache — use `fs2::FileExt` for shared lock on read, exclusive lock on write. Load/save helpers follow the `load_cache`/`save_cache` pattern in codemap.rs (atomic write via temp file + rename).

**The existing in-memory cache stays.** It's a hot-path optimization that avoids JSON deserialization on repeated calls within the same session. The file-based cache provides persistence across sessions.

### MCP tool definition changes (src/mcp.rs)

**radar tool** (was `code_map`):
- Name: `"radar"`
- Remove `files` property from `inputSchema`
- New description: `"Sweep the codebase — one line per file with public types, function names, and heuristic tags. Use this first to orient in an unfamiliar repo or find which files are relevant. Results are cached (blake3) so repeated calls are near-instant. Supports globs to narrow scope."`

**xray tool** (was `index`):
- Name: `"xray"`
- New description: `"See through a source file to its structural bones: imports, type definitions, function signatures with body insights, and line numbers. ~70-90% fewer tokens than reading the full file. Results are cached so repeated calls on unchanged files are instant. Use this to understand a file's architecture before reading specific sections with the Read tool. Supports: Rust, Python, TypeScript, JavaScript, Go, Java."`

### Testing

**radar — tests to delete** (batch skeleton tests, now obsolete):
1. `code_map_with_files_returns_skeleton_only`
2. `code_map_files_normalizes_dot_slash_prefix`
3. `code_map_files_ignores_nonexistent`
4. `code_map_batch_returns_index_format`
5. `code_map_test_file_skeleton_collapsed`
6. `code_map_batch_matches_index_source`
7. `code_map_parse_error_no_skeleton`
8. `cache_stores_skeleton` (asserts skeleton field in cache JSON)

All remaining tests calling `build_code_map(root, &[], &[])` must have the third argument removed.

**radar — new/updated tests:**
- Verify `extract_public_api` returns the same types/functions as `extract_all().0` (add a cross-check test)
- Verify old caches are discarded (test loads a stale cache, confirms it's rebuilt)

**Callers outside codemap.rs:** `benches/speed.rs` calls `build_code_map(dir.path(), &[], &[])` at 3 call sites — must remove the third argument.

**xray caching:**
- Unit test: call `call_xray` twice on the same file → second call returns cached result without re-parsing
- Unit test: modify file between calls → cache miss, re-parse
- Unit test: file outside git repo → works without file-based cache (in-memory only)
- Unit test: corrupt/missing cache file → graceful fallback to parse

---

## 2. Radar: Output Size Scaling

Two complementary changes to `radar`'s output formatting, no new parameters.

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
- **Name-only API** — function/type names without signatures. `fetch_user(user_id: str) -> User` becomes `fetch_user`. Full signatures available via `xray`.
- Files within each directory still sorted alphabetically
- Files at repo root (no directory prefix) grouped under a `(root)` header or listed first without a header

For result sets under `GROUPING_THRESHOLD`, flat list with full signatures. If globs narrow the result to <100 files, flat mode is used even if the full repo has 500+ files.

### B) Truncation of long API lists

Named constants control truncation: `FN_TRUNCATE_THRESHOLD` (8, show first 6) and `TYPE_TRUNCATE_THRESHOLD` (12, show first 10). When a file exceeds the threshold, the output shows `threshold - 2` items then `... +N more`.

This caps the worst-case per-file output. When truncated, append a cue: `... +9 more (use xray for full list)`. This nudges agents toward the intended workflow.

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

## 3. Ripple: Depth + Symbols

### New parameter

Add `depth` (integer, optional, default 1, max 3) to the `ripple` MCP tool.

### Behavior

**`used_by` direction** — expands to `depth` levels via BFS on the existing cached `DepsGraph`. Direct dependents are listed at the first indent level. Their dependents (depth 2) are shown beneath them with an arrow prefix. Rendered as an indented tree:

```
used_by:
  src/enrichment/pipeline.py (merge_records, deduplicate)
    → src/cli/enrich.py
    → src/cli/batch.py
  src/enrichment/ai_extractor.py (EnrichmentRecord)
```

At depth 1 (default), flat list under `used_by:` with symbols shown. When depth > 1, the section header includes it: `used_by (depth=2):` so users can tell at a glance what they're seeing.

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

### MCP tool definition

**ripple tool** (was `dependencies`):
- Name: `"ripple"`
- New description: `"Trace the ripple effect of changing a file. Shows what it imports (depends_on), what imports it (used_by) with depth control, and external dependencies. Symbols are shown where available. Use depth=2 or depth=3 to see the full blast radius before modifying a file."`
- Add `depth` to `inputSchema` (optional integer, default 1, min 1, max 3)

### Implementation

Changes to `src/deps.rs`:
- Modify `query_deps` signature: `query_deps(graph: &DepsGraph, file: &str, depth: u32) -> String`
- Add BFS helper for `used_by` expansion with visited set for cycle detection, producing indented tree output
- Render symbols from `ImportInfo.symbols` in both `depends_on` and `used_by` output
- For `used_by` symbol sourcing: when file B appears under file A, look up B's `FileImports` and find the `ImportInfo` where `path == A` to get the symbols B imports from A

Changes to `src/mcp.rs`:
- Rename dispatch: `"ripple"` → `call_ripple`
- Parse `depth` from arguments and pass to `query_deps`

### Testing

Unit tests with synthetic `DepsGraph`:
- Depth 1: flat list with symbols
- Depth 2: verify second-level dependents appear indented with arrow prefix
- Depth 3: verify third level with double indentation
- Cycle detection: A → B → A stops with `(cycle)` marker
- Symbol rendering: verify parenthetical format for files with symbols, clean format for files without
- Symbol sourcing in used_by: verify correct symbols at depth 2 (from parent, not root)

---

## 4. Hook Refinement: Tool Role Clarity

All hooks are text-only changes to shell scripts and `hooks.json`. No Rust changes.

**Files to modify** (5 total):
- `hooks/hooks.json` — SessionStart inline message
- `hooks/check-read.sh` — PreToolUse Read
- `hooks/check-agent.sh` — PreToolUse Agent
- `hooks/check-glob.sh` — PreToolUse Glob
- `hooks/check-grep.sh` — PreToolUse Grep

### SessionStart

Replace with a decision tree using the new tool names:

```
Structural code intelligence available (taoki plugin):
- Exploring a new codebase? → radar (no args) for tagged repo overview
- Understanding a specific file? → xray (structural skeleton with line numbers, 70-90% fewer tokens than reading)
- About to modify a file? → ripple (what depends on it, with depth for blast radius)
Always call radar first when orienting in an unfamiliar repo, then xray on files of interest.
```

### PreToolUse Read

```
Consider calling mcp__taoki__xray on this file first to get its structure
with line numbers, then Read only the sections you need. If you're about to
modify this file, mcp__taoki__ripple shows what depends on it.
```

### PreToolUse Glob

```
If you're exploring project structure (not searching for a specific file),
mcp__taoki__radar gives a tagged overview with public APIs — one call
instead of glob + multiple reads.
```

### PreToolUse Grep

```
For structural questions (what functions does this file export? what's the
class hierarchy?), mcp__taoki__xray or radar are more precise than
text search. For literal string lookups, Grep is the right tool.
```

### PreToolUse Agent

```
This subagent has access to Taoki MCP tools for code intelligence.
mcp__taoki__radar (repo overview with tags), mcp__taoki__xray
(single file skeleton), mcp__taoki__ripple (import/export graph with depth).
Call radar first when exploring a codebase, then xray on files of interest.
```

---

## Cache Impact

| Cache | Change | Version? |
|-------|--------|----------|
| `.cache/taoki/radar.json` (was `code-map.json`) | Remove `skeleton` field, reset version to 1, rename file | Fresh start |
| `.cache/taoki/xray.json` (was in-memory only) | **New** — file-based cache for xray skeletons | New file, v1 |
| `.cache/taoki/deps.json` | No structural change — depth is query-time BFS, symbols already stored | No |

Old caches are discarded on first call after upgrade — clean slate.

---

## Estimated Scope

| Area | Files | Change Size | Risk |
|------|-------|-------------|------|
| `codemap.rs` — remove batch skeletons, remove skeleton from cache, drop `#[serde(default)]`, switch to `extract_public_api`, directory grouping, truncation, delete 8 tests, update remaining test signatures | 1 | ~200 lines changed + tests | Medium — output format change |
| `mcp.rs` — rename tools, rename dispatch functions, add xray disk cache, add `find_repo_root`, add `depth` parsing for ripple, remove `files` parsing | 1 | ~150 lines + tests | Medium — rename + caching |
| `deps.rs` — depth BFS + symbol rendering in `query_deps` | 1 | ~100 lines + tests | Low |
| `benches/speed.rs` — remove third arg from `build_code_map` calls | 1 | ~3 lines | None |
| Hook scripts + hooks.json | 5 files | ~25 lines total | None — text only |
| Plugin skills — update tool names in skill files | 3 files | ~10 lines | None — text only |
| `CLAUDE.md` | 1 | Documentation updates | None |

Total: ~450-500 lines of Rust changes, ~35 lines of shell/markdown.

## Post-Implementation

Update `CLAUDE.md` to reflect:
- Tool renames: `code_map` → `radar`, `index` → `xray`, `dependencies` → `ripple`
- `radar` no longer has `files` parameter — tool separation with `xray`
- `radar` output format: directory grouping for >100 files, `GROUPING_THRESHOLD` constant
- `FN_TRUNCATE_THRESHOLD` and `TYPE_TRUNCATE_THRESHOLD` constants
- `radar` now uses `extract_public_api()` instead of `extract_all()`
- `CacheEntry` no longer has `skeleton` field, `CACHE_VERSION` reset to 1
- Cache files renamed: `radar.json`, new `xray.json`
- New xray file-based cache with `XRAY_CACHE_VERSION`
- `find_repo_root()` helper in `mcp.rs`
- New `query_deps` signature with `depth` parameter
- `ripple` tool description and depth parameter
- Updated hook descriptions with new tool names
- Updated tool descriptions for all three tools
