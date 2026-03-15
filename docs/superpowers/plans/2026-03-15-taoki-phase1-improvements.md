# Taoki Phase 1 Improvements Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add cross-file dependency graph, heuristic file tags, test detection for all languages, and a unified orchestration skill to make taoki a comprehensive code intelligence backbone for Claude Code.

**Architecture:** Four independent features that build on each other: test detection (foundation) enables the `tests` tag, tags feed into code_map output, the dependency graph adds a new `deps.rs` module and `dependencies` MCP tool, and the orchestration skill ties all three tools into a disciplined workflow. All features are pure tree-sitter + heuristics — no external API calls.

**Tech Stack:** Rust, tree-sitter (0.26), blake3, serde_json, fs2, ignore, globset. All existing dependencies — no new crates needed for Phase 1.

**Spec:** `docs/superpowers/specs/2026-03-15-taoki-improvements-design.md`

---

## Chunk 1: Test Detection for All Languages

### Task 1: Python test detection

**Files:**
- Modify: `src/index/languages/python.rs:160-162` (replace `is_test_node` stub)
- Test in same file (inline `#[cfg(test)]`)

- [ ] **Step 1: Write failing test in `src/index/languages/python.rs`**

Add a `#[cfg(test)]` module at the bottom of the file:

```rust
#[cfg(test)]
mod tests {
    use crate::index::{Language, index_source};

    #[test]
    fn python_test_functions_collapsed() {
        let src = "\
def helper():
    pass

def test_login():
    assert True

class TestAuth:
    def test_token(self):
        pass

def process():
    pass
";
        let out = index_source(src.as_bytes(), Language::Python).unwrap();
        assert!(out.contains("tests:"), "missing tests section in:\n{out}");
        assert!(!out.contains("test_login"), "test_login should be collapsed in:\n{out}");
        assert!(!out.contains("TestAuth"), "TestAuth should be collapsed in:\n{out}");
        assert!(out.contains("helper"), "helper should be visible in:\n{out}");
        assert!(out.contains("process"), "process should be visible in:\n{out}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test index::languages::python::tests::python_test_functions_collapsed -- --nocapture`
Expected: FAIL — tests section not generated, test_login visible in output

- [ ] **Step 3: Implement `is_test_node` for Python**

Replace the `is_test_node` method in `PythonExtractor` (line 160-162):

```rust
fn is_test_node(&self, node: Node, source: &[u8], _attrs: &[Node]) -> bool {
    match node.kind() {
        "function_definition" => {
            node.child_by_field_name("name")
                .map(|n| node_text(n, source).starts_with("test_"))
                .unwrap_or(false)
        }
        "decorated_definition" => {
            // Check if inner is a test function or Test* class
            if let Some(inner) = find_child(node, "function_definition") {
                return inner.child_by_field_name("name")
                    .map(|n| node_text(n, source).starts_with("test_"))
                    .unwrap_or(false);
            }
            if let Some(inner) = find_child(node, "class_definition") {
                return inner.child_by_field_name("name")
                    .map(|n| node_text(n, source).starts_with("Test"))
                    .unwrap_or(false);
            }
            false
        }
        "class_definition" => {
            node.child_by_field_name("name")
                .map(|n| node_text(n, source).starts_with("Test"))
                .unwrap_or(false)
        }
        _ => false,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test index::languages::python::tests::python_test_functions_collapsed -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run all tests + clippy**

Run: `cargo test && cargo clippy`
Expected: All 14+ tests pass, no warnings

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/python.rs
git commit -m "feat: add test detection for Python"
```

---

### Task 2: Go test detection

**Files:**
- Modify: `src/index/languages/go.rs:230-232` (replace `is_test_node` stub)
- Test in same file (inline `#[cfg(test)]`)

- [ ] **Step 1: Write failing test in `src/index/languages/go.rs`**

Add a `#[cfg(test)]` module at the bottom:

