# User Feedback Overhaul Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename tools (radar/xray/ripple), eliminate code_map/index overlap, add xray disk caching, add ripple depth+symbols, add radar directory grouping+truncation, update all hooks and docs.

**Architecture:** Three distinct MCP tools with no overlap: `radar` (repo overview), `xray` (file skeleton with disk cache), `ripple` (dependency graph with depth). Clean break — no backward compatibility.

**Tech Stack:** Rust, tree-sitter, blake3, serde_json, fs2 (file locking), tempfile (tests)

**Spec:** `docs/superpowers/specs/2026-03-17-user-feedback-improvements-design.md`

---

## File Map

| File | Role | Changes |
|------|------|---------|
| `src/mcp.rs` | MCP dispatch, tool definitions, xray caching | Rename tools+functions, remove `files` parsing, add xray disk cache, add `find_repo_root`, add `depth` parsing for ripple |
| `src/codemap.rs` | Radar implementation | Remove batch skeletons, remove skeleton from cache, switch to `extract_public_api`, rename cache file, add directory grouping + truncation |
| `src/deps.rs` | Ripple implementation | Add depth BFS + symbol rendering in `query_deps` |
| `benches/speed.rs` | Benchmark | Remove third arg from `build_code_map` calls |
| `hooks/hooks.json` | Hook config | Update SessionStart message with new tool names |
| `hooks/check-read.sh` | PreToolUse Read | Update tool references |
| `hooks/check-glob.sh` | PreToolUse Glob | Update tool references |
| `hooks/check-grep.sh` | PreToolUse Grep | Update tool references |
| `hooks/check-agent.sh` | PreToolUse Agent | Update tool references |
| `commands/taoki-map.md` | `/taoki-map` command | Rename to `taoki-radar.md`, update references |
| `commands/taoki-index.md` | `/taoki-index` command | Rename to `taoki-xray.md`, update references |
| `commands/taoki-deps.md` | `/taoki-deps` command | Rename to `taoki-ripple.md`, update references |
| `skills/taoki-workflow.md` | Workflow skill | Update all tool names and remove batch skeleton references |
| `.claude-plugin/plugin.json` | Plugin manifest | Update keywords |
| `CLAUDE.md` | Project docs | Full rewrite of tool names, descriptions, cache docs |

---

### Task 1: Rename tools in MCP dispatch (mcp.rs)

**Files:**
- Modify: `src/mcp.rs`

This is the foundation — all other tasks build on the new names being in place.

- [ ] **Step 1: Write failing test for new tool names**

Add to the test module in `src/mcp.rs`:

```rust
#[test]
fn tool_definitions_use_new_names() {
    let defs = tool_definitions();
    let tools = defs["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["xray", "radar", "ripple"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test tool_definitions_use_new_names -- --nocapture`
Expected: FAIL — names are still `["index", "code_map", "dependencies"]`

- [ ] **Step 3: Rename tool definitions and dispatch**

In `src/mcp.rs`, update `tool_definitions()`:
- Change `"name": "index"` → `"name": "xray"`
- Change `"name": "code_map"` → `"name": "radar"`
- Change `"name": "dependencies"` → `"name": "ripple"`
- Update descriptions to new text from spec (Section 1 and Section 3)
- Remove `files` property from radar's `inputSchema`
- Add `depth` property to ripple's `inputSchema`:
  ```json
  "depth": {
      "type": "integer",
      "description": "How many levels of used_by to expand (1-3, default 1). Higher values show more of the blast radius.",
      "default": 1,
      "minimum": 1,
      "maximum": 3
  }
  ```

Update `handle_tools_call()` dispatch:
- `"index"` → `"xray"` and `call_index` → `call_xray`
- `"code_map"` → `"radar"` and `call_code_map` → `call_radar`
- `"dependencies"` → `"ripple"` and `call_dependencies` → `call_ripple`

Rename the functions themselves: `call_index` → `call_xray`, `call_code_map` → `call_radar`, `call_dependencies` → `call_ripple`.

In `call_radar()`, remove the `detail_files` parsing block (lines 395-403) and change:
```rust
match codemap::build_code_map(std::path::Path::new(path), &globs, &detail_files)
```
to:
```rust
match codemap::build_code_map(std::path::Path::new(path), &globs)
```

In `call_ripple()`, parse depth:
```rust
let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(1).min(3).max(1) as u32;
```
And pass it: `crate::deps::query_deps(&graph, &rel, depth)`

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test tool_definitions_use_new_names -- --nocapture`
Expected: PASS

**WARNING: Tasks 1-4 form an atomic group.** The project will not compile until all four are done (Task 1 changes `build_code_map` and `query_deps` call signatures, Tasks 2 and 4 update the implementations to match). Do NOT push or run CI until Task 4 is complete. The test in this step only verifies tool definitions — use `cargo test tool_definitions_use_new_names` specifically, not `cargo test`.

- [ ] **Step 5: Commit**

```bash
git add src/mcp.rs
git commit -m "feat: rename MCP tools — radar, xray, ripple

