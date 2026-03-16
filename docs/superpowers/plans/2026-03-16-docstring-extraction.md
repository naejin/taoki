# Docstring Extraction Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the first line of doc comments and attach them to skeleton entries in the `index` tool output, giving agents intent/contract information without reading full source files.

**Architecture:** Add `doc: Option<String>` to `SkeletonEntry`, add `extract_doc_line()` and `strip_doc_prefix()` to `LanguageExtractor` trait with a default sibling-walk implementation. Python overrides entirely (docstrings are body children, not siblings). `format_skeleton()` renders doc lines as `/// text` between the entry header and its children.

**Tech Stack:** Rust, tree-sitter (existing grammars)

**Spec:** `docs/superpowers/specs/2026-03-16-docstring-extraction-design.md`

---

## Chunk 1: Core Infrastructure and Rust Extractor

### Task 1: Add `doc` field to `SkeletonEntry` and `strip_doc_prefix` + `extract_doc_line` to trait

**Files:**
- Modify: `src/index/mod.rs:187-208` (SkeletonEntry struct and `new()`)
- Modify: `src/index/mod.rs:216-239` (LanguageExtractor trait)

- [ ] **Step 1: Add `doc` field to `SkeletonEntry`**

In `src/index/mod.rs`, add `doc: Option<String>` to the struct and initialize it to `None` in `new()`:

```rust
pub(crate) struct SkeletonEntry {
    pub(crate) section: Section,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) text: String,
    pub(crate) children: Vec<String>,
    pub(crate) attrs: Vec<String>,
    pub(crate) insights: self::body::BodyInsights,
    pub(crate) doc: Option<String>,  // first line of docstring
}
```

In `new()`, add `doc: None` to the initializer.

- [ ] **Step 2: Add `strip_doc_prefix` and `extract_doc_line` to `LanguageExtractor` trait**

In `src/index/mod.rs`, add two new methods to the trait. `strip_doc_prefix` is language-specific (required). `extract_doc_line` has a default implementation that walks backward through siblings (same pattern as `doc_comment_start_line`), collects consecutive doc comment nodes, reverses, takes the first one, and calls `strip_doc_prefix`:

```rust
// Inside trait LanguageExtractor:

/// Strip language-specific doc comment prefix from a single line.
/// Returns None if the line is empty after stripping.
fn strip_doc_prefix(&self, _text: &str) -> Option<String> {
    None
}

/// Extract the first line of the doc comment for a node.
/// Default: walk backward through prev_sibling, collect doc comments,
/// reverse, take the first one, call strip_doc_prefix.
fn extract_doc_line(&self, node: Node, source: &[u8]) -> Option<String> {
    let mut doc_nodes = Vec::new();
    let mut prev = node.prev_sibling();
    while let Some(p) = prev {
        if self.is_attr(p) {
            prev = p.prev_sibling();
            continue;
        }
        if self.is_doc_comment(p, source) {
            doc_nodes.push(p);
            prev = p.prev_sibling();
        } else {
            break;
        }
    }
    if doc_nodes.is_empty() {
        return None;
    }
    // Backward walk finds topmost last — reverse to get first doc line
    doc_nodes.reverse();
    let text = node_text(doc_nodes[0], source);
    let stripped = self.strip_doc_prefix(text)?;
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate(trimmed, 120))
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles successfully (default `strip_doc_prefix` returns `None` for all languages, so no doc lines are extracted yet).

- [ ] **Step 4: Commit**

```bash
git add src/index/mod.rs
git commit -m "feat: add doc field to SkeletonEntry and extract_doc_line to LanguageExtractor trait"
```

### Task 2: Implement `strip_doc_prefix` for Rust and write tests

**Files:**
- Modify: `src/index/languages/rust.rs:227-295` (LanguageExtractor impl)
- Modify: `src/index/mod.rs` (tests section)

- [ ] **Step 1: Write the failing test**

In `src/index/mod.rs` tests section, add:

```rust
#[test]
fn rust_doc_comment_extracted() {
    let src = "\
/// Fetches user from the database.
pub fn fetch_user(id: &str) -> User { todo!() }

/// Configuration for the service.
pub struct Config {
    pub host: String,
}

fn no_doc() {}
";
    let out = idx(src, Language::Rust);
    has(&out, &[
        "/// Fetches user from the database.",
        "/// Configuration for the service.",
    ]);
    lacks(&out, &["/// no_doc"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test rust_doc_comment_extracted -- --nocapture 2>&1 | tail -20`
Expected: FAIL — the `///` lines do not appear in output yet.

- [ ] **Step 3: Implement `strip_doc_prefix` for Rust**

In `src/index/languages/rust.rs`, add to the `LanguageExtractor for RustExtractor` impl:

```rust
fn strip_doc_prefix(&self, text: &str) -> Option<String> {
    let stripped = text.strip_prefix("///").unwrap_or(text);
    let trimmed = stripped.strip_prefix(' ').unwrap_or(stripped);
    Some(trimmed.to_string())
}
```

- [ ] **Step 4: Wire doc extraction into `build_skeleton`**

In `src/index/mod.rs`, in `build_skeleton()` inside the `if i == 0` block (around line 464-468), add after the `line_start` adjustment:

```rust
if i == 0 {
    if let Some(doc_start) = doc_comment_start_line(child, source, extractor) {
        entry.line_start = entry.line_start.min(doc_start);
    }
    entry.doc = extractor.extract_doc_line(child, source);
}
```

- [ ] **Step 5: Render doc lines in `format_skeleton`**

In `src/index/mod.rs`, in `format_skeleton()` (around line 319-330), add doc rendering between the entry header and children. The complete rendering block for each entry should be (replacing the existing lines 319-330):

```rust
let _ = writeln!(
    out,
    "  {} {}",
    entry.text,
    line_range(entry.line_start, entry.line_end)
);
if let Some(ref doc) = entry.doc {
    let _ = writeln!(out, "    /// {doc}");
}
for child in &entry.children {
    let _ = writeln!(out, "    {child}");
}
for line in entry.insights.format_lines() {
    let _ = writeln!(out, "    {line}");
}
```

Rendering order: header, doc, children, body insights. This replaces the existing block at lines 319-330 which has: header, children, insights. The only addition is the `if let Some(ref doc)` block between header and children.

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test rust_doc_comment_extracted -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 7: Run all tests to check for regressions**

Run: `cargo test 2>&1 | tail -10`
Expected: All tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/index/mod.rs src/index/languages/rust.rs
git commit -m "feat: extract first-line doc comments for Rust"
```

### Task 3: Rust edge cases — multi-line doc, empty doc, attributes before doc

**Files:**
- Modify: `src/index/mod.rs` (tests section)

- [ ] **Step 1: Write edge case tests**

```rust
#[test]
fn rust_doc_multiline_takes_first() {
    let src = "\
/// Summary line here.
/// More details on second line.
/// Even more details.
pub fn documented() {}
";
    let out = idx(src, Language::Rust);
    has(&out, &["/// Summary line here."]);
    lacks(&out, &["More details", "Even more"]);
}

#[test]
fn rust_doc_with_attrs() {
    let src = "\
/// Does the thing.
#[derive(Debug)]
pub struct Thing {}
";
    let out = idx(src, Language::Rust);
    has(&out, &["/// Does the thing."]);
}

#[test]
fn rust_no_doc_no_line() {
    let src = "pub fn bare() {}\n";
    let out = idx(src, Language::Rust);
    lacks(&out, &["///"]);
}

#[test]
fn rust_empty_doc_comment_ignored() {
    let src = "///\n///   \npub fn blank_doc() {}\n";
    let out = idx(src, Language::Rust);
    lacks(&out, &["/// \n"]);
    // The output should contain the function but no doc line
    has(&out, &["pub blank_doc()"]);
}
```

**Known limitation:** For Rust, the default `extract_doc_line` takes the first `///` sibling node. If that node is empty (bare `///`), it returns `None` — it does NOT scan subsequent `///` nodes for a non-empty line. This differs from the Python/TS/JS extractors which iterate lines within a single node. This is acceptable because Rust convention is to put the summary on the first `///` line; an empty first `///` line is unconventional.

- [ ] **Step 2: Run tests**

Run: `cargo test rust_doc_ -- --nocapture 2>&1 | tail -30`
Expected: All pass. The default `extract_doc_line` already handles multi-line (takes first) and attrs (skips via `is_attr`).

- [ ] **Step 3: Commit**

```bash
git add src/index/mod.rs
git commit -m "test: Rust doc extraction edge cases"
```

---

## Chunk 2: TypeScript/JS and Go Extractors

### Task 4: Implement `strip_doc_prefix` for TypeScript/JS and write tests

**Files:**
- Modify: `src/index/languages/typescript.rs:259-348` (LanguageExtractor impl)
- Modify: `src/index/mod.rs` (tests section)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn ts_doc_comment_extracted() {
    let src = "\
/** Handles incoming requests. */
export function handleRequest(req: Request): Response { return req; }

/**
 * Application configuration.
 * Loaded from environment variables.
 */
export interface Config {
    port: number;
    host: string;
}

function undocumented() {}
";
    let out = idx(src, Language::TypeScript);
    has(&out, &[
        "/// Handles incoming requests.",
        "/// Application configuration.",
    ]);
    lacks(&out, &["/// undocumented", "Loaded from"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test ts_doc_comment_extracted -- --nocapture 2>&1 | tail -20`
Expected: FAIL

- [ ] **Step 3: Implement `strip_doc_prefix` for TypeScript/JS**

In `src/index/languages/typescript.rs`, add to the `LanguageExtractor for TsJsExtractor` impl:

```rust
fn strip_doc_prefix(&self, text: &str) -> Option<String> {
    let text = text.trim();
    // Single-line: /** Foo. */
    if let Some(inner) = text.strip_prefix("/**") {
        if let Some(inner) = inner.strip_suffix("*/") {
            let trimmed = inner.trim().trim_start_matches('*').trim();
            if trimmed.is_empty() { return None; }
            return Some(trimmed.to_string());
        }
        // First line of multi-line: /** or /** Foo
        let after = inner.trim().trim_start_matches('*').trim();
        if after.is_empty() { return None; }
        return Some(after.to_string());
    }
    // Middle lines: * Foo
    if let Some(inner) = text.strip_prefix('*') {
        let trimmed = inner.trim();
        if trimmed.is_empty() || trimmed == "/" { return None; }
        return Some(trimmed.to_string());
    }
    None
}
```

Note: The default `extract_doc_line()` takes the first doc comment sibling node. For TS/JS, `is_doc_comment` matches `comment` nodes starting with `/**`. A single-line `/** Foo */` is one node. A multi-line `/** ... */` is also one node — but `node_text` returns the entire comment. We need to handle this in `extract_doc_line` by overriding it for TS/JS to split multi-line JSDoc blocks.

Override `extract_doc_line` in `TsJsExtractor`:

```rust
fn extract_doc_line(&self, node: Node, source: &[u8]) -> Option<String> {
    let mut prev = node.prev_sibling();
    while let Some(p) = prev {
        if self.is_doc_comment(p, source) {
            let full = node_text(p, source);
            // Try each line until we get a non-empty stripped result
            for line in full.lines() {
                if let Some(stripped) = self.strip_doc_prefix(line) {
                    return Some(truncate(&stripped, 120));
                }
            }
            return None;
        }
        // Skip non-doc siblings (whitespace, regular comments)
        if p.kind() == "comment" || p.is_extra() {
            prev = p.prev_sibling();
            continue;
        }
        break;
    }
    None
}
```

**Known limitation:** TS/JS decorators (`@decorator`) between a JSDoc comment and a declaration will cause the JSDoc to be missed, since decorators are neither `comment` nor `is_extra()` nodes. This matches current behavior (TS/JS `is_attr` returns `false`). Decorators rarely separate JSDoc from declarations in practice.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test ts_doc_comment_extracted -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1 | tail -10`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/typescript.rs src/index/mod.rs
git commit -m "feat: extract first-line doc comments for TypeScript/JS"
```

### Task 5: Implement `strip_doc_prefix` for Go with adjacency check

**Files:**
- Modify: `src/index/languages/go.rs:218-291` (LanguageExtractor impl)
- Modify: `src/index/mod.rs` (tests section)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn go_doc_comment_extracted() {
    let src = "\
package main

// FetchUser retrieves a user by ID.
func FetchUser(id string) (*User, error) { return nil, nil }

// Config holds application settings.
type Config struct {
    Host string
    Port int
}

// not adjacent — blank line separates

func Bare() {}
";
    let out = idx(src, Language::Go);
    has(&out, &[
        "/// FetchUser retrieves a user by ID.",
        "/// Config holds application settings.",
    ]);
    lacks(&out, &["/// not adjacent", "/// Bare"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test go_doc_comment_extracted -- --nocapture 2>&1 | tail -20`
Expected: FAIL

- [ ] **Step 3: Implement for Go with adjacency check**

Go's `is_doc_comment` returns `true` for ALL `comment` nodes. We need to override `extract_doc_line` to check adjacency (no blank line gap between comment end and item start):

In `src/index/languages/go.rs`, add to `LanguageExtractor for GoExtractor`:

```rust
fn strip_doc_prefix(&self, text: &str) -> Option<String> {
    let stripped = text.strip_prefix("//").unwrap_or(text);
    let trimmed = stripped.strip_prefix(' ').unwrap_or(stripped);
    if trimmed.is_empty() { return None; }
    Some(trimmed.to_string())
}

fn extract_doc_line(&self, node: Node, source: &[u8]) -> Option<String> {
    let item_start_row = node.start_position().row;
    let mut doc_nodes = Vec::new();
    let mut prev = node.prev_sibling();
    while let Some(p) = prev {
        if p.kind() != "comment" {
            break;
        }
        let text = node_text(p, source);
        // Only line comments (//), not block comments (/* */)
        if !text.starts_with("//") {
            break;
        }
        doc_nodes.push(p);
        prev = p.prev_sibling();
    }
    if doc_nodes.is_empty() {
        return None;
    }
    // Check adjacency: closest comment must be on the line directly before the item
    // (doc_nodes[0] is the closest since we walk backward)
    let closest = doc_nodes[0];
    if closest.end_position().row + 1 < item_start_row {
        return None;  // blank line gap — not a doc comment
    }
    // First doc line is last in our backward-collected vec
    doc_nodes.reverse();
    let text = node_text(doc_nodes[0], source);
    let stripped = self.strip_doc_prefix(text)?;
    Some(truncate(&stripped, 120))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test go_doc_comment_extracted -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1 | tail -10`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/go.rs src/index/mod.rs
git commit -m "feat: extract first-line doc comments for Go with adjacency check"
```

---

## Chunk 3: Java, Python Extractors and Truncation

### Task 6: Implement `strip_doc_prefix` for Java

**Files:**
- Modify: `src/index/languages/java.rs:237-306` (LanguageExtractor impl)
- Modify: `src/index/mod.rs` (tests section)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn java_doc_comment_extracted() {
    let src = "\
package com.example;

/** Handles user operations. */
public class UserService {
    public void doStuff() {}
}

/**
 * Represents a user in the system.
 * Contains identity and role information.
 */
public record User(String name, String role) {}

public class Bare {}
";
    let out = idx(src, Language::Java);
    has(&out, &[
        "/// Handles user operations.",
        "/// Represents a user in the system.",
    ]);
    lacks(&out, &["/// Bare", "Contains identity"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test java_doc_comment_extracted -- --nocapture 2>&1 | tail -20`
Expected: FAIL

- [ ] **Step 3: Implement for Java**

Java uses `block_comment` starting with `/**` — same structure as TS/JS JSDoc. Add to `LanguageExtractor for JavaExtractor`:

```rust
fn strip_doc_prefix(&self, text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix("/**") {
        if let Some(inner) = inner.strip_suffix("*/") {
            let trimmed = inner.trim().trim_start_matches('*').trim();
            if trimmed.is_empty() { return None; }
            return Some(trimmed.to_string());
        }
        let after = inner.trim().trim_start_matches('*').trim();
        if after.is_empty() { return None; }
        return Some(after.to_string());
    }
    if let Some(inner) = text.strip_prefix('*') {
        let trimmed = inner.trim();
        if trimmed.is_empty() || trimmed == "/" { return None; }
        return Some(trimmed.to_string());
    }
    None
}

fn extract_doc_line(&self, node: Node, source: &[u8]) -> Option<String> {
    let mut prev = node.prev_sibling();
    while let Some(p) = prev {
        if self.is_doc_comment(p, source) {
            let full = node_text(p, source);
            for line in full.lines() {
                if let Some(stripped) = self.strip_doc_prefix(line) {
                    return Some(truncate(&stripped, 120));
                }
            }
            return None;
        }
        if p.kind() == "block_comment" || p.kind() == "line_comment" || p.is_extra() {
            prev = p.prev_sibling();
            continue;
        }
        break;
    }
    None
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test java_doc_comment_extracted -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/index/languages/java.rs src/index/mod.rs
git commit -m "feat: extract first-line doc comments for Java"
```

### Task 7: Implement `extract_doc_line` for Python (body-based docstrings)

**Files:**
- Modify: `src/index/languages/python.rs:182-325` (LanguageExtractor impl)
- Modify: `src/index/mod.rs` (tests section)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn python_doc_comment_extracted() {
    let src = r#"
def fetch_user(user_id: str) -> User:
    """Fetch a user from the database."""
    pass

class Config:
    """Application configuration."""
    host: str
    port: int

def bare():
    pass

def multiline_doc():
    """
    Summary on second line.
    More details here.
    """
    pass
"#;
    let out = idx(src, Language::Python);
    has(&out, &[
        "/// Fetch a user from the database.",
        "/// Application configuration.",
        "/// Summary on second line.",
    ]);
    lacks(&out, &["/// bare", "More details"]);
}

#[test]
fn python_empty_docstring_ignored() {
    let src = "def empty():\n    \"\"\"   \"\"\"\n    pass\n";
    let out = idx(src, Language::Python);
    has(&out, &["empty()"]);
    lacks(&out, &["///"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test python_doc_comment_extracted python_empty_docstring -- --nocapture 2>&1 | tail -20`
Expected: FAIL

- [ ] **Step 3: Implement for Python**

Python docstrings are NOT siblings — they're the first `expression_statement` > `string` child inside the function/class body. Override `extract_doc_line` entirely. The `strip_doc_prefix` default returning `None` is fine since Python won't use it.

In `src/index/languages/python.rs`, add to `LanguageExtractor for PythonExtractor`:

```rust
fn extract_doc_line(&self, node: Node, source: &[u8]) -> Option<String> {
    // For decorated_definition, unwrap to inner definition
    let def_node = if node.kind() == "decorated_definition" {
        find_child(node, "function_definition")
            .or_else(|| find_child(node, "class_definition"))?
    } else {
        node
    };

    // Find the body block
    let body = def_node.child_by_field_name("body")?;

    // First child of body should be expression_statement > string
    let mut cursor = body.walk();
    let first_child = body.children(&mut cursor).next()?;
    if first_child.kind() != "expression_statement" {
        return None;
    }
    let string_node = first_child.child(0)?;
    if string_node.kind() != "string" {
        return None;
    }

    let text = node_text(string_node, source);
    // Strip optional string prefix (r, u, b, etc.) and triple-quote markers
    let after_prefix = text
        .strip_prefix("r\"\"\"")
        .or_else(|| text.strip_prefix("u\"\"\""))
        .or_else(|| text.strip_prefix("b\"\"\""))
        .or_else(|| text.strip_prefix("\"\"\""))
        .or_else(|| text.strip_prefix("r'''"))
        .or_else(|| text.strip_prefix("u'''"))
        .or_else(|| text.strip_prefix("b'''"))
        .or_else(|| text.strip_prefix("'''"))?;
    let inner = after_prefix
        .strip_suffix("\"\"\"")
        .or_else(|| after_prefix.strip_suffix("'''"))
        .unwrap_or(after_prefix);

    // Find first non-empty line
    for line in inner.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return Some(truncate(trimmed, 120));
        }
    }
    None
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test python_doc_comment_extracted -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1 | tail -10`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/python.rs src/index/mod.rs
git commit -m "feat: extract first-line docstrings for Python"
```

### Task 8: Truncation test and cache version bump

**Files:**
- Modify: `src/index/mod.rs` (tests section)
- Modify: `src/codemap.rs:55` (CACHE_VERSION)

- [ ] **Step 1: Write truncation test**

```rust
#[test]
fn doc_comment_truncated_at_120() {
    let long_doc = format!("/// {}", "a".repeat(130));
    let src = format!("{long_doc}\npub fn long_doc() {{}}\n");
    let out = idx(&src, Language::Rust);
    assert!(out.contains("..."), "expected truncation in:\n{out}");
    // The doc line in output should be <= 120 chars (excluding the "    /// " prefix)
    for line in out.lines() {
        if line.contains("/// ") && line.contains("...") {
            let doc_content = line.trim().strip_prefix("/// ").unwrap();
            assert!(doc_content.chars().count() <= 120,
                "doc too long ({} chars): {doc_content}", doc_content.chars().count());
        }
    }
}
```

- [ ] **Step 2: Run test**

Run: `cargo test doc_comment_truncated_at_120 -- --nocapture 2>&1 | tail -20`
Expected: PASS (truncation was already wired in via `truncate()` call in `extract_doc_line`).

- [ ] **Step 3: Bump cache version**

In `src/codemap.rs`, change line 55:

```rust
const CACHE_VERSION: u32 = 4;
```

- [ ] **Step 4: Run all tests and clippy**

Run: `cargo test 2>&1 | tail -10 && cargo clippy 2>&1 | tail -10`
Expected: All tests pass, no clippy warnings.

- [ ] **Step 5: Commit**

```bash
git add src/index/mod.rs src/codemap.rs
git commit -m "feat: add truncation test and bump cache version to 4 for docstring extraction"
```

---

## Chunk 4: Update existing tests and final validation

### Task 9: Update `rust_all_sections` test to include a documented item

**Files:**
- Modify: `src/index/mod.rs` (existing `rust_all_sections` test, around line 546)

- [ ] **Step 1: Add a doc comment to the existing test source**

In the `rust_all_sections` test, add a `///` doc comment before `pub fn process`:

Change:
```rust
pub fn process(input: &str) -> Result<String, Error> { todo!() }
```
To:
```rust
/// Process the input string.
pub fn process(input: &str) -> Result<String, Error> { todo!() }
```

And add to the `has` assertions:
```rust
"/// Process the input string.",
```

- [ ] **Step 2: Run the test**

Run: `cargo test rust_all_sections -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/index/mod.rs
git commit -m "test: add doc comment to rust_all_sections test"
```

### Task 10: Update `ts_all_sections` test similarly

**Files:**
- Modify: `src/index/mod.rs` (existing `ts_all_sections` test)

- [ ] **Step 1: Read the current `ts_all_sections` test to find a good item to document**

Read `src/index/mod.rs` around the `ts_all_sections` test to see its source.

- [ ] **Step 2: Add a JSDoc comment to one export and assert it**

Add `/** The main handler. */` before `export function handler` (or similar exported function in the test), and add to `has`:
```rust
"/// The main handler.",
```

- [ ] **Step 3: Run the test**

Run: `cargo test ts_all_sections -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/index/mod.rs
git commit -m "test: add doc comment to ts_all_sections test"
```

### Task 11: Final validation — full test suite and clippy

**Files:** None (validation only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass (should be ~95+ tests now).

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1`
Expected: No warnings (except the known `framing` warning in `main.rs`).

- [ ] **Step 3: Manual smoke test**

Run taoki against its own repo to see doc lines in output:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{"path":"src/index/mod.rs"}}}' | cargo run 2>/dev/null | python3 -c "import sys,json; [print(json.loads(l).get('result',{}).get('content',[{}])[0].get('text','')[:2000]) for l in sys.stdin if l.strip()]"
```

Expected: `///` doc lines appear in the skeleton output for documented items.

- [ ] **Step 4: Final commit if any adjustments needed**

If the smoke test reveals formatting issues, fix and commit.