```rust
#[cfg(test)]
mod tests {
    use crate::index::{Language, index_source};

    #[test]
    fn go_test_functions_collapsed() {
        let src = r#"
package main

func Helper() {}

func TestLogin(t *testing.T) {
    t.Log("test")
}

func BenchmarkSort(b *testing.B) {
    b.Log("bench")
}

func ExampleFoo() {
    fmt.Println("example")
}

func Process() {}
"#;
        let out = index_source(src.as_bytes(), Language::Go).unwrap();
        assert!(out.contains("tests:"), "missing tests section in:\n{out}");
        assert!(!out.contains("TestLogin"), "TestLogin should be collapsed in:\n{out}");
        assert!(!out.contains("BenchmarkSort"), "BenchmarkSort should be collapsed in:\n{out}");
        assert!(!out.contains("ExampleFoo"), "ExampleFoo should be collapsed in:\n{out}");
        assert!(out.contains("Helper"), "Helper should be visible in:\n{out}");
        assert!(out.contains("Process"), "Process should be visible in:\n{out}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test index::languages::go::tests::go_test_functions_collapsed -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement `is_test_node` for Go**

Replace the `is_test_node` method in `GoExtractor` (line 230-232):

```rust
fn is_test_node(&self, node: Node, source: &[u8], _attrs: &[Node]) -> bool {
    if node.kind() != "function_declaration" {
        return false;
    }
    node.child_by_field_name("name")
        .map(|n| {
            let name = node_text(n, source);
            name.starts_with("Test") || name.starts_with("Benchmark") || name.starts_with("Example")
        })
        .unwrap_or(false)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test index::languages::go::tests::go_test_functions_collapsed -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run all tests + clippy**

Run: `cargo test && cargo clippy`
Expected: All tests pass, no warnings

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/go.rs
git commit -m "feat: add test detection for Go"
```

---

### Task 3: TypeScript/JavaScript test detection

**Files:**
- Modify: `src/index/languages/typescript.rs:215-217` (replace `is_test_node` stub)
- Test in same file (inline `#[cfg(test)]`)

- [ ] **Step 1: Write failing test in `src/index/languages/typescript.rs`**

Add a `#[cfg(test)]` module at the bottom:

```rust
#[cfg(test)]
mod tests {
    use crate::index::{Language, index_source};

    #[test]
    fn ts_test_calls_collapsed() {
        let src = "\
import { expect } from 'vitest';

export function helper(): string { return ''; }

describe('auth', () => {
  it('should login', () => {
    expect(true).toBe(true);
  });
});

test('standalone test', () => {
  expect(1).toBe(1);
});
";
        let out = index_source(src.as_bytes(), Language::TypeScript).unwrap();
        assert!(out.contains("tests:"), "missing tests section in:\n{out}");
        assert!(!out.contains("describe"), "describe should be collapsed in:\n{out}");
        assert!(!out.contains("standalone"), "standalone test should be collapsed in:\n{out}");
        assert!(out.contains("helper"), "helper should be visible in:\n{out}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test index::languages::typescript::tests::ts_test_calls_collapsed -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement `is_test_node` for TypeScript/JavaScript**

Replace the `is_test_node` method in `TsJsExtractor` (line 215-217):

```rust
fn is_test_node(&self, node: Node, source: &[u8], _attrs: &[Node]) -> bool {
    // Match expression_statement nodes containing describe(), it(), test() calls
    if node.kind() != "expression_statement" {
        return false;
    }
    let Some(expr) = node.child(0) else { return false };
    if expr.kind() != "call_expression" {
        return false;
    }
    let Some(func) = expr.child_by_field_name("function") else { return false };
    let name = node_text(func, source);
    matches!(name, "describe" | "it" | "test")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test index::languages::typescript::tests::ts_test_calls_collapsed -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run all tests + clippy**

Run: `cargo test && cargo clippy`
Expected: All tests pass, no warnings

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/typescript.rs
git commit -m "feat: add test detection for TypeScript/JavaScript"
```

---

### Task 4: Filename-based test detection in `call_index`

**Files:**
- Modify: `src/mcp.rs:176-292` (update `call_index` to detect test files by name)

- [ ] **Step 1: Write failing test**

This test must go in `src/mcp.rs` as an inline test since it tests `call_index` behavior:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_file_by_name_collapses_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test_auth.py");
        fs::write(&test_file, "def test_login():\n    assert True\n\ndef test_logout():\n    pass\n").unwrap();

        let args = serde_json::json!({ "path": test_file.to_str().unwrap() });
        let result = call_index(&args);
        assert!(!result.is_error);
        let text = &result.content[0].text;
        assert!(text.contains("tests:"), "should collapse entire file as tests:\n{text}");
        assert!(!text.contains("test_login"), "individual test names should not appear:\n{text}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test mcp::tests::test_file_by_name_collapses_entirely -- --nocapture`
Expected: FAIL — individual test functions shown instead of collapsed

- [ ] **Step 3: Implement filename-based test detection**

Add a helper function before `call_index` in `mcp.rs`:

```rust
fn is_test_filename(path: &std::path::Path) -> bool {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    // Go: *_test.go
    name.ends_with("_test.go")
    // Python: test_*.py, *_test.py
        || (matches!(ext, "py" | "pyi") && (stem.starts_with("test_") || stem.ends_with("_test")))
    // TS/JS: *.test.ts, *.spec.ts, *.test.js, *.spec.js
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
    // Java: *Test.java, *Tests.java
        || (ext == "java" && (stem.ends_with("Test") || stem.ends_with("Tests")))
}
```

Then in `call_index`, after computing the source and before parsing, add:

```rust
// If entire file is a test file by naming convention, collapse it
if is_test_filename(path) {
    let total_lines = source.iter().filter(|&&b| b == b'\n').count() + 1;
    let skeleton = format!("tests: [1-{}]\n", total_lines);
    INDEX_CACHE.with(|cache| {
        cache.borrow_mut().insert(path_buf.clone(), (hash.clone(), skeleton.clone()));
    });
    return ToolResult {
        content: vec![ToolContent {
            r#type: "text".to_string(),
            text: skeleton,
        }],
        is_error: false,
    };
}
```

Insert this block after the cache miss check (after line 270) and before the `index::index_source` call.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test mcp::tests::test_file_by_name_collapses_entirely -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run all tests + clippy**

Run: `cargo test && cargo clippy`
Expected: All tests pass, no warnings

- [ ] **Step 6: Commit**

```bash
git add src/mcp.rs
git commit -m "feat: collapse test files by filename convention in index tool"
```

---

## Chunk 2: Heuristic Tags in code_map

### Task 5: Add tags field to CacheEntry and tag computation

**Files:**
- Modify: `src/codemap.rs` (CacheEntry struct, tag computation, output format)

- [ ] **Step 1: Write failing test**

Add to existing `#[cfg(test)] mod tests` in `codemap.rs`:

```rust
#[test]
fn tags_entry_point() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.rs");
    fs::write(&file, "fn main() {}\n").unwrap();

    let result = build_code_map(dir.path(), &[]).unwrap();
    assert!(result.contains("[entry-point]"), "missing entry-point tag in:\n{result}");
}

#[test]
fn tags_tests_by_filename() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test_auth.py");
    fs::write(&file, "def test_login():\n    pass\n").unwrap();

    let result = build_code_map(dir.path(), &[]).unwrap();
    assert!(result.contains("[tests]"), "missing tests tag in:\n{result}");
}

#[test]
fn tags_module_root() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("pkg");
    fs::create_dir(&sub).unwrap();
    let file = sub.join("mod.rs");
    fs::write(&file, "pub fn foo() {}\n").unwrap();

    let result = build_code_map(dir.path(), &[]).unwrap();
    assert!(result.contains("[module-root]"), "missing module-root tag in:\n{result}");
}

#[test]
fn tags_error_types() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("errors.rs");
    fs::write(&file, "pub enum MyError { Io, Parse }\n").unwrap();

    let result = build_code_map(dir.path(), &[]).unwrap();
    assert!(result.contains("[error-types]"), "missing error-types tag in:\n{result}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test codemap::tests::tags_ -- --nocapture`
Expected: FAIL — no `[tag]` in output

- [ ] **Step 3: Update CacheEntry and add tag computation**

Update the `CacheEntry` struct:

```rust
#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    hash: String,
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}
```

Bump cache version:

```rust
const CACHE_VERSION: u32 = 2;
```

Add tag computation function:

```rust
fn compute_tags(
    filename: &str,
    public_types: &[String],
    public_functions: &[String],
    source: &[u8],
) -> Vec<String> {
    let mut tags = Vec::new();

    // entry-point: has main()
    if public_functions.iter().any(|f| f.starts_with("main("))
        || public_functions.iter().any(|f| f == "main()")
    {
        tags.push("entry-point".to_string());
    }
    // Also check non-public main for languages where main isn't exported
    let source_str = std::str::from_utf8(source).unwrap_or("");
    if tags.is_empty()
        && (source_str.contains("fn main()")
            || source_str.contains("func main()")
            || source_str.contains("def main(")
            || source_str.contains("public static void main("))
    {
        tags.push("entry-point".to_string());
    }

    // tests: filename convention (extension-aware to avoid false positives)
    let fpath = std::path::Path::new(filename);
    let stem = fpath.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = fpath.extension().and_then(|s| s.to_str()).unwrap_or("");
    if filename.ends_with("_test.go")
        || (matches!(ext, "py" | "pyi") && (stem.starts_with("test_") || stem.ends_with("_test")))
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || (ext == "java" && (stem.ends_with("Test") || stem.ends_with("Tests")))
    {
        tags.push("tests".to_string());
    }

    // data-models: only types, no functions
    if !public_types.is_empty() && public_functions.is_empty() {
        tags.push("data-models".to_string());
    }

    // interfaces: types contain trait/interface indicators (heuristic from type names)
    // This is approximate — we check if the file defines traits/interfaces by looking
    // at the source for trait/interface keywords
    if (source_str.contains("pub trait ") || source_str.contains("export interface ")
        || source_str.contains("public interface "))
        && !source_str.contains("impl ")
    {
        tags.push("interfaces".to_string());
    }

    // error-types: types with Error/Exception in name
    if public_types.iter().any(|t| t.contains("Error") || t.contains("Exception")) {
        tags.push("error-types".to_string());
    }

    // http-handlers: heuristic pattern matching on source
    if source_str.contains("@GetMapping") || source_str.contains("@PostMapping")
        || source_str.contains("@RequestMapping") || source_str.contains("@Path")
        || source_str.contains("@app.route") || source_str.contains("@router.")
        || source_str.contains("http.ResponseWriter") || source_str.contains("*http.Request")
        || source_str.contains("#[get(") || source_str.contains("#[post(")
        || source_str.contains("#[put(") || source_str.contains("#[delete(")
    {
        tags.push("http-handlers".to_string());
    }

    // barrel-file: mostly re-exports
    if source_str.contains("pub use ") || source_str.contains("pub mod ")
        || source_str.contains("export * from") || source_str.contains("export { ")
    {
        // Only tag as barrel if it's predominantly re-exports (more re-exports than definitions)
        let reexport_count = source_str.matches("pub use ").count()
            + source_str.matches("pub mod ").count()
            + source_str.matches("export ").count();
        let fn_count = public_functions.len();
        let type_count = public_types.len();
        if reexport_count > fn_count + type_count && reexport_count >= 3 {
            tags.push("barrel-file".to_string());
        }
    }

    // cli: argument parsing patterns
    if source_str.contains("clap::") || source_str.contains("#[derive(Parser")]
        || source_str.contains("argparse") || source_str.contains("ArgumentParser")
        || source_str.contains("flag.Parse()") || source_str.contains("flag.String(")
    {
        tags.push("cli".to_string());
    }

    // module-root: specific filenames
    if filename.ends_with("mod.rs")
        || filename.ends_with("__init__.py")
        || filename.ends_with("/index.ts")
        || filename.ends_with("/index.js")
        || filename.ends_with("/index.tsx")
        || filename.ends_with("/index.jsx")
    {
        tags.push("module-root".to_string());
    }

    tags
}
```

- [ ] **Step 4: Integrate tag computation into `build_code_map`**

In the parse loop of `build_code_map` (after `extract_public_api`), compute tags and store in CacheEntry. Update the results tuple to include tags.

Change the `results` type:

```rust
let mut results: Vec<(String, usize, Vec<String>, Vec<String>, Vec<String>, bool)> = Vec::new();
```

After parsing (where `public_types` and `public_functions` are extracted), add:

```rust
let tags = compute_tags(&rel, &public_types, &public_functions, &source);
```

Update `CacheEntry` insertions to include `tags`:

```rust
new_files.insert(
    rel.clone(),
    CacheEntry {
        hash,
        lines,
        public_types: public_types.clone(),
        public_functions: public_functions.clone(),
        tags: tags.clone(),
    },
);

results.push((rel, lines, public_types, public_functions, tags, false));
```

For cache hits, include cached tags:

```rust
results.push((
    rel.clone(),
    cached.lines,
    cached.public_types.clone(),
    cached.public_functions.clone(),
    cached.tags.clone(),
    false,
));
new_files.insert(rel, CacheEntry {
    hash,
    lines: cached.lines,
    public_types: cached.public_types.clone(),
    public_functions: cached.public_functions.clone(),
    tags: cached.tags.clone(),
});
```

- [ ] **Step 5: Update output formatting to include tags**

In the output formatting loop at the end of `build_code_map`, add tag display:

```rust
for (path, lines, types, fns, tags, parse_error) in &results {
    if *parse_error {
        out.push_str(&format!("- {path} ({lines} lines) (parse error)\n"));
        continue;
    }
    let tags_str = if tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", tags.join(", "))
    };
    let types_str = if types.is_empty() {
        "(none)".to_string()
    } else {
        types.join(", ")
    };
    let fns_str = if fns.is_empty() {
        "(none)".to_string()
    } else {
        fns.join(", ")
    };
    out.push_str(&format!(
        "- {path} ({lines} lines){tags_str} - public_types: {types_str} - public_functions: {fns_str}\n"
    ));
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test codemap::tests -- --nocapture`
Expected: All codemap tests pass (old and new)

- [ ] **Step 7: Run all tests + clippy**

Run: `cargo test && cargo clippy`
Expected: All tests pass, no warnings

- [ ] **Step 8: Commit**

```bash
git add src/codemap.rs
git commit -m "feat: add heuristic tags to code_map output"
```

---

## Chunk 3: Dependencies Tool

### Task 6: Create `src/deps.rs` — import extraction and resolution

**Files:**
- Create: `src/deps.rs`
- Modify: `src/main.rs:1` (add `mod deps;`)

- [ ] **Step 0: Make `ts_language()` accessible from `deps.rs`**

In `src/index/mod.rs`, change the visibility of `ts_language` from private to `pub(crate)` (line 48):

```rust
pub(crate) fn ts_language(&self) -> tree_sitter::Language {
```

This allows the `deps` module (a sibling of `index`) to call `lang.ts_language()` for creating parsers.

- [ ] **Step 1: Write the module with tests first**

Create `src/deps.rs` with import extraction, resolution logic, and inline tests:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::index::Language;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportInfo {
    pub path: String,       // resolved relative path, or empty if external
    pub symbols: Vec<String>,
    pub external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileImports {
    pub imports: Vec<ImportInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DepsGraph {
    pub version: u32,
    pub graph: HashMap<String, FileImports>,
}

pub const DEPS_VERSION: u32 = 1;
const DEPS_FILE: &str = "deps.json";
const DEPS_DIR: &str = ".cache/taoki";

impl DepsGraph {
    pub fn new() -> Self {
        Self {
            version: DEPS_VERSION,
            graph: HashMap::new(),
        }
    }
}

/// Extract raw import strings from source using tree-sitter.
/// Returns Vec<(import_path_or_module, symbols)>.
pub fn extract_imports(source: &[u8], lang: Language) -> Vec<(String, Vec<String>)> {
    use tree_sitter::Parser;

    let mut parser = Parser::new();
    if parser.set_language(&lang.ts_language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let root = tree.root_node();

    match lang {
        Language::Rust => extract_rust_imports(root, source),
        Language::Python => extract_python_imports(root, source),
        Language::TypeScript | Language::JavaScript => extract_ts_imports(root, source),
        Language::Go => extract_go_imports(root, source),
        Language::Java => extract_java_imports(root, source),
    }
}

fn extract_rust_imports(root: tree_sitter::Node, source: &[u8]) -> Vec<(String, Vec<String>)> {
    use crate::index::node_text;
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "use_declaration" {
            let text = node_text(child, source)
                .strip_prefix("use ")
                .unwrap_or(node_text(child, source))
                .trim_end_matches(';')
                .to_string();
            imports.push((text, Vec::new()));
        }
    }
    imports
}

fn extract_python_imports(root: tree_sitter::Node, source: &[u8]) -> Vec<(String, Vec<String>)> {
    use crate::index::node_text;
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "import_statement" => {
                let text = node_text(child, source)
                    .strip_prefix("import ")
                    .unwrap_or("")
                    .trim()
                    .to_string();
                imports.push((text, Vec::new()));
            }
            "import_from_statement" => {
                let text = node_text(child, source);
                // "from foo.bar import X, Y"
                if let Some(rest) = text.strip_prefix("from ") {
                    let parts: Vec<&str> = rest.splitn(2, " import ").collect();
                    let module = parts[0].trim().to_string();
                    let symbols: Vec<String> = if parts.len() > 1 {
                        parts[1].split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
                    } else {
                        Vec::new()
                    };
                    imports.push((module, symbols));
                }
            }
            _ => {}
        }
    }
    imports
}

fn extract_ts_imports(root: tree_sitter::Node, source: &[u8]) -> Vec<(String, Vec<String>)> {
    use crate::index::node_text;
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "import_statement" {
            let text = node_text(child, source);
            // Extract the source path from `import ... from 'path'`
            if let Some(source_node) = child.child_by_field_name("source") {
                let path = node_text(source_node, source)
                    .trim_matches(|c| c == '\'' || c == '"')
                    .to_string();
                // Extract imported symbols
                let mut symbols = Vec::new();
                let mut ic = child.walk();
                for inner in child.children(&mut ic) {
                    if inner.kind() == "import_clause" {
                        let clause_text = node_text(inner, source);
                        // Simple extraction of named imports
                        if let Some(start) = clause_text.find('{') {
                            if let Some(end) = clause_text.find('}') {
                                let names = &clause_text[start+1..end];
                                symbols.extend(
                                    names.split(',')
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty())
                                );
                            }
                        }
                    }
                }
                imports.push((path, symbols));
            } else {
                // Fallback: parse from text
                let cleaned = text
                    .strip_prefix("import ")
                    .unwrap_or(text)
                    .trim_end_matches(';')
                    .to_string();
                imports.push((cleaned, Vec::new()));
            }
        }
    }
    imports
}