Rename tool definitions and dispatch functions.
Remove files parameter from radar, add depth to ripple."
```

---

### Task 2: Strip radar of skeletons (codemap.rs)

**Files:**
- Modify: `src/codemap.rs`
- Modify: `benches/speed.rs`

- [ ] **Step 1: Update CacheEntry and build_code_map signature**

In `src/codemap.rs`:

Remove `skeleton` field from `CacheEntry` (lines 33-34):
```rust
// DELETE these two lines:
//    #[serde(default)]
//    skeleton: String,
```

Remove `#[serde(default)]` from the `tags` field (line 31-32) — clean break, no compat needed:
```rust
// Change from:
//    #[serde(default)]
//    tags: Vec<String>,
// To:
    tags: Vec<String>,
```

Change `CACHE_VERSION` from 6 to 1:
```rust
const CACHE_VERSION: u32 = 1;
```

Rename cache file constant:
```rust
const CACHE_FILE: &str = "radar.json";
```

Remove `build_batch_skeletons()` function entirely (lines 340-371).

Update `build_code_map` signature — remove `detail_files`:
```rust
pub fn build_code_map(root: &Path, globs: &[String]) -> Result<String, CodeMapError> {
```

Remove the batch skeleton branch at the top of `build_code_map` (lines 374-377):
```rust
// DELETE:
// if !detail_files.is_empty() {
//     return build_batch_skeletons(root, detail_files);
// }
```

Switch from `extract_all` to `extract_public_api`. Replace the parsing block (lines 444-465):
```rust
let (public_types, public_functions) =
    match index::extract_public_api(&source, lang) {
        Ok((types, fns)) => (types, fns),
        Err(_) => {
            results.push(FileResult {
                path: rel.clone(),
                lines,
                public_types: Vec::new(),
                public_functions: Vec::new(),
                tags: Vec::new(),
                parse_error: true,
            });
            continue;
        }
    };
```

Remove all `skeleton` references in the cache read/write blocks:
- In the cache hit block (~line 406-426), remove `skeleton: cached.skeleton.clone()` from both `results.push` and `new_files.insert`
- In the new entry block (~line 469-489), remove `skeleton: skeleton.clone()` and the `skeleton` variable

The success path after `extract_public_api` should retain the existing flow: compute tags → insert into `new_files` → push to `results`. Just without skeleton:

```rust
let tags = compute_tags(&rel, &public_types, &public_functions, &source);

new_files.insert(
    rel.clone(),
    CacheEntry {
        hash: hash.clone(),
        lines,
        public_types: public_types.clone(),
        public_functions: public_functions.clone(),
        tags: tags.clone(),
        // no skeleton field
    },
);

results.push(FileResult {
    path: rel,
    lines,
    public_types,
    public_functions,
    tags,
    parse_error: false,
});
```

- [ ] **Step 2: Update benches/speed.rs**

Remove the third argument from all 3 `build_code_map` calls:
```rust
// Change from: build_code_map(dir.path(), &[], &[])
// To:          build_code_map(dir.path(), &[])
```

- [ ] **Step 3: Delete obsolete tests, fix remaining test signatures**

Delete these 8 tests from `src/codemap.rs`:
1. `code_map_with_files_returns_skeleton_only`
2. `code_map_files_normalizes_dot_slash_prefix`
3. `code_map_files_ignores_nonexistent`
4. `code_map_batch_returns_index_format`
5. `code_map_test_file_skeleton_collapsed`
6. `code_map_batch_matches_index_source`
7. `code_map_parse_error_no_skeleton`
8. `cache_stores_skeleton`

In all remaining tests that call `build_code_map`, remove the third `&[]` argument.

Also update cache file path references in surviving tests:
- `caching_reuses_results`: change `.cache/taoki/code-map.json` → `.cache/taoki/radar.json`
- `cache_v2_triggers_rebuild`: change `code-map.json` → `radar.json` in both the write path and the read-back assertion

- [ ] **Step 4: Build and run tests**

Run: `cargo test --lib -- codemap`
Expected: All remaining codemap tests pass. Compile succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/codemap.rs benches/speed.rs
git commit -m "feat: strip radar of skeletons, switch to extract_public_api

Remove batch skeleton mode, skeleton cache field, and 8 obsolete tests.
Reset cache version to 1, rename cache file to radar.json."
```

---

### Task 3: Add xray disk cache (mcp.rs)

**Files:**
- Modify: `src/mcp.rs`

- [ ] **Step 1: Write failing test for disk cache**

Add to test module in `src/mcp.rs`:

```rust
#[test]
fn xray_disk_cache_persists() {
    let dir = tempfile::tempdir().unwrap();
    // Create a fake git repo so find_repo_root works
    fs::create_dir(dir.path().join(".git")).unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "pub fn hello() {}\n").unwrap();

    let args = serde_json::json!({ "path": file.to_str().unwrap() });

    // First call — parses and caches
    let r1 = call_xray(&args);
    assert!(!r1.is_error);

    // Clear in-memory cache to force disk read
    INDEX_CACHE.with(|c| c.borrow_mut().clear());

    // Second call — should hit disk cache
    let r2 = call_xray(&args);
    assert!(!r2.is_error);
    assert_eq!(r1.content[0].text, r2.content[0].text);

    // Verify cache file exists
    let cache_path = dir.path().join(".cache/taoki/xray.json");
    assert!(cache_path.exists(), "disk cache should exist");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test xray_disk_cache_persists -- --nocapture`
Expected: FAIL — no disk cache exists yet

- [ ] **Step 3: Implement find_repo_root and disk cache**

Add to `src/mcp.rs`:

```rust
use std::path::{Path, PathBuf};

