# Code Map Skeleton Cache Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cache full structural skeletons in code-map.json and add a `files` parameter to `code_map` that returns them inline, collapsing N+1 tool calls into 2.

**Architecture:** Extend the existing code-map cache with a `skeleton` field per file. Introduce `extract_all()` in `index/mod.rs` to parse once and return both public API and skeleton. Add `files` parameter to `code_map` tool and update hooks to teach the two-call pattern.

**Tech Stack:** Rust, tree-sitter, serde, blake3, fs2

**Spec:** `docs/superpowers/specs/2026-03-16-code-map-skeleton-cache-design.md`

---

## Chunk 1: Core infrastructure

### Task 1: Add `extract_all()` to index module

**Files:**
- Modify: `src/index/mod.rs:410-462`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` block in `src/index/mod.rs`:

```rust
#[test]
fn extract_all_returns_api_and_skeleton() {
    let src = "pub struct Foo {}\npub fn bar() {}\nfn private() {}\n";
    let (api, skeleton) = extract_all(src.as_bytes(), Language::Rust).unwrap();
    assert!(api.types.contains(&"Foo".to_string()));
    assert!(api.functions.iter().any(|f| f.starts_with("bar(")));
    assert!(!api.functions.iter().any(|f| f.starts_with("private(")));
    assert!(skeleton.contains("types:"));
    assert!(skeleton.contains("Foo"));
    assert!(skeleton.contains("fns:"));
    assert!(skeleton.contains("bar()"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test extract_all_returns_api_and_skeleton -- --nocapture`
Expected: FAIL — `extract_all` doesn't exist yet.

- [ ] **Step 3: Implement `extract_all()`**

Add this public function in `src/index/mod.rs`, right before `extract_public_api()` (before line 451). It parses once and returns both the public API and the skeleton string:

```rust
pub fn extract_all(source: &[u8], lang: Language) -> Result<(PublicApi, String), IndexError> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .map_err(|_| IndexError::ParseFailed)?;

    let tree = parser.parse(source, None).ok_or(IndexError::ParseFailed)?;
    let root = tree.root_node();
    let extractor = lang.extractor();

    // Extract public API
    let api = extractor.extract_public_api(root, source);

    // Extract skeleton
    let module_doc = detect_module_doc(root, source, extractor);
    let mut entries = Vec::new();
    let mut test_lines: Vec<usize> = Vec::new();

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if extractor.is_attr(child) || extractor.is_doc_comment(child, source) {
            continue;
        }
        let attrs = extractor.collect_preceding_attrs(child);
        if extractor.is_test_node(child, source, &attrs) {
            test_lines.push(child.start_position().row + 1);
            continue;
        }
        for (i, mut entry) in extractor
            .extract_nodes(child, source, &attrs)
            .into_iter()
            .enumerate()
        {
            if i == 0 {
                if let Some(doc_start) = doc_comment_start_line(child, source, extractor) {
                    entry.line_start = entry.line_start.min(doc_start);
                }
            }
            entries.push(entry);
        }
    }

    let skeleton = format_skeleton(&entries, &test_lines, module_doc);
    Ok((api, skeleton))
}
```

Also make `PublicApi` public (change `pub(crate)` to `pub` on the struct and its fields at line 208-211):

```rust
pub struct PublicApi {
    pub types: Vec<String>,
    pub functions: Vec<String>,
}
```

Then refactor `index_source` (line 410-448) to delegate to `extract_all`, eliminating duplication:

```rust
pub fn index_source(source: &[u8], lang: Language) -> Result<String, IndexError> {
    let (_api, skeleton) = extract_all(source, lang)?;
    Ok(skeleton)
}
```

And refactor `extract_public_api` (line 451-462) similarly:

```rust
pub fn extract_public_api(source: &[u8], lang: Language) -> Result<(Vec<String>, Vec<String>), IndexError> {
    let (api, _skeleton) = extract_all(source, lang)?;
    Ok((api.types, api.functions))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test extract_all_returns_api_and_skeleton -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All tests pass. No regressions.

- [ ] **Step 6: Commit**

```bash
git add src/index/mod.rs
git commit -m "feat: add extract_all() for single-pass public API + skeleton extraction"
```

---

### Task 2: Expand cache schema and refactor FileResult

**Files:**
- Modify: `src/codemap.rs:19-33` (CacheEntry)
- Modify: `src/codemap.rs:53` (CACHE_VERSION)
- Modify: `src/codemap.rs:59-60` (FileResult)

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)]` in `src/codemap.rs`:

```rust
#[test]
fn cache_stores_skeleton() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "pub struct Foo {}\npub fn bar() {}\n").unwrap();

    // First call: parses and caches
    let result = build_code_map(dir.path(), &[], &[]).unwrap();
    assert!(!result.contains("[skeleton]"));

    // Verify cache contains skeleton
    let cache_path = dir.path().join(".cache/taoki/code-map.json");
    let cache_data: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cache_path).unwrap()).unwrap();
    let skeleton = cache_data["files"]["lib.rs"]["skeleton"].as_str().unwrap();
    assert!(skeleton.contains("types:"));
    assert!(skeleton.contains("Foo"));
    assert!(skeleton.contains("fns:"));
    assert!(skeleton.contains("bar()"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test cache_stores_skeleton -- --nocapture`
Expected: FAIL — `build_code_map` doesn't accept 3 args yet, and cache has no `skeleton` field.

- [ ] **Step 3: Update CacheEntry, CACHE_VERSION, and FileResult**

In `src/codemap.rs`, make these changes:

Update `CacheEntry` (line 25-33):

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
    skeleton: String,
}
```

Update `CACHE_VERSION` (line 53):

```rust
const CACHE_VERSION: u32 = 3;
```

Replace the `FileResult` type alias (line 59-60) with a struct:

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

- [ ] **Step 4: Update `build_code_map` signature and internals**

Update the signature (line 396):

```rust
pub fn build_code_map(root: &Path, globs: &[String], detail_files: &[String]) -> Result<String, CodeMapError> {
```

Normalize `detail_files` at the top of the function:

```rust
let detail_set: std::collections::HashSet<String> = detail_files
    .iter()
    .map(|f| f.strip_prefix("./").unwrap_or(f).to_string())
    .collect();
```

Update the cache-hit branch (around line 425-443) to include skeleton:

```rust
if let Some(cached) = cache.files.get(&rel) {
    if cached.hash == hash {
        results.push(FileResult {
            path: rel.clone(),
            lines: cached.lines,
            public_types: cached.public_types.clone(),
            public_functions: cached.public_functions.clone(),
            tags: cached.tags.clone(),
            parse_error: false,
            skeleton: cached.skeleton.clone(),
        });
        new_files.insert(rel, CacheEntry {
            hash,
            lines: cached.lines,
            public_types: cached.public_types.clone(),
            public_functions: cached.public_functions.clone(),
            tags: cached.tags.clone(),
            skeleton: cached.skeleton.clone(),
        });
        continue;
    }
}
```

Update the parse branch (around line 446-481) to use `extract_all` instead of `extract_public_api`, and handle test files. Replace the `extract_public_api` call and surrounding code:

```rust
let source = match std::fs::read(file_path) {
    Ok(s) => s,
    Err(_) => continue,
};

let lines = source.iter().filter(|&&b| b == b'\n').count() + 1;

// Check if test file — collapse skeleton but still extract public API
let is_test = crate::mcp::is_test_filename(file_path);

let (public_types, public_functions, skeleton) =
    match index::extract_all(&source, lang) {
        Ok((api, skel)) => {
            let final_skeleton = if is_test {
                format!("tests: [1-{}]\n", lines)
            } else {
                skel
            };
            (api.types, api.functions, final_skeleton)
        }
        Err(_) => {
            results.push(FileResult {
                path: rel.clone(),
                lines,
                public_types: Vec::new(),
                public_functions: Vec::new(),
                tags: Vec::new(),
                parse_error: true,
                skeleton: String::new(),
            });
            continue;
        }
    };

let tags = compute_tags(&rel, &public_types, &public_functions, &source);

new_files.insert(
    rel.clone(),
    CacheEntry {
        hash,
        lines,
        public_types: public_types.clone(),
        public_functions: public_functions.clone(),
        tags: tags.clone(),
        skeleton: skeleton.clone(),
    },
);

results.push(FileResult {
    path: rel,
    lines,
    public_types,
    public_functions,
    tags,
    parse_error: false,
    skeleton,
});
```

Update the results sort (line 495) to use struct field instead of tuple index:

```rust
results.sort_by(|a, b| a.path.cmp(&b.path));
```

Update the output formatting loop (around line 498-531) to use struct fields and append skeletons. Replace the loop:

```rust
for fr in &results {
    if fr.parse_error {
        out.push_str(&format!("- {} ({} lines) (parse error)\n", fr.path, fr.lines));
        continue;
    }
    let tags_str = if fr.tags.is_empty() {
        String::new()
    } else {
        format!(" {}", fr.tags.iter().map(|t| format!("[{t}]")).collect::<Vec<_>>().join(" "))
    };
    let types_str = if fr.public_types.is_empty() {
        "(none)".to_string()
    } else {
        fr.public_types.join(", ")
    };
    let fns_str = if fr.public_functions.is_empty() {
        "(none)".to_string()
    } else {
        fr.public_functions.join(", ")
    };
    out.push_str(&format!(
        "- {} ({} lines){tags_str} - public_types: {types_str} - public_functions: {fns_str}\n",
        fr.path, fr.lines
    ));
    if let Some(enrich_entry) = enrichments.get(&fr.path) {
        if let Some(cache_entry) = cache.files.get(&fr.path) {
            if enrich_entry.hash == cache_entry.hash {
                out.push_str(&format!(
                    "  [enriched] {}\n",
                    enrich_entry.enrichment.replace('\n', " ")
                ));
            }
        }
    }
    if detail_set.contains(&fr.path) && !fr.skeleton.is_empty() {
        out.push_str("  [skeleton]\n");
        for line in fr.skeleton.lines() {
            out.push_str(&format!("  {line}\n"));
        }
    }
}
```

Also make `is_test_filename` public in `src/mcp.rs` (change `fn is_test_filename` to `pub fn is_test_filename` at line 404).

- [ ] **Step 5: Fix all callers of `build_code_map`**

In `src/mcp.rs`, update `call_code_map` (around line 419-457). The call at line 441 needs the third argument:

```rust
match codemap::build_code_map(std::path::Path::new(path), &globs, &[]) {
```

(The `files` parameter will be wired in Task 3.)

Also update **all existing test calls** in `src/codemap.rs` tests module. Every call to `build_code_map(dir.path(), &[])` must become `build_code_map(dir.path(), &[], &[])`. Search for `build_code_map(` in the tests module and add `&[]` as the third argument to each call.

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test cache_stores_skeleton -- --nocapture`
Expected: PASS

- [ ] **Step 7: Run full test suite and clippy**

Run: `cargo test && cargo clippy`
Expected: All tests pass, no clippy warnings.

- [ ] **Step 8: Commit**

```bash
git add src/codemap.rs src/index/mod.rs src/mcp.rs
git commit -m "feat: cache full skeletons in code-map.json (v3), refactor FileResult to struct"
```

---

## Chunk 2: MCP integration and output

### Task 3: Wire `files` parameter in MCP layer

**Files:**
- Modify: `src/mcp.rs:80-136` (tool_definitions)
- Modify: `src/mcp.rs:419-457` (call_code_map)

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)]` in `src/codemap.rs`:

```rust
#[test]
fn code_map_with_files_includes_skeleton() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub struct Foo {}\npub fn bar() {}\n").unwrap();
    fs::write(src_dir.join("main.rs"), "fn main() {}\n").unwrap();

    let result = build_code_map(dir.path(), &[], &["src/lib.rs".to_string()]).unwrap();

    // lib.rs should have skeleton
    assert!(result.contains("[skeleton]"));
    assert!(result.contains("Foo"));
    assert!(result.contains("bar()"));

    // main.rs should NOT have skeleton (not in files list)
    // Find the main.rs line and check no skeleton follows
    let lines: Vec<&str> = result.lines().collect();
    let main_idx = lines.iter().position(|l| l.contains("main.rs")).unwrap();
    if main_idx + 1 < lines.len() {
        assert!(!lines[main_idx + 1].contains("[skeleton]"),
            "main.rs should not have skeleton");
    }
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test code_map_with_files_includes_skeleton -- --nocapture`
Expected: PASS — this exercises functionality completed in Task 2.

- [ ] **Step 3: Add test for path normalization**

```rust
#[test]
fn code_map_files_normalizes_dot_slash_prefix() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();

    let result = build_code_map(dir.path(), &[], &["./lib.rs".to_string()]).unwrap();
    assert!(result.contains("[skeleton]"), "should match with ./ prefix:\n{result}");
}
```

- [ ] **Step 4: Run normalization test**

Run: `cargo test code_map_files_normalizes_dot_slash_prefix -- --nocapture`
Expected: PASS

- [ ] **Step 5: Add test for enriched-before-skeleton ordering**

```rust
#[test]
fn code_map_enriched_before_skeleton() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "pub fn hello() {}\n").unwrap();

    // Initialize git repo
    std::process::Command::new("git").args(["init"]).current_dir(dir.path()).output().unwrap();

    // Get file hash
    let source = fs::read(&file).unwrap();
    let hash = blake3::hash(&source).to_hex().to_string();

    // Write enrichment cache
    let root_hash = blake3::hash(
        dir.path().canonicalize().unwrap().to_string_lossy().as_bytes(),
    ).to_hex().to_string();
    let cache_dir = dir.path().join(".cache/taoki");
    fs::create_dir_all(&cache_dir).unwrap();
    let enrichment = serde_json::json!({
        "version": 1,
        "model": "haiku",
        "repo_root_hash": root_hash,
        "files": {
            "lib.rs": {
                "hash": hash,
                "enrichment": "Test enrichment."
            }
        }
    });
    fs::write(cache_dir.join("enriched.json"), serde_json::to_string(&enrichment).unwrap()).unwrap();

    let result = build_code_map(dir.path(), &[], &["lib.rs".to_string()]).unwrap();
    let enriched_pos = result.find("[enriched]").expect("should have enrichment");
    let skeleton_pos = result.find("[skeleton]").expect("should have skeleton");
    assert!(enriched_pos < skeleton_pos, "enriched should appear before skeleton:\n{result}");
}
```

- [ ] **Step 6: Run ordering test**

Run: `cargo test code_map_enriched_before_skeleton -- --nocapture`
Expected: PASS

- [ ] **Step 7: Update tool definitions in `mcp.rs`**

In `src/mcp.rs`, update `tool_definitions()` (line 80-136).

Update the `code_map` tool entry:

```rust
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

Update the `index` tool description:

```
"description": "Return a compact structural skeleton of a source file: imports, type definitions, function signatures, and their line numbers. ~70-90% fewer tokens than reading the full file. Use this to understand a file's architecture before reading specific sections with the Read tool. For multiple files, prefer code_map with the files parameter instead. Supports: Rust, Python, TypeScript, JavaScript, Go, Java."
```

- [ ] **Step 8: Wire `files` parameter in `call_code_map`**

In `src/mcp.rs`, update `call_code_map` to parse the `files` parameter and pass it through:

```rust
fn call_code_map(args: &Value) -> ToolResult {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: "missing required parameter: path".to_string(),
            }],
            is_error: true,
        };
    }

    let globs: Vec<String> = args
        .get("globs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let detail_files: Vec<String> = args
        .get("files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    match codemap::build_code_map(std::path::Path::new(path), &globs, &detail_files) {
        Ok(map) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: map,
            }],
            is_error: false,
        },
        Err(e) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: e.to_string(),
            }],
            is_error: true,
        },
    }
}
```

- [ ] **Step 9: Run full test suite and clippy**

Run: `cargo test && cargo clippy`
Expected: All tests pass, no clippy warnings.

- [ ] **Step 10: Commit**

```bash
git add src/mcp.rs src/codemap.rs
git commit -m "feat: wire files parameter for code_map detailed mode"
```

---

### Task 4: Add remaining edge case tests

**Files:**
- Modify: `src/codemap.rs` (tests module)

- [ ] **Step 1: Test backward compatibility (no files param)**

```rust
#[test]
fn code_map_without_files_has_no_skeleton() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();

    let result = build_code_map(dir.path(), &[], &[]).unwrap();
    assert!(!result.contains("[skeleton]"), "should have no skeleton without files param:\n{result}");
}
```

- [ ] **Step 2: Test nonexistent file in files param**

```rust
#[test]
fn code_map_files_ignores_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();

    let result = build_code_map(dir.path(), &[], &["nonexistent.rs".to_string()]).unwrap();
    assert!(!result.contains("[skeleton]"), "should have no skeleton for nonexistent file:\n{result}");
}
```

- [ ] **Step 3: Test skeleton indentation**

```rust
#[test]
fn code_map_skeleton_lines_indented() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();

    let result = build_code_map(dir.path(), &[], &["lib.rs".to_string()]).unwrap();
    let in_skeleton = result.lines()
        .skip_while(|l| !l.contains("[skeleton]"))
        .skip(1) // skip the [skeleton] line itself
        .take_while(|l| !l.is_empty() && !l.starts_with("- "))
        .collect::<Vec<_>>();
    assert!(!in_skeleton.is_empty(), "should have skeleton lines");
    for line in &in_skeleton {
        assert!(line.starts_with("  "), "skeleton line should be indented: {line:?}");
    }
}
```

- [ ] **Step 4: Test test-file skeleton is collapsed**

```rust
#[test]
fn code_map_test_file_skeleton_collapsed() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("test_auth.py"), "def test_login():\n    assert True\n\ndef test_logout():\n    pass\n").unwrap();

    let result = build_code_map(dir.path(), &[], &["test_auth.py".to_string()]).unwrap();
    assert!(result.contains("[skeleton]"), "test file should still get skeleton block");
    assert!(result.contains("tests:"), "skeleton should be collapsed tests:");
    assert!(!result.contains("test_login"), "individual test names should not appear");
}
```

- [ ] **Step 5: Test cache version migration**

```rust
#[test]
fn cache_v2_triggers_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();

    // Write a v2 cache (old format, no skeleton)
    let cache_dir = dir.path().join(".cache/taoki");
    fs::create_dir_all(&cache_dir).unwrap();
    let old_cache = serde_json::json!({
        "version": 2,
        "files": {
            "lib.rs": {
                "hash": "oldhash",
                "lines": 1,
                "public_types": [],
                "public_functions": ["hello()"],
                "tags": []
            }
        }
    });
    fs::write(cache_dir.join("code-map.json"), serde_json::to_string(&old_cache).unwrap()).unwrap();

    // build_code_map should rebuild (version mismatch)
    let result = build_code_map(dir.path(), &[], &["lib.rs".to_string()]).unwrap();
    assert!(result.contains("[skeleton]"), "should rebuild cache and produce skeleton:\n{result}");

    // Verify cache is now v3
    let cache_data: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(cache_dir.join("code-map.json")).unwrap()).unwrap();
    assert_eq!(cache_data["version"], 3);
}
```

- [ ] **Step 6: Test cached skeleton matches fresh index_source output**

```rust
#[test]
fn cached_skeleton_matches_index_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = "pub struct Foo {}\npub fn bar() {}\nfn private() {}\n";
    fs::write(dir.path().join("lib.rs"), source).unwrap();

    // Build code map to populate cache
    build_code_map(dir.path(), &[], &[]).unwrap();

    // Get skeleton from cache via code_map with files
    let result = build_code_map(dir.path(), &[], &["lib.rs".to_string()]).unwrap();
    let skeleton_start = result.find("[skeleton]").unwrap();
    let skeleton_block: String = result[skeleton_start + "[skeleton]\n".len()..]
        .lines()
        .take_while(|l| !l.starts_with("- "))
        .map(|l| l.strip_prefix("  ").unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n");

    // Get skeleton from index_source directly
    let fresh = crate::index::index_source(source.as_bytes(), crate::index::Language::Rust).unwrap();

    assert_eq!(skeleton_block.trim(), fresh.trim(),
        "cached skeleton should match fresh index_source output");
}
```

- [ ] **Step 7: Test parse error produces no skeleton block**

```rust
#[test]
fn code_map_parse_error_no_skeleton() {
    let dir = tempfile::tempdir().unwrap();
    // Write a file with a supported extension but invalid syntax
    fs::write(dir.path().join("broken.rs"), "this is not valid rust {{{{").unwrap();

    let result = build_code_map(dir.path(), &[], &["broken.rs".to_string()]).unwrap();
    assert!(result.contains("(parse error)"), "should show parse error");
    assert!(!result.contains("[skeleton]"), "should have no skeleton for parse error:\n{result}");
}
```

- [ ] **Step 8: Test files param respects globs filter**

```rust
#[test]
fn code_map_files_respects_globs_filter() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    let lib_dir = dir.path().join("lib");
    fs::create_dir(&src_dir).unwrap();
    fs::create_dir(&lib_dir).unwrap();
    fs::write(src_dir.join("main.rs"), "pub fn main() {}\n").unwrap();
    fs::write(lib_dir.join("helper.rs"), "pub fn help() {}\n").unwrap();

    // Glob filters to src/ only, but files asks for lib/helper.rs
    let result = build_code_map(
        dir.path(),
        &["src/**/*.rs".to_string()],
        &["lib/helper.rs".to_string()],
    ).unwrap();

    // helper.rs should not appear at all (filtered by glob)
    assert!(!result.contains("helper.rs"), "helper.rs filtered by glob should not appear:\n{result}");
    assert!(!result.contains("[skeleton]"), "no skeleton for glob-filtered file:\n{result}");
}
```

- [ ] **Step 9: Run all new tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/codemap.rs
git commit -m "test: add edge case tests for code_map skeleton cache"
```