fn extract_go_imports(root: tree_sitter::Node, source: &[u8]) -> Vec<(String, Vec<String>)> {
    use crate::index::{node_text, find_child};
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "import_declaration" {
            if let Some(spec_list) = find_child(child, "import_spec_list") {
                let mut sc = spec_list.walk();
                for spec in spec_list.children(&mut sc) {
                    if spec.kind() == "import_spec" {
                        let path = spec.child_by_field_name("path")
                            .map(|n| node_text(n, source).trim_matches('"').to_string())
                            .unwrap_or_default();
                        imports.push((path, Vec::new()));
                    }
                }
            } else if let Some(spec) = find_child(child, "import_spec") {
                let path = spec.child_by_field_name("path")
                    .map(|n| node_text(n, source).trim_matches('"').to_string())
                    .unwrap_or_default();
                imports.push((path, Vec::new()));
            }
        }
    }
    imports
}

fn extract_java_imports(root: tree_sitter::Node, source: &[u8]) -> Vec<(String, Vec<String>)> {
    use crate::index::node_text;
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "import_declaration" {
            let text = node_text(child, source)
                .strip_prefix("import ")
                .unwrap_or("")
                .trim_end_matches(';')
                .trim()
                .to_string();
            // "com.example.Foo" -> module "com.example", symbol "Foo"
            if let Some(dot_pos) = text.rfind('.') {
                let module = text[..dot_pos].to_string();
                let symbol = text[dot_pos+1..].to_string();
                imports.push((module, vec![symbol]));
            } else {
                imports.push((text, Vec::new()));
            }
        }
    }
    imports
}