const XRAY_CACHE_VERSION: u32 = 1;
const XRAY_CACHE_DIR: &str = ".cache/taoki";
const XRAY_CACHE_FILE: &str = "xray.json";

#[derive(Debug, Serialize, Deserialize)]
struct XrayDiskCache {
    version: u32,
    files: HashMap<String, XrayDiskEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct XrayDiskEntry {
    hash: String,
    skeleton: String,
}

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

fn xray_cache_path(root: &Path) -> PathBuf {
    root.join(XRAY_CACHE_DIR).join(XRAY_CACHE_FILE)
}

fn load_xray_cache(root: &Path) -> XrayDiskCache {
    let path = xray_cache_path(root);
    // Use fs2 shared lock for concurrent read safety
    let file = match std::fs::OpenOptions::new().read(true).open(&path) {
        Ok(f) => f,
        Err(_) => return XrayDiskCache { version: XRAY_CACHE_VERSION, files: HashMap::new() },
    };
    if file.lock_shared().is_ok() {
        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => { let _ = file.unlock(); return XrayDiskCache { version: XRAY_CACHE_VERSION, files: HashMap::new() }; }
        };
        let _ = file.unlock();
        match serde_json::from_str::<XrayDiskCache>(&data) {
            Ok(c) if c.version == XRAY_CACHE_VERSION => c,
            _ => XrayDiskCache { version: XRAY_CACHE_VERSION, files: HashMap::new() },
        }
    } else {
        XrayDiskCache { version: XRAY_CACHE_VERSION, files: HashMap::new() }
    }
}

fn save_xray_cache(root: &Path, cache: &XrayDiskCache) {
    let path = xray_cache_path(root);
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    // Use fs2 exclusive lock + atomic rename (same pattern as codemap.rs save_cache)
    let tmp = path.with_extension("tmp");
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        if std::fs::write(&tmp, &json).is_ok() {
            if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path) {
                if f.lock_exclusive().is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                    let _ = f.unlock();
                }
            }
        }
    }
}
```

Then update `call_xray` to use disk cache. After the in-memory cache check and before parsing, add:

```rust
// Check disk cache
let repo_root = find_repo_root(path);
let rel_path = repo_root.as_ref().and_then(|root| {
    path.strip_prefix(root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))
});

if let (Some(root), Some(ref rel)) = (&repo_root, &rel_path) {
    let disk_cache = load_xray_cache(root);
    if let Some(entry) = disk_cache.files.get(rel) {
        if entry.hash == hash {
            // Populate in-memory cache too
            INDEX_CACHE.with(|cache| {
                cache.borrow_mut().insert(path_buf.clone(), (hash.clone(), entry.skeleton.clone()));
            });
            return ToolResult {
                content: vec![ToolContent {
                    r#type: "text".to_string(),
                    text: entry.skeleton.clone(),
                }],
                is_error: false,
            };
        }
    }
}
```

After parsing (in the `Ok(raw_skeleton)` branch), add disk cache write:

```rust
// Save to disk cache
if let (Some(root), Some(ref rel)) = (&repo_root, &rel_path) {
    let mut disk_cache = load_xray_cache(root);
    disk_cache.files.insert(rel.clone(), XrayDiskEntry {
        hash: hash.clone(),
        skeleton: raw_skeleton.clone(),
    });
    save_xray_cache(root, &disk_cache);
}
```

Also handle test file case: after the test file skeleton is built, save to disk cache before returning.

- [ ] **Step 4: Run tests**

Run: `cargo test xray_disk_cache_persists -- --nocapture`
Expected: PASS

Run: `cargo test --lib`
Expected: All tests pass

- [ ] **Step 5: Write additional cache tests**

```rust
#[test]
fn xray_disk_cache_invalidated_on_change() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "pub fn hello() {}\n").unwrap();

    let args = serde_json::json!({ "path": file.to_str().unwrap() });
    let r1 = call_xray(&args);

    // Modify the file
    fs::write(&file, "pub fn hello() {}\npub fn world() {}\n").unwrap();
    INDEX_CACHE.with(|c| c.borrow_mut().clear());

    let r2 = call_xray(&args);
    assert_ne!(r1.content[0].text, r2.content[0].text, "should re-parse changed file");
}

