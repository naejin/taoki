# User Feedback Improvements

## Context

Real-world user feedback after testing all three taoki tools against a medium Python project (~60 files):

1. **`code_map` output too large** — 71KB for ~60 files overflowed context. The "one shot overview" advantage partially collapses when the output must be persisted to disk and read in chunks.
2. **`dependencies` is shallow** — only one hop deep, doesn't show which symbols are imported (just files), insufficient for real impact analysis.
3. **Tool role confusion** — not immediately obvious when to use `code_map` vs `index` vs `code_map(files=[...])` vs Grep/Glob.

User assessment: `index` is the standout tool. `dependencies` is "modestly useful." `code_map` is good for cold-start but outgrown quickly.

## Goal

Address all three feedback points to make taoki a more complete toolkit rather than "index plus two extras." Preserve backward compatibility for small repos.

---

## 1. Dependencies: Depth + Symbols

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
- Add BFS helper for `used_by` expansion with cycle detection (visited set per chain)
- Render symbols from `ImportInfo.symbols` in `depends_on` and `used_by` output

Changes to `src/mcp.rs`:
- Add `depth` to `dependencies` tool definition (optional integer, default 1, min 1, max 3)
- Parse and pass `depth` to `query_deps`
- Update `dependencies` tool description to mention `depth` parameter. The existing description already claims "specific symbols used" — symbol rendering now fulfills that claim, no description change needed for symbols.

### Testing

Unit tests with synthetic `DepsGraph`:
- Depth 1: identical to current output
- Depth 2: verify second-level dependents appear indented
- Depth 3: verify third level
- Cycle detection: A → B → A stops with `(cycle)` marker
- Symbol rendering: verify parenthetical format for files with symbols, clean format for files without

---

## 2. Code Map: Output Size Scaling

Two complementary changes, no new parameters.

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
- **Name-only API** — function/type names without signatures. `fetch_user(user_id: str) -> User` becomes `fetch_user`. Full signatures available via `index` or `code_map(files=[...])`.
- Files within each directory still sorted alphabetically

For result sets under `GROUPING_THRESHOLD`, current behavior is preserved — flat list with full signatures. If `code_map` is called with globs that narrow the result to <100 files, flat mode is used even if the full repo has 500+ files.

### B) Truncation of long API lists

Named constants control truncation: `FN_TRUNCATE_THRESHOLD` (8, show first 6) and `TYPE_TRUNCATE_THRESHOLD` (12, show first 10). When a file exceeds the threshold, the output shows `threshold - 2` items then `... +N more`.

This caps the worst-case per-file output. The user who sees `... +9 more` knows to call `index` on that file for the full picture.

### Implementation

Changes to `src/codemap.rs`:
- After `results` are sorted, detect file count and branch to grouped vs flat formatting
- Grouped formatter: collect files by directory prefix, emit directory headers, format each file with name-only API
- Truncation helper: applied in both grouped and flat modes
- No cache changes — `CacheEntry` stores full data, formatting is output-time only

### Testing

- Synthetic test with >100 files: verify directory headers, name-only output, aggregate counts
- Synthetic test with <100 files: verify flat output preserved with full signatures
- Truncation: file with 15 public functions shows 6 + `... +9 more`
- Edge cases: files in repo root (no directory prefix), single-file directories

---

## 3. Hook Refinement: Tool Role Clarity

All hooks are text-only changes to shell scripts. No Rust changes.

### SessionStart

Replace the current wall-of-text with a decision tree:

```
Structural code intelligence available (taoki plugin):
- Exploring a new codebase? → code_map (no args) for repo overview
- Understanding a specific file before reading? → index (structural skeleton, 70-90% fewer tokens)
- About to modify a file? → dependencies (impact analysis: what depends on it)
- Need structure for 2+ specific files? → code_map with files: [...] (batch skeletons)
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

No changes — current Agent hook text about including Taoki tool instructions in subagent prompts remains appropriate.

---

## Cache Impact

**No cache version bumps required:**
- Dependencies depth is query-time BFS on the existing `DepsGraph` — cached data unchanged
- Code map directory grouping is output formatting — `CacheEntry` data unchanged
- Symbol rendering reads existing `ImportInfo.symbols` — already cached

---

## Estimated Scope

| Area | Files | Change Size | Risk |
|------|-------|-------------|------|
| `deps.rs` — depth BFS + symbols | 1 | ~80 lines + tests | Low — additive, default=1 preserves behavior |
| `mcp.rs` — depth param | 1 | ~15 lines | Low |
| `codemap.rs` — grouping + truncation | 1 | ~100 lines + tests | Low — threshold means small repos unchanged |
| Hook scripts | 4 files | ~20 lines total | None — text only |
| `CLAUDE.md` | 1 | Update `query_deps` signature, directory grouping behavior, hook descriptions | None |

Total: ~200-300 lines of Rust, ~20 lines of shell. All changes are backward-compatible.

## Post-Implementation

Update `CLAUDE.md` to reflect:
- New `query_deps` signature with `depth` parameter
- `code_map` directory grouping behavior and `GROUPING_THRESHOLD` constant
- `FN_TRUNCATE_THRESHOLD` and `TYPE_TRUNCATE_THRESHOLD` constants
- Updated hook descriptions
- Updated `dependencies` tool description noting `depth` parameter and symbol rendering