/// Resolve an import path to a file path relative to repo root.
/// Returns None if the import is external.
pub fn resolve_import(
    import_path: &str,
    lang: Language,
    current_file: &str,
    all_files: &[String],
) -> Option<String> {
    match lang {
        Language::Rust => resolve_rust_import(import_path, all_files),
        Language::Python => resolve_python_import(import_path, current_file, all_files),
        Language::TypeScript | Language::JavaScript => resolve_ts_import(import_path, current_file, all_files),
        Language::Go => None, // Go imports are almost always external packages
        Language::Java => resolve_java_import(import_path, all_files),
    }
}

fn resolve_rust_import(import_path: &str, all_files: &[String]) -> Option<String> {
    // Only resolve crate-local imports
    let local = import_path.strip_prefix("crate::")?;
    // crate::foo::bar -> try src/foo/bar.rs, src/foo/bar/mod.rs, foo/bar.rs, foo/bar/mod.rs
    let parts: Vec<&str> = local.split("::").collect();
    // Try progressively shorter path prefixes (symbols at the end aren't paths)
    for depth in (1..=parts.len()).rev() {
        let path_parts = &parts[..depth];
        let joined = path_parts.join("/");
        let candidates = [
            format!("src/{joined}.rs"),
            format!("src/{joined}/mod.rs"),
            format!("{joined}.rs"),
            format!("{joined}/mod.rs"),
        ];
        for candidate in &candidates {
            if all_files.iter().any(|f| f == candidate) {
                return Some(candidate.clone());
            }
        }
    }
    None
}

