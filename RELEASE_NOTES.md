# Taoki v1.2.0 Release Notes

## Highlights

This release redesigns the plugin hooks to eliminate **alarm fatigue**. Previously, hooks fired the same generic message on every Read, Glob, and Grep — in practice, the model habituated and ignored them all, even on 2000+ line files where xray would have saved ~90% of tokens.

Hooks are now **context-aware**: they check file size, detect targeted reads, and inspect glob patterns before deciding whether to nudge. Tested against 8 benchmark repos, this reduces hook noise by 57-96% while preserving nudges for exactly the files where taoki tools provide the most value.

## Changes

### Smart Read Hook (Size-Aware)

The Read hook now checks three conditions before firing:
1. **Extension** — only source files (.rs, .py, .ts, .js, .go, .java, etc.)
2. **Targeted read** — silent when `offset`/`limit` are provided (already knows structure)
3. **File size** — silent for files under 300 lines; nudges for 300+ with actual line count

Old message (every source file): "Consider calling xray on this file first..."
New message (only 300+ line files): "This file has 2263 lines. xray shows the structural skeleton with line numbers in ~10% of the tokens — consider xray first, then Read the sections you need."

### Smart Glob Hook (Pattern-Aware)

The Glob hook now reads the pattern and only fires when it contains `**` (broad exploration). Targeted lookups like `hooks/*.sh` or `src/index/languages/*.rs` are silent.

### Grep Hook Removed

Removed entirely. It fired on every Grep including perfectly valid literal string searches. The nudge ("use xray for structural questions") was generic and rarely actionable.

### SessionStart Workflow

Added a workflow sequence to the session start message:

```
Workflow: radar to orient → xray files of interest → Read specific sections → ripple before modifying
```

This teaches the tool sequence upfront rather than relying on per-call nudges.

### Error Handling

All hooks now follow a strict rule: **any failure → silent allow**. File stat failures, JSON parse errors, or missing files never disrupt the user experience.

## Noise Reduction (Tested on Benchmark Repos)

| Repo | Files | Old (every read) | New (300+ only) | Reduction |
|------|-------|-------------------|-----------------|-----------|
| ripgrep | 100 | 100 | 43 | 57% |
| flask | 83 | 83 | 18 | 78% |
| next.js | 21,140 | 21,140 | 837 | 96% |
| caddy | 301 | 301 | 107 | 64% |
| guava | 3,245 | 3,245 | 718 | 77% |
| cobra | 36 | 36 | 11 | 69% |
| tokio | 767 | 767 | 177 | 76% |
| serde | 208 | 208 | 24 | 88% |

## Breaking Changes

None. This is a hooks-only change — no changes to the MCP tools (radar, xray, ripple), cache format, or binary.

## Stats

- **187 unit tests**, 0 clippy warnings
- **4 hooks** (down from 5 — Grep hook removed)
- **57-96% noise reduction** across 8 tested repos

## Upgrading

Re-run the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh -o /tmp/taoki-install.sh && bash /tmp/taoki-install.sh
```

Or if installed via marketplace: `claude plugin install taoki@monet-plugins` (will fetch the new version automatically).

No cache changes — existing `.cache/taoki/` directories are unaffected.