---

## Chunk 3: Hooks and documentation

### Task 5: Update hooks

**Files:**
- Modify: `hooks/hooks.json`
- Modify: `hooks/check-read.sh`

- [ ] **Step 1: Update SessionStart hook**

In `hooks/hooks.json`, replace the first SessionStart hook's command (line 9):

```json
"command": "echo 'You have structural code intelligence tools available via the taoki plugin. Before reading source files, use: mcp__taoki__code_map (repo overview with tags — call with no args first, then pass files: [...] to get full skeletons for files of interest), mcp__taoki__index (single file skeleton with line numbers — 70-90% fewer tokens than reading), mcp__taoki__dependencies (import/export graph for impact analysis). Always call code_map first when exploring a codebase. When you need structure for multiple files, use code_map with the files parameter instead of calling index on each one separately.'"
```

- [ ] **Step 2: Update PreToolUse Read hook**

In `hooks/check-read.sh`, replace the `additionalContext` message in the case branch (line 18-20):

```bash
  rs|py|pyi|ts|tsx|js|jsx|mjs|cjs|go|java)
    cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"Consider calling mcp__taoki__index on this file first to get its structure with line numbers, then Read only the specific sections you need. If you need multiple files, use mcp__taoki__code_map with files: [\"path1\", \"path2\"] to get all skeletons in one call. This typically saves 70-90% of tokens."}}
EOF
    ;;
```