fn resolve_python_import(import_path: &str, current_file: &str, all_files: &[String]) -> Option<String> {
    let is_relative = import_path.starts_with('.');
    let clean = import_path.trim_start_matches('.');
    let path = clean.replace('.', "/");

    if is_relative {
        let current_dir = Path::new(current_file).parent()?.to_str()?;
        let candidates = [
            format!("{current_dir}/{path}.py"),
            format!("{current_dir}/{path}/__init__.py"),
        ];
        for c in &candidates {
            if all_files.iter().any(|f| f == c) {
                return Some(c.clone());
            }
        }
    }

    let candidates = [
        format!("{path}.py"),
        format!("{path}/__init__.py"),
    ];
    for c in &candidates {
        if all_files.iter().any(|f| f == c) {
            return Some(c.clone());
        }
    }
    None
}

fn resolve_ts_import(import_path: &str, current_file: &str, all_files: &[String]) -> Option<String> {
    // Only resolve relative imports
    if !import_path.starts_with('.') {
        return None;
    }

    let current_dir = Path::new(current_file).parent()?.to_str()?;
    let resolved = if import_path.starts_with("./") || import_path.starts_with("../") {
        let base = Path::new(current_dir).join(import_path);
        // Normalize (remove ./ and ../)
        let normalized = normalize_path(&base);
        normalized.to_str()?.to_string()
    } else {
        import_path.to_string()
    };

    let extensions = ["ts", "tsx", "js", "jsx", "mjs", "cjs"];
    // Try direct match with extensions
    for ext in &extensions {
        let candidate = format!("{resolved}.{ext}");
        if all_files.iter().any(|f| f == &candidate) {
            return Some(candidate);
        }
    }
    // Try index files
    for ext in &extensions {
        let candidate = format!("{resolved}/index.{ext}");
        if all_files.iter().any(|f| f == &candidate) {
            return Some(candidate);
        }
    }
    None
}