#[test]
fn xray_works_outside_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    // No .git directory
    let file = dir.path().join("lib.rs");
    fs::write(&file, "pub fn hello() {}\n").unwrap();

    let args = serde_json::json!({ "path": file.to_str().unwrap() });
    let result = call_xray(&args);
    assert!(!result.is_error, "should work without git repo");
    assert!(result.content[0].text.contains("hello"));
}
```

- [ ] **Step 6: Write corrupt cache and cross-check tests**

```rust
#[test]
fn xray_corrupt_cache_falls_back_to_parse() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "pub fn hello() {}\n").unwrap();

    // Write corrupt cache
    let cache_dir = dir.path().join(".cache/taoki");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("xray.json"), "{{not valid json}}").unwrap();

    let args = serde_json::json!({ "path": file.to_str().unwrap() });
    let result = call_xray(&args);
    assert!(!result.is_error, "should gracefully fall back to parsing");
    assert!(result.content[0].text.contains("hello"));
}
```

Add to `src/index/mod.rs` test module (cross-check test):

```rust
#[test]
fn extract_public_api_matches_extract_all() {
    let src = b"pub struct Foo;\npub fn bar() {}\nfn private() {}\n";
    let (api, _skeleton) = extract_all(src, Language::Rust).unwrap();
    let (types, fns) = extract_public_api(src, Language::Rust).unwrap();
    assert_eq!(api.types, types);
    assert_eq!(api.functions, fns);
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test --lib`
Expected: All pass

- [ ] **Step 8: Commit**

```bash
git add src/mcp.rs src/index/mod.rs
git commit -m "feat: add xray disk cache for persistent skeleton caching

Skeletons persist in .cache/taoki/xray.json across MCP sessions.
Uses blake3 hash for invalidation, in-memory cache as hot path."
```

---

### Task 4: Add ripple depth + symbols (deps.rs)

**Files:**
- Modify: `src/deps.rs`

- [ ] **Step 1: Write failing tests for symbols and depth**

Add to test module in `src/deps.rs`:

```rust
#[test]
fn query_deps_renders_symbols() {
    let mut graph = DepsGraph { version: DEPS_VERSION, graph: HashMap::new() };
    graph.graph.insert("a.py".to_string(), FileImports {
        imports: vec![ImportInfo {
            path: "b.py".to_string(),
            symbols: vec!["Foo".to_string(), "Bar".to_string()],
            external: false,
        }],
    });
    graph.graph.insert("b.py".to_string(), FileImports { imports: vec![] });

    let out = query_deps(&graph, "a.py", 1);
    assert!(out.contains("b.py (Foo, Bar)"), "should show symbols: {out}");
}

#[test]
fn query_deps_depth_2_shows_transitive() {
    let mut graph = DepsGraph { version: DEPS_VERSION, graph: HashMap::new() };
    // a.py imports nothing
    graph.graph.insert("a.py".to_string(), FileImports { imports: vec![] });
    // b.py imports a.py
    graph.graph.insert("b.py".to_string(), FileImports {
        imports: vec![ImportInfo { path: "a.py".to_string(), symbols: vec!["X".to_string()], external: false }],
    });
    // c.py imports b.py
    graph.graph.insert("c.py".to_string(), FileImports {
        imports: vec![ImportInfo { path: "b.py".to_string(), symbols: vec!["Y".to_string()], external: false }],
    });

    let out = query_deps(&graph, "a.py", 2);
    assert!(out.contains("b.py"), "depth 1: b.py uses a.py: {out}");
    assert!(out.contains("c.py"), "depth 2: c.py uses b.py: {out}");
}

#[test]
fn query_deps_cycle_detection() {
    let mut graph = DepsGraph { version: DEPS_VERSION, graph: HashMap::new() };
    graph.graph.insert("a.py".to_string(), FileImports {
        imports: vec![ImportInfo { path: "b.py".to_string(), symbols: vec![], external: false }],
    });
    graph.graph.insert("b.py".to_string(), FileImports {
        imports: vec![ImportInfo { path: "a.py".to_string(), symbols: vec![], external: false }],
    });

    let out = query_deps(&graph, "a.py", 3);
    assert!(out.contains("(cycle)"), "should detect cycle: {out}");
}

#[test]
fn query_deps_depth_header() {
    let mut graph = DepsGraph { version: DEPS_VERSION, graph: HashMap::new() };
    graph.graph.insert("a.py".to_string(), FileImports { imports: vec![] });
    graph.graph.insert("b.py".to_string(), FileImports {
        imports: vec![ImportInfo { path: "a.py".to_string(), symbols: vec![], external: false }],
    });

    let out1 = query_deps(&graph, "a.py", 1);
    assert!(out1.contains("used_by:\n"), "depth 1 has plain header: {out1}");

    let out2 = query_deps(&graph, "a.py", 2);
    assert!(out2.contains("used_by (depth=2):"), "depth 2 has annotated header: {out2}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test query_deps_renders_symbols query_deps_depth_2 query_deps_cycle query_deps_depth_header -- --nocapture`
Expected: FAIL — `query_deps` doesn't accept depth and doesn't render symbols

- [ ] **Step 3: Implement depth + symbols in query_deps**

Replace the `query_deps` function in `src/deps.rs`:

```rust
pub fn query_deps(graph: &DepsGraph, file: &str, depth: u32) -> String {
    let mut out = String::new();

    // depends_on: internal files this file imports (always depth 1)
    // Deduplicate by path, merge symbols from duplicate imports
    let mut depends_map: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    if let Some(fi) = graph.graph.get(file) {
        for imp in &fi.imports {
            if !imp.external {
                let entry = depends_map.entry(imp.path.clone()).or_default();
                for sym in &imp.symbols {
                    if !entry.contains(sym) {
                        entry.push(sym.clone());
                    }
                }
            }
        }
    }

    out.push_str("depends_on:\n");
    for (path, symbols) in &depends_map {
        if symbols.is_empty() {
            out.push_str(&format!("  {path}\n"));
        } else {
            out.push_str(&format!("  {path} ({})\n", symbols.join(", ")));
        }
    }

    // used_by with depth expansion
    if depth > 1 {
        out.push_str(&format!("used_by (depth={depth}):\n"));
    } else {
        out.push_str("used_by:\n");
    }
    let mut visited = std::collections::HashSet::new();
    visited.insert(file.to_string());
    collect_used_by(graph, file, depth, 1, &mut visited, &mut out);

    // external: deduplicated external dependencies
    let mut external: Vec<String> = graph
        .graph
        .get(file)
        .map(|fi| {
            fi.imports
                .iter()
                .filter(|i| i.external)
                .map(|i| i.path.clone())
                .collect()
        })
        .unwrap_or_default();
    external.sort();
    external.dedup();

    out.push_str("external:\n");
    for ext in &external {
        out.push_str(&format!("  {ext}\n"));
    }

    out
}

fn collect_used_by(
    graph: &DepsGraph,
    target: &str,
    max_depth: u32,
    current_depth: u32,
    visited: &mut std::collections::HashSet<String>,
    out: &mut String,
) {
    if current_depth > max_depth {
        return;
    }

    let mut users: Vec<(String, Vec<String>)> = Vec::new();
    for (other_file, fi) in &graph.graph {
        if let Some(imp) = fi.imports.iter().find(|i| !i.external && i.path == target) {
            users.push((other_file.clone(), imp.symbols.clone()));
        }
    }
    users.sort_by(|a, b| a.0.cmp(&b.0));

    let indent = if current_depth == 1 {
        "  ".to_string()
    } else {
        format!("{}→ ", "  ".repeat(current_depth as usize))
    };

    for (user, symbols) in &users {
        if visited.contains(user) {
            out.push_str(&format!("{indent}{user} (cycle)\n"));
            continue;
        }
        if symbols.is_empty() {
            out.push_str(&format!("{indent}{user}\n"));
        } else {
            out.push_str(&format!("{indent}{user} ({})\n", symbols.join(", ")));
        }
        if current_depth < max_depth {
            visited.insert(user.clone());
            collect_used_by(graph, user, max_depth, current_depth + 1, visited, out);
            visited.remove(user);
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib -- deps`
Expected: All deps tests pass

- [ ] **Step 5: Run full test suite**

Run: `cargo test --lib`
Expected: All tests pass (mcp.rs now compiles since `query_deps` accepts depth)

- [ ] **Step 6: Commit**

```bash
git add src/deps.rs
git commit -m "feat: add depth BFS and symbol rendering to ripple

query_deps now accepts depth (1-3) for used_by expansion.
Symbols shown parenthetically. Cycle detection prevents infinite loops."
```

---

### Task 5: Add radar directory grouping + truncation (codemap.rs)

**Files:**
- Modify: `src/codemap.rs`

- [ ] **Step 1: Write failing test for truncation**

```rust
#[test]
fn truncation_caps_long_function_lists() {
    let dir = tempfile::tempdir().unwrap();
    // Create a file with 15 public functions
    let mut src = String::new();
    for i in 0..15 {
        src.push_str(&format!("pub fn func_{i}() {{}}\n"));
    }
    std::fs::write(dir.path().join("big.rs"), &src).unwrap();

    let result = build_code_map(dir.path(), &[]).unwrap();
    assert!(result.contains("... +9 more"), "should truncate: {result}");
    assert!(result.contains("use xray for full list"), "should include xray cue: {result}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test truncation_caps_long_function_lists -- --nocapture`
Expected: FAIL — no truncation logic yet

- [ ] **Step 3: Add truncation constants and helper**

At the top of `src/codemap.rs` with other constants:

```rust
const GROUPING_THRESHOLD: usize = 100;
const FN_TRUNCATE_THRESHOLD: usize = 8;
const TYPE_TRUNCATE_THRESHOLD: usize = 12;
const FN_TRUNCATE_SHOW: usize = 6;     // FN_TRUNCATE_THRESHOLD - 2
const TYPE_TRUNCATE_SHOW: usize = 10;   // TYPE_TRUNCATE_THRESHOLD - 2

fn truncate_list(items: &[String], threshold: usize, show: usize) -> (Vec<String>, usize) {
    if items.len() > threshold {
        let shown: Vec<String> = items[..show].to_vec();
        let remaining = items.len() - show;
        (shown, remaining)
    } else {
        (items.to_vec(), 0)
    }
}

fn format_names_only(items: &[String]) -> Vec<String> {
    items.iter().map(|s| {
        // Extract just the name: take everything before '(' or ' '
        s.split(|c: char| c == '(' || c == ':')
         .next()
         .unwrap_or(s)
         .split_whitespace()
         .last()
         .unwrap_or(s)
         .to_string()
    }).collect()
}
```

- [ ] **Step 4: Apply truncation to the flat formatter**

In `build_code_map`, replace the output formatting block (after `results.sort_by`). Update the formatting to apply truncation:

```rust
let use_grouping = results.len() > GROUPING_THRESHOLD;

if use_grouping {
    out = format_grouped(&results);
} else {
    out = format_flat(&results);
}
```

Extract the current flat formatting into a `format_flat` function and add truncation:

```rust
fn format_flat(results: &[FileResult]) -> String {
    let mut out = String::new();
    for fr in results {
        if fr.parse_error {
            out.push_str(&format!("- {} ({} lines) (parse error)\n", fr.path, fr.lines));
            continue;
        }
        let tags_str = if fr.tags.is_empty() {
            String::new()
        } else {
            format!(" {}", fr.tags.iter().map(|t| format!("[{t}]")).collect::<Vec<_>>().join(" "))
        };

        let (shown_types, more_types) = truncate_list(&fr.public_types, TYPE_TRUNCATE_THRESHOLD, TYPE_TRUNCATE_SHOW);
        let (shown_fns, more_fns) = truncate_list(&fr.public_functions, FN_TRUNCATE_THRESHOLD, FN_TRUNCATE_SHOW);

        let types_str = if shown_types.is_empty() {
            "(none)".to_string()
        } else {
            let mut s = shown_types.join(", ");
            if more_types > 0 {
                s.push_str(&format!(", ... +{more_types} more (use xray for full list)"));
            }
            s
        };
        let fns_str = if shown_fns.is_empty() {
            "(none)".to_string()
        } else {
            let mut s = shown_fns.iter()
                .map(|f| f.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join(", ");
            if more_fns > 0 {
                s.push_str(&format!(", ... +{more_fns} more (use xray for full list)"));
            }
            s
        };

        out.push_str(&format!(
            "- {} ({} lines){tags_str} - public_types: {types_str} - public_functions: {fns_str}\n",
            fr.path, fr.lines
        ));
    }
    out
}
```

- [ ] **Step 5: Run truncation test**

Run: `cargo test truncation_caps_long_function_lists -- --nocapture`
Expected: PASS

- [ ] **Step 6: Write failing test for directory grouping**

```rust
#[test]
fn directory_grouping_for_large_repos() {
    let dir = tempfile::tempdir().unwrap();
    // Create >100 files across directories
    for i in 0..60 {
        let subdir = dir.path().join(format!("src/mod_{}", i / 10));
        std::fs::create_dir_all(&subdir).unwrap();
        std::fs::write(subdir.join(format!("file_{i}.rs")), format!("pub fn f{i}() {{}}\n")).unwrap();
    }
    for i in 0..50 {
        let subdir = dir.path().join(format!("lib/pkg_{}", i / 10));
        std::fs::create_dir_all(&subdir).unwrap();
        std::fs::write(subdir.join(format!("mod_{i}.rs")), format!("pub struct S{i};\n")).unwrap();
    }

    let result = build_code_map(dir.path(), &[]).unwrap();
    // Should have directory headers
    assert!(result.contains("src/mod_0/"), "should have directory headers: {result}");
    // Should use name-only API (no full signatures)
    assert!(!result.contains("public_types:"), "grouped mode should not use flat format labels");
}
```

- [ ] **Step 7: Implement directory grouping**

Add `format_grouped` function:

```rust
fn format_grouped(results: &[FileResult]) -> String {
    let mut out = String::new();

    // Group by directory
    let mut groups: Vec<(String, Vec<&FileResult>)> = Vec::new();
    let mut current_dir = String::new();
    let mut current_files: Vec<&FileResult> = Vec::new();

    for fr in results {
        let dir = match fr.path.rfind('/') {
            Some(pos) => &fr.path[..=pos],
            None => "(root)/",
        };
        if dir != current_dir {
            if !current_files.is_empty() {
                groups.push((current_dir.clone(), std::mem::take(&mut current_files)));
            }
            current_dir = dir.to_string();
        }
        current_files.push(fr);
    }
    if !current_files.is_empty() {
        groups.push((current_dir, current_files));
    }

    for (dir, files) in &groups {
        let total_lines: usize = files.iter().map(|f| f.lines).sum();
        out.push_str(&format!("{dir} ({} files, {} lines)\n", files.len(), total_lines));

        for fr in files {
            let filename = fr.path.rsplit('/').next().unwrap_or(&fr.path);
            let tags_str = if fr.tags.is_empty() {
                String::new()
            } else {
                format!(" {}", fr.tags.iter().map(|t| format!("[{t}]")).collect::<Vec<_>>().join(" "))
            };

            // Name-only API
            let type_names = format_names_only(&fr.public_types);
            let fn_names = format_names_only(&fr.public_functions);
            let mut all_names: Vec<String> = Vec::new();

            let (shown_types, more_types) = truncate_list(&type_names, TYPE_TRUNCATE_THRESHOLD, TYPE_TRUNCATE_SHOW);
            all_names.extend(shown_types);
            if more_types > 0 {
                all_names.push(format!("... +{more_types} more types"));
            }

            let (shown_fns, more_fns) = truncate_list(&fn_names, FN_TRUNCATE_THRESHOLD, FN_TRUNCATE_SHOW);
            all_names.extend(shown_fns);
            if more_fns > 0 {
                all_names.push(format!("... +{more_fns} more fns (use xray for full list)"));
            }

            if all_names.is_empty() {
                out.push_str(&format!("  {filename} ({} lines){tags_str}\n", fr.lines));
            } else {
                out.push_str(&format!("  {filename} ({} lines){tags_str} - {}\n", fr.lines, all_names.join(", ")));
            }
        }
    }
    out
}
```

- [ ] **Step 8: Run all codemap tests**

Run: `cargo test --lib -- codemap`
Expected: All pass

- [ ] **Step 9: Commit**

```bash
git add src/codemap.rs
git commit -m "feat: add directory grouping and truncation to radar

Files grouped by directory when >100 files, with name-only API.
Long API lists truncated with xray cue."
```

---

### Task 6: Update hooks

**Files:**
- Modify: `hooks/hooks.json`
- Modify: `hooks/check-read.sh`
- Modify: `hooks/check-glob.sh`
- Modify: `hooks/check-grep.sh`
- Modify: `hooks/check-agent.sh`

- [ ] **Step 1: Update hooks.json SessionStart message**

Replace the echo command in hooks.json with the decision tree from the spec:

```
echo 'Structural code intelligence available (taoki plugin):
- Exploring a new codebase? → radar (no args) for tagged repo overview
- Understanding a specific file? → xray (structural skeleton with line numbers, 70-90% fewer tokens than reading)
- About to modify a file? → ripple (what depends on it, with depth for blast radius)
Always call radar first when orienting in an unfamiliar repo, then xray on files of interest.'
```

- [ ] **Step 2: Update check-read.sh**

Replace the additionalContext in the source file match case with:

```
Consider calling mcp__taoki__xray on this file first to get its structure with line numbers, then Read only the sections you need. If you're about to modify this file, mcp__taoki__ripple shows what depends on it.
```

- [ ] **Step 3: Update check-glob.sh**

Replace the additionalContext with:

```
If you're exploring project structure (not searching for a specific file), mcp__taoki__radar gives a tagged overview with public APIs — one call instead of glob + multiple reads.
```

- [ ] **Step 4: Update check-grep.sh**

Replace the additionalContext with:

```
For structural questions (what functions does this file export? what's the class hierarchy?), mcp__taoki__xray or radar are more precise than text search. For literal string lookups, Grep is the right tool.
```

- [ ] **Step 5: Update check-agent.sh**

Replace the additionalContext with:

```
This subagent has access to Taoki MCP tools for code intelligence. If it will explore or modify code, include in its prompt: 'You have MCP tools for code intelligence: mcp__taoki__radar (repo overview with tags), mcp__taoki__xray (single file skeleton), mcp__taoki__ripple (import/export graph with depth). Call radar first when exploring a codebase, then xray on files of interest.'
```

- [ ] **Step 6: Commit**

```bash
git add hooks/
git commit -m "feat: update all hooks with radar/xray/ripple tool names

SessionStart uses decision tree format.
Each PreToolUse hook guides toward the right tool for the context."
```

---

### Task 7: Update commands, skills, plugin.json

**Files:**
- Rename+modify: `commands/taoki-map.md` → `commands/taoki-radar.md`
- Rename+modify: `commands/taoki-index.md` → `commands/taoki-xray.md`
- Rename+modify: `commands/taoki-deps.md` → `commands/taoki-ripple.md`
- Modify: `skills/taoki-workflow.md`
- Modify: `.claude-plugin/plugin.json`

- [ ] **Step 1: Rename and update command files**

```bash
git mv commands/taoki-map.md commands/taoki-radar.md
git mv commands/taoki-index.md commands/taoki-xray.md
git mv commands/taoki-deps.md commands/taoki-ripple.md
```

Update `commands/taoki-radar.md`:
```markdown
---
allowed-tools: mcp__taoki__radar
description: Sweep the repository with radar for a structural overview
---

Call the `mcp__taoki__radar` tool to sweep this repository for a structural overview.

If arguments are provided, use them as glob patterns. Otherwise, default to all supported file types.

After receiving the radar sweep, provide a concise summary of the repository's architecture:
- Key modules and their responsibilities
- Main types and how they relate
- Entry points and public API surface
```

Update `commands/taoki-xray.md`:
```markdown
---
allowed-tools: mcp__taoki__xray
description: Xray a source file to see its structural bones
---

Call the `mcp__taoki__xray` tool on the specified file path.

After receiving the xray, present the file structure and highlight:
- The main types and their purpose
- Key functions and what they do
- Notable patterns (traits, impls, test coverage)
```

Update `commands/taoki-ripple.md`:
```markdown
---
allowed-tools: mcp__taoki__ripple
description: Trace the ripple effect — what depends on a file and what it depends on
---

Call the `mcp__taoki__ripple` tool on the specified file path.

After receiving the ripple analysis, present:
- Files this file depends on (imports) with symbols
- Files that depend on this file (used_by / reverse dependencies)
- External packages used
- Impact assessment: how many files would be affected by changes
```

- [ ] **Step 2: Update skills/taoki-workflow.md**

Rewrite with new tool names, remove batch skeleton references, update the table:

```markdown
---
name: taoki-workflow
description: "Use when exploring a codebase, understanding code architecture, reviewing code, implementing features, fixing bugs, or before reading source files. Provides structural code intelligence: radar for repo overview with tags, xray for file skeletons with line numbers, ripple for import/export graphs with depth. Saves 70-90% tokens vs reading full files. Use this BEFORE Read, Glob, or Grep on source files."
allowed-tools: mcp__taoki__radar, mcp__taoki__xray, mcp__taoki__ripple
---

You have access to three structural code intelligence tools. Use them in this order:

## Workflow

### 1. RADAR — Sweep the repository

Call `mcp__taoki__radar` with the repository root path. This returns one line per file with:
- Line count
- **[tags]** like `[entry-point]`, `[tests]`, `[data-models]`, `[interfaces]`, `[error-types]`, `[module-root]`
- Public types and function names

Results are cached on disk (blake3 hash). Cached calls are near-instant. **Always call this first.**

Use the tags to narrow which files matter for your task:
- Fixing a bug? Look for `[error-types]` and related `[tests]` files
- Adding a feature? Look for `[interfaces]` and `[data-models]`
- Understanding entry points? Look for `[entry-point]`

### 2. RIPPLE — Check the blast radius

Call `mcp__taoki__ripple` with the file you plan to modify and the repo root. This shows:
- **depends_on:** files this file imports with symbols
- **used_by:** files that import this file (what will be affected by changes)
- **external:** third-party dependencies

Use `depth=2` or `depth=3` to see transitive impact. **Call this on every file you plan to modify.**

### 3. XRAY — See inside a file

Call `mcp__taoki__xray` on the file. This returns the structural skeleton:
- Imports, types, function signatures with body insights — all with line numbers
- 70-90% fewer tokens than reading the full file

Results are cached on disk — repeated calls on unchanged files are instant. **Never Read a source file without xraying it first.**

### 4. READ — Targeted reading

Use the `Read` tool with `offset` and `limit` parameters to read only the specific functions or sections identified by the xray. Don't read entire files when you only need a few functions.

### 5. PLAN + IMPLEMENT

With full structural understanding and dependency context, plan your changes and implement them.

## When NOT to use these tools

- For non-code files (config, markdown, JSON) — use Read directly
- When searching for a specific string — use Grep
- When you already know exactly which file and line to edit — skip to Read/Edit

## Tool reference

| Tool | Purpose | When |
|------|---------|------|
| `mcp__taoki__radar` | Repo overview with file tags | First, always |
| `mcp__taoki__ripple` | Impact analysis with depth | Before modifying any file |
| `mcp__taoki__xray` | Single file skeleton with line numbers | Before reading any source file |
```

- [ ] **Step 3: Update plugin.json keywords**

In `.claude-plugin/plugin.json`, update keywords:
```json
"keywords": ["radar", "xray", "ripple", "tree-sitter", "code-intelligence"]
```

- [ ] **Step 4: Commit**

```bash
git add commands/ skills/ .claude-plugin/plugin.json
git commit -m "feat: update commands, skills, and plugin.json with new tool names

Rename command files: taoki-radar, taoki-xray, taoki-ripple.
Rewrite workflow skill for the new tool separation."
```

---

### Task 8: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update all tool references throughout CLAUDE.md**

Apply all changes listed in the spec's Post-Implementation section:
- Tool renames throughout: `code_map` → `radar`, `index` → `xray`, `dependencies` → `ripple`
- `radar` no longer has `files` parameter
- `radar` output format: directory grouping for >100 files, `GROUPING_THRESHOLD` constant
- `FN_TRUNCATE_THRESHOLD` and `TYPE_TRUNCATE_THRESHOLD` constants
- `radar` now uses `extract_public_api()` instead of `extract_all()`
- `CacheEntry` no longer has `skeleton` field, `CACHE_VERSION` reset to 1
- Cache files: `radar.json`, new `xray.json`
- `find_repo_root()` helper in `mcp.rs`
- `query_deps` signature with `depth` parameter
- Updated hook descriptions
- Updated tool descriptions

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No warnings (except the known `framing` warning in main.rs)

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for radar/xray/ripple overhaul

Complete documentation update reflecting new tool names, removed
batch skeletons, xray disk cache, ripple depth/symbols, radar
directory grouping/truncation, and updated hooks."
```

---

### Task 9: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All 145+ tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: Clean (only known framing warning)

- [ ] **Step 3: Run benchmark on taoki itself**

Run: `cargo run --bin benchmark --features benchmark`
Expected: All projects pass (this validates the radar changes don't break the real-world benchmark)

- [ ] **Step 4: Manual smoke test**

Start the MCP server and verify each tool responds:
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | cargo run --quiet 2>/dev/null | head -1
```
Verify output contains `"radar"`, `"xray"`, `"ripple"`.

- [ ] **Step 5: Tag release**

```bash
git tag v1.0.0
```