- [ ] **Step 3: Commit**

```bash
git add hooks/hooks.json hooks/check-read.sh
git commit -m "feat: update hooks to teach code_map files parameter pattern"
```

---

### Task 6: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update the Architecture section**

In `CLAUDE.md`, update the `codemap.rs` bullet to mention the skeleton cache:

> - **`codemap.rs`** — `build_code_map()` walks a repo (respecting .gitignore), hashes files with blake3, caches results in `.cache/taoki/code-map.json` with file-level locking (fs2). Calls `index::extract_all` for each file to get both public API and structural skeleton in a single parse pass. Computes heuristic tags per file. Supports optional `files` parameter to include full skeletons inline for specific files. Also triggers dependency graph building via `deps.rs`. Loads and merges LLM enrichment data from `.cache/taoki/enriched.json` when available.

- [ ] **Step 2: Update Key Conventions cache bullet**

Update the cache bullet to mention v3 and skeleton:

> - Cache is stored at `<repo>/.cache/taoki/` (gitignored): `code-map.json` (v3, with tags and skeletons), `deps.json` (dependency graph), and `enriched.json` (LLM-generated semantic summaries).

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with skeleton cache and files parameter"
```

---

### Task 7: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No warnings (except the known `framing` warning in main.rs).

- [ ] **Step 3: Manual smoke test**

Run taoki against its own repo to verify the output looks correct:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"code_map","arguments":{"path":"'$(pwd)'","files":["src/codemap.rs","src/mcp.rs"]}}}' | cargo run 2>/dev/null | tail -1 | python3 -m json.tool
```

Verify:
- `src/codemap.rs` and `src/mcp.rs` have `[skeleton]` blocks
- Other files do NOT have `[skeleton]` blocks
- Skeleton content matches what `index` would return
- `[enriched]` appears before `[skeleton]` if enrichment exists

- [ ] **Step 4: Commit any final fixes if needed**