fn resolve_java_import(import_path: &str, all_files: &[String]) -> Option<String> {
    // com.example -> com/example as a directory prefix
    let path = import_path.replace('.', "/");
    // Look for files under that path
    let candidate = format!("{path}.java");
    // Try with src/main/java prefix (Maven convention) and without
    let candidates = [
        candidate.clone(),
        format!("src/main/java/{candidate}"),
        format!("src/{candidate}"),
    ];
    for c in &candidates {
        if all_files.iter().any(|f| f == c) {
            return Some(c.clone());
        }
    }
    None
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => { components.pop(); }
            std::path::Component::CurDir => {}
            _ => { components.push(component); }
        }
    }
    components.iter().collect()
}

/// Build the full dependency graph for a set of files.
pub fn build_deps_graph(
    root: &Path,
    files: &[PathBuf],
) -> DepsGraph {
    let all_rel: Vec<String> = files
        .iter()
        .filter_map(|f| f.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let mut graph = DepsGraph::new();

    for file_path in files {
        let rel = match file_path.strip_prefix(root) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = Language::from_extension(ext) else { continue };
        let Ok(source) = std::fs::read(file_path) else { continue };

        let raw_imports = extract_imports(&source, lang);
        let mut file_imports = FileImports { imports: Vec::new() };

        for (import_path, symbols) in raw_imports {
            if let Some(resolved) = resolve_import(&import_path, lang, &rel, &all_rel) {
                file_imports.imports.push(ImportInfo {
                    path: resolved,
                    symbols,
                    external: false,
                });
            } else {
                // Extract just the top-level module name for external deps
                let module_name = import_path
                    .split(|c| c == ':' || c == '.' || c == '/')
                    .next()
                    .unwrap_or(&import_path)
                    .to_string();
                file_imports.imports.push(ImportInfo {
                    path: module_name,
                    symbols,
                    external: true,
                });
            }
        }

        graph.graph.insert(rel, file_imports);
    }

    graph
}

/// Query the graph: what does this file depend on, and what depends on it?
pub fn query_deps(graph: &DepsGraph, file: &str) -> String {
    let mut out = String::new();

    // depends_on: files this file imports (internal only)
    if let Some(file_imports) = graph.graph.get(file) {
        let internal: Vec<&ImportInfo> = file_imports.imports.iter().filter(|i| !i.external).collect();
        if !internal.is_empty() {
            out.push_str("depends_on:\n");
            for imp in &internal {
                let syms = if imp.symbols.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", imp.symbols.join(", "))
                };
                out.push_str(&format!("  {}{syms}\n", imp.path));
            }
        }
    }

    // used_by: files that import this file
    let mut dependents: Vec<(&str, Vec<&str>)> = Vec::new();
    for (other_file, other_imports) in &graph.graph {
        if other_file == file {
            continue;
        }
        for imp in &other_imports.imports {
            if !imp.external && imp.path == file {
                let syms: Vec<&str> = imp.symbols.iter().map(|s| s.as_str()).collect();
                dependents.push((other_file.as_str(), syms));
            }
        }
    }
    if !dependents.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("used_by:\n");
        dependents.sort_by_key(|(f, _)| *f);
        for (dep_file, syms) in &dependents {
            let sym_str = if syms.is_empty() {
                String::new()
            } else {
                format!(" ({})", syms.join(", "))
            };
            out.push_str(&format!("  {dep_file}{sym_str}\n"));
        }
    }

    // external dependencies
    if let Some(file_imports) = graph.graph.get(file) {
        let external: Vec<&str> = file_imports.imports.iter()
            .filter(|i| i.external)
            .map(|i| i.path.as_str())
            .collect();
        if !external.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            let mut deduped: Vec<&str> = external;
            deduped.sort();
            deduped.dedup();
            out.push_str(&format!("external:\n  {}\n", deduped.join(", ")));
        }
    }

    if out.is_empty() {
        out.push_str("no dependencies found\n");
    }

    out
}

/// Cache management for dependency graph
pub fn deps_cache_path(root: &Path) -> PathBuf {
    root.join(DEPS_DIR).join(DEPS_FILE)
}

pub fn load_deps_cache(root: &Path) -> Option<DepsGraph> {
    let path = deps_cache_path(root);
    let data = std::fs::read_to_string(&path).ok()?;
    let graph: DepsGraph = serde_json::from_str(&data).ok()?;
    if graph.version != DEPS_VERSION {
        return None;
    }
    Some(graph)
}

