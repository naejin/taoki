# Changelog

## v1.0.0 — Tool Overhaul

**Breaking change:** Tools renamed. `code_map` → `radar`, `index` → `xray`, `dependencies` → `ripple`. No backward compatibility — all three tools use new names immediately. Users upgrading from v0.x must restart their Claude Code session to pick up the new tool definitions.

### What changed

**Tool renames**
- `code_map` is now **`radar`** — sweep a repository for a structural overview
- `index` is now **`xray`** — see inside a single file's structure
- `dependencies` is now **`ripple`** — trace the ripple effect of changes

**Radar (formerly code_map)**
- Removed batch skeleton mode (`files` parameter). Radar is now purely a repo overview tool. Use xray for file skeletons.
- Added directory grouping for large repos (>100 files) — files grouped by directory with summary headers
- Added API truncation — long type/function lists capped with "use xray for full list" cue
- Switched from `extract_all` to `extract_public_api` (faster, no skeleton overhead)
- Cache file renamed from `code-map.json` to `radar.json`, version reset to 1

**Xray (formerly index)**
- Added persistent disk cache at `.cache/taoki/xray.json` — skeletons survive across MCP sessions
- Uses blake3 content hashing for invalidation, same as radar
- Falls back gracefully when outside a git repo (no disk cache, in-memory only)
- Corrupt cache files are silently discarded and rebuilt

**Ripple (formerly dependencies)**
- Added `depth` parameter (1-3) for used_by expansion — see not just direct dependents but what depends on those
- Symbols shown inline: `src/mcp.rs (JsonRpcRequest, ToolResult)` instead of bare file paths
- Cycle detection prevents infinite loops in circular dependency chains
- Depth header: `used_by (depth=2):` when depth > 1

**Hooks**
- SessionStart message rewritten as a decision tree: radar for exploration, xray for files, ripple for impact
- All PreToolUse hooks updated with new tool names and contextual guidance

**Commands and skills**
- `/taoki-map` → `/taoki-radar`, `/taoki-index` → `/taoki-xray`, `/taoki-deps` → `/taoki-ripple`
- Workflow skill rewritten for the radar → ripple → xray → Read flow

### Cache migration

Old cache files (`code-map.json`) are **not** migrated. On first use after upgrading, radar will rebuild from scratch (typically <1 second). The old `code-map.json` can be safely deleted. Xray cache (`xray.json`) is new and will be created on first use.

### For plugin developers

If you have hooks or skills that reference the old tool names (`mcp__taoki__code_map`, `mcp__taoki__index`, `mcp__taoki__dependencies`), update them to `mcp__taoki__radar`, `mcp__taoki__xray`, `mcp__taoki__ripple`.