pub fn save_deps_cache(root: &Path, graph: &DepsGraph) {
    use fs2::FileExt;
    let path = deps_cache_path(root);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let lock_path = path.with_extension("lock");
    let lock_file = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(_) => return,
    };
    if lock_file.lock_exclusive().is_err() {
        return;
    }
    if let Ok(data) = serde_json::to_string_pretty(graph) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    let _ = lock_file.unlock();
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rust_crate_import_resolves() {
        let all_files = vec![
            "src/codemap.rs".to_string(),
            "src/index/mod.rs".to_string(),
            "src/mcp.rs".to_string(),
        ];
        let result = resolve_import("crate::codemap", Language::Rust, "src/mcp.rs", &all_files);
        assert_eq!(result, Some("src/codemap.rs".to_string()));
    }

    #[test]
    fn rust_external_import_unresolved() {
        let all_files = vec!["src/main.rs".to_string()];
        let result = resolve_import("serde::Serialize", Language::Rust, "src/main.rs", &all_files);
        assert_eq!(result, None);
    }

    #[test]
    fn python_relative_import_resolves() {
        let all_files = vec![
            "src/auth.py".to_string(),
            "src/api.py".to_string(),
        ];
        let result = resolve_import(".auth", Language::Python, "src/api.py", &all_files);
        assert_eq!(result, Some("src/auth.py".to_string()));
    }

    #[test]
    fn ts_relative_import_resolves() {
        let all_files = vec![
            "src/utils.ts".to_string(),
            "src/main.ts".to_string(),
        ];
        let result = resolve_import("./utils", Language::TypeScript, "src/main.ts", &all_files);
        assert_eq!(result, Some("src/utils.ts".to_string()));
    }

    #[test]
    fn ts_bare_import_is_external() {
        let all_files = vec!["src/main.ts".to_string()];
        let result = resolve_import("express", Language::TypeScript, "src/main.ts", &all_files);
        assert_eq!(result, None);
    }

    #[test]
    fn build_graph_and_query() {
        let dir = tempfile::tempdir().unwrap();

        // Create a simple Rust project
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("main.rs"), "mod helper;\nuse crate::helper;\nfn main() { helper::run(); }\n").unwrap();
        fs::write(src.join("helper.rs"), "pub fn run() {}\n").unwrap();

        let files = vec![src.join("main.rs"), src.join("helper.rs")];
        let graph = build_deps_graph(dir.path(), &files);

        let main_deps = query_deps(&graph, "src/main.rs");
        assert!(main_deps.contains("depends_on:"), "main should depend on helper:\n{main_deps}");
        assert!(main_deps.contains("src/helper.rs"), "main should depend on helper:\n{main_deps}");

        let helper_deps = query_deps(&graph, "src/helper.rs");
        assert!(helper_deps.contains("used_by:"), "helper should be used by main:\n{helper_deps}");
        assert!(helper_deps.contains("src/main.rs"), "helper should be used by main:\n{helper_deps}");
    }
}
```

- [ ] **Step 2: Register module in `main.rs`**

Add `mod deps;` after line 2 in `src/main.rs`:

```rust
mod codemap;
mod deps;
mod index;
mod mcp;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test deps::tests -- --nocapture`
Expected: All 6 deps tests pass

- [ ] **Step 4: Run all tests + clippy**

Run: `cargo test && cargo clippy`
Expected: All tests pass, no warnings

- [ ] **Step 5: Commit**

```bash
git add src/deps.rs src/main.rs src/index/mod.rs
git commit -m "feat: add dependency graph extraction and resolution module"
```

---

### Task 7: Wire `dependencies` tool into MCP handler

**Files:**
- Modify: `src/mcp.rs` (add tool definition, handler, dispatch)

- [ ] **Step 1: Write failing test**

Add to `#[cfg(test)] mod tests` in `mcp.rs`:

```rust
#[test]
fn dependencies_tool_returns_deps() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("main.rs"), "use crate::helper;\nfn main() { helper::run(); }\n").unwrap();
    fs::write(src.join("helper.rs"), "pub fn run() {}\n").unwrap();

    let args = serde_json::json!({
        "file": src.join("main.rs").to_str().unwrap(),
        "repo_root": dir.path().to_str().unwrap(),
    });
    let result = call_dependencies(&args);
    assert!(!result.is_error, "should not error: {:?}", result.content);
    let text = &result.content[0].text;
    assert!(text.contains("depends_on:") || text.contains("external:"),
        "should show dependencies:\n{text}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test mcp::tests::dependencies_tool_returns_deps -- --nocapture`
Expected: FAIL — `call_dependencies` doesn't exist

- [ ] **Step 3: Add `dependencies` to tool definitions**

In `tool_definitions()` in `mcp.rs`, add a third tool after `code_map`:

```rust
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

- [ ] **Step 4: Add `call_dependencies` handler and dispatch**

Add dispatch in `handle_tools_call`:

```rust
"dependencies" => call_dependencies(&arguments),
```

Add the handler function:

```rust
fn call_dependencies(args: &Value) -> ToolResult {
    let file_str = args.get("file").and_then(|v| v.as_str()).unwrap_or("");
    let root_str = args.get("repo_root").and_then(|v| v.as_str()).unwrap_or("");

    if file_str.is_empty() || root_str.is_empty() {
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: "missing required parameters: file, repo_root".to_string(),
            }],
            is_error: true,
        };
    }

    let root = std::path::Path::new(root_str);
    let file_path = std::path::Path::new(file_str);

    let rel = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .to_string();

    // Try loading cached graph, build if missing
    let graph = match crate::deps::load_deps_cache(root) {
        Some(g) => g,
        None => {
            // Build graph by walking all files
            let files = match crate::codemap::walk_files_public(root) {
                Ok(f) => f,
                Err(e) => {
                    return ToolResult {
                        content: vec![ToolContent {
                            r#type: "text".to_string(),
                            text: format!("failed to walk files: {e}"),
                        }],
                        is_error: true,
                    };
                }
            };
            let g = crate::deps::build_deps_graph(root, &files);
            crate::deps::save_deps_cache(root, &g);
            g
        }
    };

    let output = crate::deps::query_deps(&graph, &rel);

    ToolResult {
        content: vec![ToolContent {
            r#type: "text".to_string(),
            text: output,
        }],
        is_error: false,
    }
}
```

- [ ] **Step 5: Expose `walk_files` from `codemap.rs`**

Add a public wrapper in `codemap.rs` so `mcp.rs` can use the file walker:

```rust
pub fn walk_files_public(root: &Path) -> Result<Vec<PathBuf>, CodeMapError> {
    walk_files(root, &[])
}
```

- [ ] **Step 6: Integrate graph building into `build_code_map`**

At the end of `build_code_map()`, after saving the code map cache, build and save the dependency graph:

```rust
// Build and cache dependency graph alongside code map
// Note: This rebuilds the full graph on every code_map call. Per-file incremental
// updates (skip unchanged files) is a Phase 2 optimization.
let graph = crate::deps::build_deps_graph(root, &files);
crate::deps::save_deps_cache(root, &graph);
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test mcp::tests::dependencies_tool_returns_deps -- --nocapture`
Expected: PASS

- [ ] **Step 8: Run all tests + clippy**

Run: `cargo test && cargo clippy`
Expected: All tests pass, no warnings

- [ ] **Step 9: Commit**

```bash
git add src/mcp.rs src/codemap.rs
git commit -m "feat: add dependencies MCP tool for cross-file impact analysis"
```

---

## Chunk 4: Orchestration Skill and Plugin Cleanup

### Task 8: Create unified workflow skill

**Files:**
- Create: `skills/taoki-workflow.md`
- Delete: `skills/taoki-map.md`
- Delete: `skills/taoki-index.md`

- [ ] **Step 1: Create `skills/taoki-workflow.md`**

```markdown
---
name: taoki-workflow
description: "Use when starting ANY coding task in a project. Triggers on: implementing features, fixing bugs, refactoring, understanding code, exploring a repo, finding files to modify, planning changes, investigating issues. Use BEFORE Glob, Grep, Read, or Edit."
allowed-tools: mcp__taoki__code_map, mcp__taoki__index, mcp__taoki__dependencies
---

You have access to three structural code intelligence tools. Use them in this order:

## Workflow

### 1. MAP — Understand the repository

Call `mcp__taoki__code_map` with the repository root path. This returns one line per file with:
- Line count
- **[tags]** like `[entry-point]`, `[tests]`, `[data-models]`, `[interfaces]`, `[error-types]`, `[module-root]`
- Public types and function signatures

Results are cached on disk (blake3 hash). Cached calls are near-instant — cheaper than a single Glob. **Always call this first.** Never skip it.

Use the tags to narrow which files matter for your task:
- Fixing a bug? Look for `[error-types]` and related `[tests]` files
- Adding a feature? Look for `[interfaces]` and `[data-models]`
- Understanding entry points? Look for `[entry-point]`

### 2. FOCUS — Find related files

Call `mcp__taoki__dependencies` with the file you plan to modify and the repo root. This shows:
- **depends_on:** files this file imports (what it needs)
- **used_by:** files that import this file (what will be affected by changes)
- **external:** third-party dependencies

**Call this on every file you plan to modify.** Check `used_by` to understand impact before making changes.

### 3. INDEX — Understand file architecture

Call `mcp__taoki__index` on each file you need to understand. This returns the structural skeleton:
- Imports, types, function signatures, impl blocks — all with line numbers
- 70-90% fewer tokens than reading the full file

**Never Read a source file without indexing it first.** Use the line numbers to Read only the specific sections you need.

### 4. READ — Targeted reading

Use the `Read` tool with `offset` and `limit` parameters to read only the specific functions or sections identified by the index. Don't read entire files when you only need a few functions.

### 5. PLAN + IMPLEMENT

With full structural understanding and dependency context, plan your changes and implement them.

## When NOT to use these tools

- For non-code files (config, markdown, JSON) — use Read directly
- When searching for a specific string — use Grep
- When you already know exactly which file and line to edit — skip to Read/Edit

## Tool reference

| Tool | Purpose | When |
|------|---------|------|
| `mcp__taoki__code_map` | Repo overview with file tags | First, always |
| `mcp__taoki__dependencies` | Impact analysis | Before modifying any file |
| `mcp__taoki__index` | File structure with line numbers | Before reading any source file |
```

- [ ] **Step 2: Delete old skills**

```bash
rm skills/taoki-map.md skills/taoki-index.md
```

- [ ] **Step 3: Update commands to reference new tool names**

Read `commands/taoki-map.md` and `commands/taoki-index.md` — keep them as-is (they reference the same MCP tools and work independently of skills).

- [ ] **Step 4: Commit**

```bash
git add skills/taoki-workflow.md
git rm skills/taoki-map.md skills/taoki-index.md
git commit -m "feat: replace individual skills with unified workflow orchestration skill"
```

---

### Task 9: Final integration test and cleanup

**Files:**
- All modified files

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass (should be ~20+ now)

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No warnings (except the known `framing` warning in main.rs)

- [ ] **Step 3: Build release binary**

Run: `cargo build --release`
Expected: Clean build

- [ ] **Step 4: Manual smoke test**

Test the MCP server manually by piping a JSON-RPC request:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | cargo run 2>/dev/null
```

Expected: Response includes `index`, `code_map`, and `dependencies` tools

- [ ] **Step 5: Final commit if any cleanup needed**

Stage only changed files and commit:
```bash
git commit -am "chore: phase 1 integration cleanup"
```
