# Body Insights Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract call graph, match/switch arms, and error return sites from function bodies using tree-sitter, and render them in skeleton output.

**Architecture:** New `src/index/body.rs` module with generic body analysis functions parameterized by `Language`. A shared `walk_body()` traverses function bodies while skipping nested definitions. Three extractors (`extract_calls`, `extract_match_arms`, `extract_error_returns`) walk the body and collect insights. Results are attached to `SkeletonEntry` via a new `insights` field and rendered by `format_skeleton()`. Method-level insights are formatted via `BodyInsights::format_lines()` and appended as child strings in each language extractor.

**Tech Stack:** Rust, tree-sitter 0.26 with language grammars at 0.23. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-03-16-body-insights-design.md`

---

## Chunk 1: Foundation — Data Structures, Formatting, and SkeletonEntry Changes

### Task 1: Create body.rs with data structures and constants

**Files:**
- Create: `src/index/body.rs`
- Modify: `src/index/mod.rs:6` (add mod declaration)

- [ ] **Step 1: Create body.rs with types, constants, and empty analyze_body**

```rust
// src/index/body.rs
use tree_sitter::Node;

use crate::index::{Language, node_text, truncate};

// Truncation constants (single source of truth)
pub(crate) const INSIGHT_CALL_TRUNCATE: usize = 40;
pub(crate) const INSIGHT_MATCH_TARGET_TRUNCATE: usize = 30;
pub(crate) const INSIGHT_ARM_TRUNCATE: usize = 30;
pub(crate) const INSIGHT_ERROR_TRUNCATE: usize = 40;
pub(crate) const MAX_CALLS: usize = 12;
pub(crate) const MAX_MATCH_ARMS: usize = 10;
pub(crate) const MAX_ERRORS: usize = 8;

#[derive(Debug, Clone, Default)]
pub(crate) struct MatchInsight {
    pub(crate) target: String,
    pub(crate) arms: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BodyInsights {
    pub(crate) calls: Vec<String>,
    pub(crate) match_arms: Vec<MatchInsight>,
    pub(crate) error_returns: Vec<String>,
    pub(crate) try_count: usize,
}

impl BodyInsights {
    pub(crate) fn is_empty(&self) -> bool {
        self.calls.is_empty()
            && self.match_arms.is_empty()
            && self.error_returns.is_empty()
            && self.try_count == 0
    }

    pub(crate) fn format_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();

        // Calls (sorted lexicographically, truncated)
        if !self.calls.is_empty() {
            let display: Vec<&str> = self.calls.iter().take(MAX_CALLS).map(|s| s.as_str()).collect();
            let suffix = if self.calls.len() > MAX_CALLS { ", ..." } else { "" };
            lines.push(format!("→ calls: {}{suffix}", display.join(", ")));
        }

        // Match/switch arms (source order)
        for m in &self.match_arms {
            let arms_display: Vec<&str> = m.arms.iter().take(MAX_MATCH_ARMS).map(|s| s.as_str()).collect();
            let suffix = if m.arms.len() > MAX_MATCH_ARMS { ", ..." } else { "" };
            lines.push(format!("→ match: {} → {}{suffix}", m.target, arms_display.join(", ")));
        }

        // Errors (named first in source order, then ? count)
        if !self.error_returns.is_empty() || self.try_count > 0 {
            let mut parts: Vec<String> = self.error_returns
                .iter()
                .take(MAX_ERRORS)
                .cloned()
                .collect();
            if self.error_returns.len() > MAX_ERRORS {
                parts.push("...".to_string());
            }
            if self.try_count > 0 {
                parts.push(format!("{}× ?", self.try_count));
            }
            lines.push(format!("→ errors: {}", parts.join(", ")));
        }

        lines
    }
}

/// Analyze a function/method declaration node and extract body insights.
/// Pass the function declaration node itself (e.g., `function_item`), not the body.
/// Returns empty insights if the node has no body (abstract/interface methods).
pub(crate) fn analyze_body(node: Node, source: &[u8], lang: Language) -> BodyInsights {
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return BodyInsights::default(),
    };

    let calls = extract_calls(body, source, lang);
    let match_arms = extract_match_arms(body, source, lang);
    let (error_returns, try_count) = extract_error_returns(body, source, lang);

    BodyInsights {
        calls,
        match_arms,
        error_returns,
        try_count,
    }
}

// --- Placeholder implementations (filled in subsequent tasks) ---

fn extract_calls(_body: Node, _source: &[u8], _lang: Language) -> Vec<String> {
    Vec::new()
}

fn extract_match_arms(_body: Node, _source: &[u8], _lang: Language) -> Vec<MatchInsight> {
    Vec::new()
}

fn extract_error_returns(_body: Node, _source: &[u8], _lang: Language) -> (Vec<String>, usize) {
    (Vec::new(), 0)
}

#[cfg(test)]
mod tests {
    use super::*;
}
```

- [ ] **Step 2: Add mod declaration in index/mod.rs**

In `src/index/mod.rs:6`, after `mod languages;`, add:

```rust
pub(crate) mod body;
```

- [ ] **Step 3: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1`
Expected: Compiles with warnings about unused imports/functions (acceptable at this stage).

- [ ] **Step 4: Commit**

```bash
git add src/index/body.rs src/index/mod.rs
git commit -m "feat: add body.rs module with BodyInsights types and format_lines"
```

### Task 2: Test and finalize format_lines()

**Files:**
- Modify: `src/index/body.rs` (add tests)

- [ ] **Step 1: Write tests for format_lines()**

Add to the `tests` module in `body.rs`:

```rust
#[test]
fn test_format_lines_empty() {
    let insights = BodyInsights::default();
    assert!(insights.is_empty());
    assert!(insights.format_lines().is_empty());
}

#[test]
fn test_format_lines_calls_only() {
    let insights = BodyInsights {
        calls: vec!["bar".into(), "foo".into(), "qux".into()],
        ..Default::default()
    };
    assert!(!insights.is_empty());
    assert_eq!(insights.format_lines(), vec!["→ calls: bar, foo, qux"]);
}

#[test]
fn test_format_lines_calls_truncated() {
    let calls: Vec<String> = (0..15).map(|i| format!("fn_{i}")).collect();
    let insights = BodyInsights {
        calls,
        ..Default::default()
    };
    let lines = insights.format_lines();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].ends_with(", ..."));
    // Should contain exactly 12 function names
    assert_eq!(lines[0].matches(',').count(), 12); // 11 commas between 12 items + ", ..."
}

#[test]
fn test_format_lines_match() {
    let insights = BodyInsights {
        match_arms: vec![MatchInsight {
            target: "cmd".into(),
            arms: vec!["\"start\"".into(), "\"stop\"".into(), "_".into()],
        }],
        ..Default::default()
    };
    let lines = insights.format_lines();
    assert_eq!(lines, vec!["→ match: cmd → \"start\", \"stop\", _"]);
}

#[test]
fn test_format_lines_errors_with_try() {
    let insights = BodyInsights {
        error_returns: vec!["IoError".into(), "ParseError".into()],
        try_count: 3,
        ..Default::default()
    };
    let lines = insights.format_lines();
    assert_eq!(lines, vec!["→ errors: IoError, ParseError, 3× ?"]);
}

#[test]
fn test_format_lines_try_only() {
    let insights = BodyInsights {
        try_count: 5,
        ..Default::default()
    };
    let lines = insights.format_lines();
    assert_eq!(lines, vec!["→ errors: 5× ?"]);
}

#[test]
fn test_format_lines_all_sections() {
    let insights = BodyInsights {
        calls: vec!["alpha".into(), "beta".into()],
        match_arms: vec![MatchInsight {
            target: "x".into(),
            arms: vec!["1".into(), "2".into()],
        }],
        error_returns: vec!["Err(NotFound)".into()],
        try_count: 1,
    };
    let lines = insights.format_lines();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "→ calls: alpha, beta");
    assert_eq!(lines[1], "→ match: x → 1, 2");
    assert_eq!(lines[2], "→ errors: Err(NotFound), 1× ?");
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib body::tests -- --nocapture 2>&1`
Expected: All 6 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/index/body.rs
git commit -m "test: add format_lines unit tests for BodyInsights"
```

### Task 3: Add insights field to SkeletonEntry and update format_skeleton

**Files:**
- Modify: `src/index/mod.rs:186-206` (SkeletonEntry struct and new())
- Modify: `src/index/mod.rs:288-337` (format_skeleton)

- [ ] **Step 1: Add insights field to SkeletonEntry**

In `src/index/mod.rs`, add import at top (after existing imports):

```rust
use crate::index::body::BodyInsights;
```

Note: This requires changing the existing `mod languages;` / `pub(crate) mod body;` to appear before the `use` statements, OR using `self::body::BodyInsights`. Since `body` is declared as `pub(crate) mod body;` in the same file, use:

```rust
use self::body::BodyInsights;
```

Add to `SkeletonEntry` struct (after `attrs` field at line 192):

```rust
    pub(crate) insights: BodyInsights,
```

Update `SkeletonEntry::new()` (line 196-205) to initialize insights:

```rust
    pub(crate) fn new(section: Section, node: Node, text: String) -> Self {
        Self {
            section,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            text,
            children: Vec::new(),
            attrs: Vec::new(),
            insights: BodyInsights::default(),
        }
    }
```

- [ ] **Step 2: Update format_skeleton to render insights**

In `format_skeleton()` (line 288-337), after the children rendering loop (after line 324 `}`), add insight rendering:

Replace the block:
```rust
                for child in &entry.children {
                    let _ = writeln!(out, "    {child}");
                }
```

With:
```rust
                for child in &entry.children {
                    let _ = writeln!(out, "    {child}");
                }
                for line in entry.insights.format_lines() {
                    let _ = writeln!(out, "    {line}");
                }
```

- [ ] **Step 3: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1`
Expected: Compiles successfully.

- [ ] **Step 4: Run all existing tests to verify no regression**

Run: `cargo test 2>&1`
Expected: All 52+ tests pass. Existing skeleton output is unchanged because all `insights` fields default to empty.

- [ ] **Step 5: Commit**

```bash
git add src/index/mod.rs
git commit -m "feat: add insights field to SkeletonEntry, render in format_skeleton"
```

---

## Chunk 2: Body Walker and Call Graph Extraction

### Task 4: Implement walk_body utility

**Files:**
- Modify: `src/index/body.rs`

- [ ] **Step 1: Write test for walk_body with nested function skipping**

Add to tests module in `body.rs`:

```rust
use tree_sitter::Parser;
use crate::index::Language;

fn parse_and_get_fn_body(source: &str, lang: Language) -> (tree_sitter::Tree, Vec<u8>) {
    let bytes = source.as_bytes().to_vec();
    let mut parser = Parser::new();
    parser.set_language(&lang.ts_language()).unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    (tree, bytes)
}

#[test]
fn test_walk_body_skips_nested_rust() {
    let src = r#"
fn outer() {
    foo();
    let f = || bar();
    fn inner() { baz(); }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
    let root = tree.root_node();
    // Find the function_item
    let fn_node = root.child(0).unwrap();
    assert_eq!(fn_node.kind(), "function_item");
    let body = fn_node.child_by_field_name("body").unwrap();

    let mut visited_kinds = Vec::new();
    walk_body(body, Language::Rust, &mut |node| {
        if node.kind() == "call_expression" {
            let callee = node_text(node.child(0).unwrap(), &bytes);
            visited_kinds.push(callee.to_string());
        }
    });
    // Should see foo() but NOT bar() (closure) or baz() (inner fn)
    assert_eq!(visited_kinds, vec!["foo"]);
}
```

- [ ] **Step 2: Implement walk_body**

Add to `body.rs` (replacing any placeholder):

```rust
/// Check if a node is a function/closure definition that should not be descended into.
fn is_nested_function_def(node: Node, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(node.kind(), "function_item" | "closure_expression"),
        Language::Python => matches!(node.kind(), "function_definition" | "lambda"),
        Language::TypeScript | Language::JavaScript => {
            matches!(node.kind(), "function_declaration" | "arrow_function" | "function")
        }
        Language::Go => matches!(node.kind(), "func_literal" | "function_declaration"),
        Language::Java => matches!(node.kind(), "method_declaration" | "lambda_expression"),
    }
}

/// Recursively walk a function body, visiting every node except those inside
/// nested function/closure definitions. The visitor sees each node once.
fn walk_body(node: Node, lang: Language, visitor: &mut impl FnMut(Node)) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_nested_function_def(child, lang) {
            continue;
        }
        visitor(child);
        walk_body(child, lang, visitor);
    }
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test --lib body::tests::test_walk_body_skips_nested_rust -- --nocapture 2>&1`
Expected: PASS.

- [ ] **Step 4: Write nested-skip tests for Python and TypeScript**

```rust
#[test]
fn test_walk_body_skips_nested_python() {
    let src = r#"
def outer():
    foo()
    inner = lambda: bar()
    def nested():
        baz()
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Python);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    assert_eq!(fn_node.kind(), "function_definition");
    let body = fn_node.child_by_field_name("body").unwrap();

    let mut calls = Vec::new();
    walk_body(body, Language::Python, &mut |node| {
        if node.kind() == "call" {
            if let Some(func) = node.child_by_field_name("function") {
                calls.push(node_text(func, &bytes).to_string());
            }
        }
    });
    assert_eq!(calls, vec!["foo"]);
}

#[test]
fn test_walk_body_skips_nested_typescript() {
    let src = r#"
function outer() {
    foo();
    const f = () => bar();
    function inner() { baz(); }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::TypeScript);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    assert_eq!(fn_node.kind(), "function_declaration");
    let body = fn_node.child_by_field_name("body").unwrap();

    let mut calls = Vec::new();
    walk_body(body, Language::TypeScript, &mut |node| {
        if node.kind() == "call_expression" {
            if let Some(func) = node.child_by_field_name("function") {
                let text = node_text(func, &bytes);
                calls.push(text.to_string());
            }
        }
    });
    assert_eq!(calls, vec!["foo"]);
}
```

- [ ] **Step 5: Run all walk_body tests**

Run: `cargo test --lib body::tests::test_walk_body -- --nocapture 2>&1`
Expected: All 3 PASS.

- [ ] **Step 6: Commit**

```bash
git add src/index/body.rs
git commit -m "feat: implement walk_body with nested function skipping"
```

### Task 5: Implement extract_calls for all languages

**Files:**
- Modify: `src/index/body.rs`

- [ ] **Step 1: Write test for Rust call extraction**

```rust
#[test]
fn test_extract_calls_rust() {
    let src = r#"
fn example() {
    let x = foo();
    bar::baz();
    x.method();
    alpha(beta(gamma()));
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();

    let calls = extract_calls(body, &bytes, Language::Rust);
    // Sorted lexicographically, deduplicated
    assert_eq!(calls, vec!["alpha", "bar::baz", "beta", "foo", "gamma", "method"]);
}
```

- [ ] **Step 2: Implement extract_calls**

Replace the placeholder `extract_calls` in `body.rs`:

```rust
/// Extract the callee name from a call expression node.
fn extract_callee_name<'a>(node: Node<'a>, source: &'a [u8], lang: Language) -> Option<&'a str> {
    match lang {
        Language::Rust => {
            // call_expression: function field is the callee
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => Some(node_text(func, source)),
                "scoped_identifier" | "field_expression" => {
                    // For scoped: take full path (e.g., "bar::baz")
                    // For field_expression: take the field name
                    if func.kind() == "field_expression" {
                        let field = func.child_by_field_name("field")?;
                        Some(node_text(field, source))
                    } else {
                        Some(node_text(func, source))
                    }
                }
                _ => Some(node_text(func, source)),
            }
        }
        Language::Python => {
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => Some(node_text(func, source)),
                "attribute" => {
                    let attr = func.child_by_field_name("attribute")?;
                    Some(node_text(attr, source))
                }
                _ => None,
            }
        }
        Language::TypeScript | Language::JavaScript => {
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => Some(node_text(func, source)),
                "member_expression" => {
                    let prop = func.child_by_field_name("property")?;
                    Some(node_text(prop, source))
                }
                _ => None,
            }
        }
        Language::Go => {
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => Some(node_text(func, source)),
                "selector_expression" => {
                    let field = func.child_by_field_name("field")?;
                    Some(node_text(field, source))
                }
                _ => None,
            }
        }
        Language::Java => {
            // method_invocation: name field
            let name = node.child_by_field_name("name")?;
            Some(node_text(name, source))
        }
    }
}

/// Check if a node is a call expression for the given language.
fn is_call_node(node: Node, lang: Language) -> bool {
    match lang {
        Language::Java => node.kind() == "method_invocation",
        Language::Python => node.kind() == "call",
        _ => node.kind() == "call_expression",
    }
}

fn extract_calls(body: Node, source: &[u8], lang: Language) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    walk_body(body, lang, &mut |node| {
        if is_call_node(node, lang) {
            if let Some(name) = extract_callee_name(node, source, lang) {
                let truncated = truncate(name, INSIGHT_CALL_TRUNCATE);
                seen.insert(truncated);
            }
        }
    });
    seen.into_iter().collect() // BTreeSet gives sorted order
}
```

- [ ] **Step 3: Run the Rust call test**

Run: `cargo test --lib body::tests::test_extract_calls_rust -- --nocapture 2>&1`
Expected: PASS.

- [ ] **Step 4: Write and run tests for Python, TypeScript, Go, Java**

```rust
#[test]
fn test_extract_calls_python() {
    let src = r#"
def example():
    foo()
    obj.method()
    alpha(beta())
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Python);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let calls = extract_calls(body, &bytes, Language::Python);
    assert_eq!(calls, vec!["alpha", "beta", "foo", "method"]);
}

#[test]
fn test_extract_calls_typescript() {
    let src = r#"
function example() {
    foo();
    obj.method();
    alpha(beta());
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::TypeScript);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let calls = extract_calls(body, &bytes, Language::TypeScript);
    assert_eq!(calls, vec!["alpha", "beta", "foo", "method"]);
}

#[test]
fn test_extract_calls_go() {
    let src = r#"
package main
func example() {
    foo()
    pkg.Method()
    alpha(beta())
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Go);
    let root = tree.root_node();
    // Go: package decl is child 0, func is child 1
    let mut cursor = root.walk();
    let fn_node = root.children(&mut cursor)
        .find(|c| c.kind() == "function_declaration")
        .unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let calls = extract_calls(body, &bytes, Language::Go);
    assert_eq!(calls, vec!["Method", "alpha", "beta", "foo"]);
}

#[test]
fn test_extract_calls_java() {
    let src = r#"
class Example {
    void example() {
        foo();
        obj.method();
        alpha(beta());
    }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Java);
    let root = tree.root_node();
    let class_node = root.child(0).unwrap();
    let body = class_node.child_by_field_name("body").unwrap();
    // Find the method_declaration
    let mut cursor = body.walk();
    let method = body.children(&mut cursor)
        .find(|c| c.kind() == "method_declaration")
        .unwrap();
    let method_body = method.child_by_field_name("body").unwrap();
    let calls = extract_calls(method_body, &bytes, Language::Java);
    assert_eq!(calls, vec!["alpha", "beta", "foo", "method"]);
}
```

- [ ] **Step 5: Run all call extraction tests**

Run: `cargo test --lib body::tests::test_extract_calls -- --nocapture 2>&1`
Expected: All 5 PASS.

- [ ] **Step 6: Commit**

```bash
git add src/index/body.rs
git commit -m "feat: implement extract_calls for all 6 languages"
```

---

## Chunk 3: Match/Switch Arm Extraction

### Task 6: Implement extract_match_arms for all languages

**Files:**
- Modify: `src/index/body.rs`

- [ ] **Step 1: Write test for Rust match extraction**

```rust
#[test]
fn test_extract_match_rust() {
    let src = r#"
fn example(cmd: &str) {
    match cmd {
        "start" => start(),
        "stop" => stop(),
        _ => default(),
    }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let matches = extract_match_arms(body, &bytes, Language::Rust);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].target, "cmd");
    assert_eq!(matches[0].arms, vec!["\"start\"", "\"stop\"", "_"]);
}
```

- [ ] **Step 2: Implement extract_match_arms**

Replace the placeholder in `body.rs`:

```rust
fn extract_match_arms(body: Node, source: &[u8], lang: Language) -> Vec<MatchInsight> {
    let mut insights = Vec::new();
    walk_body(body, lang, &mut |node| {
        if let Some(insight) = extract_single_match(node, source, lang) {
            insights.push(insight);
        }
    });
    insights
}

fn extract_single_match(node: Node, source: &[u8], lang: Language) -> Option<MatchInsight> {
    match lang {
        Language::Rust => extract_rust_match(node, source),
        Language::Python => extract_python_match(node, source),
        Language::TypeScript | Language::JavaScript => extract_ts_switch(node, source),
        Language::Go => extract_go_switch(node, source),
        Language::Java => extract_java_switch(node, source),
    }
}

fn extract_rust_match(node: Node, source: &[u8]) -> Option<MatchInsight> {
    if node.kind() != "match_expression" {
        return None;
    }
    let scrutinee = node.child_by_field_name("value")?;
    let target = truncate(node_text(scrutinee, source).trim(), INSIGHT_MATCH_TARGET_TRUNCATE);

    let match_body = node.children(&mut node.walk())
        .find(|c| c.kind() == "match_block")?;

    let mut arms = Vec::new();
    let mut cursor = match_body.walk();
    for child in match_body.children(&mut cursor) {
        if child.kind() == "match_arm" {
            if let Some(pattern) = child.child_by_field_name("pattern") {
                arms.push(truncate(node_text(pattern, source).trim(), INSIGHT_ARM_TRUNCATE));
            }
        }
    }
    Some(MatchInsight { target, arms })
}

fn extract_python_match(node: Node, source: &[u8]) -> Option<MatchInsight> {
    if node.kind() != "match_statement" {
        return None;
    }
    let subject = node.child_by_field_name("subject")?;
    let target = truncate(node_text(subject, source).trim(), INSIGHT_MATCH_TARGET_TRUNCATE);

    // case_clause nodes may be inside a body block or direct children — check both
    let mut arms = Vec::new();
    let body = node.child_by_field_name("body");
    let search_node = body.unwrap_or(node);
    let mut cursor = search_node.walk();
    for child in search_node.children(&mut cursor) {
        if child.kind() == "case_clause" {
            // Pattern is the first named child of case_clause (case_pattern or similar)
            if let Some(pattern) = child.children(&mut child.walk())
                .find(|c| c.is_named() && c.kind() != "block")
            {
                arms.push(truncate(node_text(pattern, source).trim(), INSIGHT_ARM_TRUNCATE));
            }
        }
    }
    Some(MatchInsight { target, arms })
}

fn unwrap_parenthesized(node: Node) -> Node {
    if node.kind() == "parenthesized_expression" {
        // child(0) is '(', child(1) is the inner expression, child(2) is ')'
        node.child(1).unwrap_or(node)
    } else {
        node
    }
}

fn extract_ts_switch(node: Node, source: &[u8]) -> Option<MatchInsight> {
    if node.kind() != "switch_statement" {
        return None;
    }
    let value = node.child_by_field_name("value")?;
    let inner = unwrap_parenthesized(value);
    let target = truncate(node_text(inner, source).trim(), INSIGHT_MATCH_TARGET_TRUNCATE);

    let switch_body = node.children(&mut node.walk())
        .find(|c| c.kind() == "switch_body")?;

    let mut arms = Vec::new();
    let mut cursor = switch_body.walk();
    for child in switch_body.children(&mut cursor) {
        if child.kind() == "switch_case" {
            if let Some(val) = child.child_by_field_name("value") {
                arms.push(truncate(node_text(val, source).trim(), INSIGHT_ARM_TRUNCATE));
            }
        } else if child.kind() == "switch_default" {
            arms.push("default".to_string());
        }
    }
    Some(MatchInsight { target, arms })
}

fn extract_go_switch(node: Node, source: &[u8]) -> Option<MatchInsight> {
    match node.kind() {
        "expression_switch_statement" => {
            let value = node.child_by_field_name("value");
            let target = value
                .map(|v| truncate(node_text(v, source).trim(), INSIGHT_MATCH_TARGET_TRUNCATE))
                .unwrap_or_default();

            let mut arms = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "expression_case" {
                    if let Some(val) = child.child_by_field_name("value") {
                        arms.push(truncate(node_text(val, source).trim(), INSIGHT_ARM_TRUNCATE));
                    }
                } else if child.kind() == "default_case" {
                    arms.push("default".to_string());
                }
            }
            Some(MatchInsight { target, arms })
        }
        "type_switch_statement" => {
            let target = "type".to_string();
            let mut arms = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_case" {
                    if let Some(val) = child.children(&mut child.walk()).find(|c| c.is_named()) {
                        arms.push(truncate(node_text(val, source).trim(), INSIGHT_ARM_TRUNCATE));
                    }
                } else if child.kind() == "default_case" {
                    arms.push("default".to_string());
                }
            }
            Some(MatchInsight { target, arms })
        }
        _ => None,
    }
}

fn extract_java_switch(node: Node, source: &[u8]) -> Option<MatchInsight> {
    if !matches!(node.kind(), "switch_expression" | "switch_statement") {
        return None;
    }
    let condition = node.child_by_field_name("condition")?;
    let inner = unwrap_parenthesized(condition);
    let target = truncate(node_text(inner, source).trim(), INSIGHT_MATCH_TARGET_TRUNCATE);

    let body = node.child_by_field_name("body")?;
    let mut arms = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child.kind() == "switch_block_statement_group" {
            // Find switch_label children
            let mut label_cursor = child.walk();
            for label_child in child.children(&mut label_cursor) {
                if label_child.kind() == "switch_label" {
                    let text = node_text(label_child, source).trim();
                    let cleaned = text.strip_prefix("case ").unwrap_or(text);
                    let cleaned = cleaned.strip_suffix(':').unwrap_or(cleaned);
                    if cleaned == "default" || text.starts_with("default") {
                        arms.push("default".to_string());
                    } else {
                        arms.push(truncate(cleaned.trim(), INSIGHT_ARM_TRUNCATE));
                    }
                }
            }
        }
    }
    Some(MatchInsight { target, arms })
}
```

- [ ] **Step 3: Run the Rust match test**

Run: `cargo test --lib body::tests::test_extract_match_rust -- --nocapture 2>&1`
Expected: PASS. If `pattern` field doesn't work on tree-sitter-rust 0.23, adjust to positional extraction and document.

- [ ] **Step 4: Write and run tests for TypeScript and Go switches**

```rust
#[test]
fn test_extract_match_typescript() {
    let src = r#"
function example(cmd: string) {
    switch (cmd) {
        case "start":
            start();
            break;
        case "stop":
            stop();
            break;
        default:
            other();
    }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::TypeScript);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let matches = extract_match_arms(body, &bytes, Language::TypeScript);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].target, "cmd");
    assert_eq!(matches[0].arms, vec!["\"start\"", "\"stop\"", "default"]);
}

#[test]
fn test_extract_match_go() {
    let src = r#"
package main
func example(cmd string) {
    switch cmd {
    case "start":
        start()
    case "stop":
        stop()
    default:
        other()
    }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Go);
    let root = tree.root_node();
    let mut cursor = root.walk();
    let fn_node = root.children(&mut cursor)
        .find(|c| c.kind() == "function_declaration")
        .unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let matches = extract_match_arms(body, &bytes, Language::Go);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].target, "cmd");
    assert_eq!(matches[0].arms, vec!["\"start\"", "\"stop\"", "default"]);
}
```

- [ ] **Step 5: Run all match tests**

Run: `cargo test --lib body::tests::test_extract_match -- --nocapture 2>&1`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add src/index/body.rs
git commit -m "feat: implement extract_match_arms for all 6 languages"
```

---

## Chunk 4: Error Return Extraction

### Task 7: Implement extract_error_returns for all languages

**Files:**
- Modify: `src/index/body.rs`

- [ ] **Step 1: Write test for Rust error extraction**

```rust
#[test]
fn test_extract_errors_rust() {
    let src = r#"
fn example() -> Result<(), MyError> {
    let x = something()?;
    let y = other()?;
    if bad {
        return Err(MyError::NotFound);
    }
    bail!("oh no");
    Ok(())
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let (errors, try_count) = extract_error_returns(body, &bytes, Language::Rust);
    assert_eq!(try_count, 2);
    assert_eq!(errors, vec!["MyError::NotFound", "bail!"]);
}
```

- [ ] **Step 2: Implement extract_error_returns**

Replace the placeholder in `body.rs`:

```rust
fn extract_error_returns(body: Node, source: &[u8], lang: Language) -> (Vec<String>, usize) {
    let mut errors = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut try_count: usize = 0;

    walk_body(body, lang, &mut |node| {
        match lang {
            Language::Rust => {
                // Count ? operators
                if node.kind() == "try_expression" {
                    try_count += 1;
                    return;
                }
                // Err(...) calls
                if node.kind() == "call_expression" {
                    if let Some(func) = node.child_by_field_name("function") {
                        if node_text(func, source) == "Err" {
                            if let Some(args) = node.child_by_field_name("arguments") {
                                // Get the inner expression (first named child of argument_list)
                                if let Some(inner) = args.children(&mut args.walk())
                                    .find(|c| c.is_named())
                                {
                                    let text = truncate(node_text(inner, source).trim(), INSIGHT_ERROR_TRUNCATE);
                                    if seen.insert(text.clone()) {
                                        errors.push(text);
                                    }
                                }
                            }
                        }
                    }
                }
                // Macro invocations: bail!, anyhow!, panic!, todo!, unimplemented!
                if node.kind() == "macro_invocation" {
                    if let Some(mac) = node.child(0) {
                        let name = node_text(mac, source);
                        if matches!(name, "bail" | "anyhow" | "panic" | "todo" | "unimplemented") {
                            let text = format!("{name}!");
                            if seen.insert(text.clone()) {
                                errors.push(text);
                            }
                        }
                    }
                }
            }
            Language::Python => {
                if node.kind() == "raise_statement" {
                    // First named child is the exception expression
                    if let Some(expr) = node.children(&mut node.walk()).find(|c| c.is_named()) {
                        let name = match expr.kind() {
                            "call" => {
                                // raise SomeError(...) -> extract "SomeError"
                                expr.child_by_field_name("function")
                                    .map(|f| node_text(f, source))
                                    .unwrap_or_else(|| node_text(expr, source))
                            }
                            _ => node_text(expr, source),
                        };
                        let text = truncate(name.trim(), INSIGHT_ERROR_TRUNCATE);
                        if seen.insert(text.clone()) {
                            errors.push(text);
                        }
                    }
                }
            }
            Language::TypeScript | Language::JavaScript => {
                if node.kind() == "throw_statement" {
                    if let Some(expr) = node.children(&mut node.walk()).find(|c| c.is_named()) {
                        let name = match expr.kind() {
                            "new_expression" => {
                                // throw new Error(...) -> extract "Error"
                                expr.child_by_field_name("constructor")
                                    .map(|c| node_text(c, source))
                                    .unwrap_or_else(|| node_text(expr, source))
                            }
                            _ => node_text(expr, source),
                        };
                        let text = truncate(name.trim(), INSIGHT_ERROR_TRUNCATE);
                        if seen.insert(text.clone()) {
                            errors.push(text);
                        }
                    }
                }
            }
            Language::Go => {
                // Look for errors.New(...) or fmt.Errorf(...) calls
                if node.kind() == "call_expression" {
                    if let Some(func) = node.child_by_field_name("function") {
                        let text = node_text(func, source);
                        if matches!(text, "errors.New" | "fmt.Errorf") {
                            let text = truncate(text, INSIGHT_ERROR_TRUNCATE);
                            if seen.insert(text.clone()) {
                                errors.push(text);
                            }
                        }
                    }
                }
            }
            Language::Java => {
                if node.kind() == "throw_statement" {
                    if let Some(expr) = node.children(&mut node.walk()).find(|c| c.is_named()) {
                        let name = match expr.kind() {
                            "object_creation_expression" => {
                                // throw new SomeException(...) -> extract type
                                expr.child_by_field_name("type")
                                    .map(|t| node_text(t, source))
                                    .unwrap_or_else(|| node_text(expr, source))
                            }
                            _ => node_text(expr, source),
                        };
                        let text = truncate(name.trim(), INSIGHT_ERROR_TRUNCATE);
                        if seen.insert(text.clone()) {
                            errors.push(text);
                        }
                    }
                }
            }
        }
    });

    (errors, try_count)
}
```

- [ ] **Step 3: Run the Rust error test**

Run: `cargo test --lib body::tests::test_extract_errors_rust -- --nocapture 2>&1`
Expected: PASS.

- [ ] **Step 4: Write and run tests for Python, TypeScript, Go, Java**

```rust
#[test]
fn test_extract_errors_python() {
    let src = r#"
def example():
    raise ValueError("bad")
    raise NotFoundError()
    raise ValueError("duplicate")
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Python);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let (errors, try_count) = extract_error_returns(body, &bytes, Language::Python);
    assert_eq!(try_count, 0);
    // Deduplicated: ValueError appears once
    assert_eq!(errors, vec!["ValueError", "NotFoundError"]);
}

#[test]
fn test_extract_errors_typescript() {
    let src = r#"
function example() {
    throw new Error("bad");
    throw new NotFoundError("missing");
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::TypeScript);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let (errors, _) = extract_error_returns(body, &bytes, Language::TypeScript);
    assert_eq!(errors, vec!["Error", "NotFoundError"]);
}

#[test]
fn test_extract_errors_go() {
    let src = r#"
package main
import "errors"
import "fmt"
func example() error {
    if bad {
        return errors.New("bad")
    }
    x := fmt.Errorf("failed: %v", err)
    return x
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Go);
    let root = tree.root_node();
    let mut cursor = root.walk();
    let fn_node = root.children(&mut cursor)
        .find(|c| c.kind() == "function_declaration")
        .unwrap();
    let body = fn_node.child_by_field_name("body").unwrap();
    let (errors, _) = extract_error_returns(body, &bytes, Language::Go);
    assert_eq!(errors, vec!["errors.New", "fmt.Errorf"]);
}

#[test]
fn test_extract_errors_java() {
    let src = r#"
class Example {
    void example() {
        throw new IllegalArgumentException("bad");
        throw new RuntimeException("oops");
    }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Java);
    let root = tree.root_node();
    let class_node = root.child(0).unwrap();
    let body = class_node.child_by_field_name("body").unwrap();
    let mut cursor = body.walk();
    let method = body.children(&mut cursor)
        .find(|c| c.kind() == "method_declaration")
        .unwrap();
    let method_body = method.child_by_field_name("body").unwrap();
    let (errors, _) = extract_error_returns(method_body, &bytes, Language::Java);
    assert_eq!(errors, vec!["IllegalArgumentException", "RuntimeException"]);
}
```

- [ ] **Step 5: Run all error tests**

Run: `cargo test --lib body::tests::test_extract_errors -- --nocapture 2>&1`
Expected: All 5 PASS.

- [ ] **Step 6: Commit**

```bash
git add src/index/body.rs
git commit -m "feat: implement extract_error_returns for all 6 languages"
```

---

## Chunk 5: Integration Into Skeleton Pipeline

### Task 8: Wire analyze_body into build_skeleton for top-level functions

**Files:**
- Modify: `src/index/mod.rs:410-468` (index_source, extract_all, build_skeleton)

- [ ] **Step 1: Add Language parameter to build_skeleton and wire analyze_body**

In `src/index/mod.rs`, modify `build_skeleton` signature (line 438) to accept `lang`:

```rust
fn build_skeleton(root: Node, source: &[u8], extractor: &dyn LanguageExtractor, lang: Language) -> String {
```

Inside the loop (after line 463 `entries.push(entry);`), add body analysis. Replace:

```rust
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
```

With:

```rust
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
            // Analyze body for top-level functions and Go methods (which use Section::Impl)
            let is_function = entry.section == Section::Function;
            let is_go_method = lang == Language::Go && child.kind() == "method_declaration";
            if is_function || is_go_method {
                entry.insights = body::analyze_body(child, source, lang);
            }
            entries.push(entry);
        }
```

Update the two call sites of `build_skeleton`:

In `index_source` (line 420):
```rust
    Ok(build_skeleton(root, source, extractor, lang))
```

In `extract_all` (line 434):
```rust
    let skeleton = build_skeleton(root, source, extractor, lang);
```

- [ ] **Step 2: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1`
Expected: Compiles successfully.

- [ ] **Step 3: Run all tests to verify no regression**

Run: `cargo test 2>&1`
Expected: All tests pass. Existing skeleton tests now include insight output (empty for simple test cases, may include data for more complex fixtures).

- [ ] **Step 4: Commit**

```bash
git add src/index/mod.rs
git commit -m "feat: wire analyze_body into build_skeleton for top-level functions"
```

### Task 9: Wire method-level insights into Rust extractor

**Files:**
- Modify: `src/index/languages/rust.rs:129-153` (extract_methods)

- [ ] **Step 1: Add import and modify extract_methods**

Add import at top of `rust.rs`:

```rust
use crate::index::body;
```

Modify `extract_methods` (line 129-153). After pushing each method signature string, analyze the body and append insight lines. Replace:

```rust
            let lr = line_range(child.start_position().row + 1, child.end_position().row + 1);
            if include_vis {
                let vis = vis_prefix(child, source);
                methods.push(format!("{} {lr}", prefixed(vis, format_args!("{sig}"))));
            } else {
                methods.push(format!("{sig} {lr}"));
            }
```

With:

```rust
            let lr = line_range(child.start_position().row + 1, child.end_position().row + 1);
            if include_vis {
                let vis = vis_prefix(child, source);
                methods.push(format!("{} {lr}", prefixed(vis, format_args!("{sig}"))));
            } else {
                methods.push(format!("{sig} {lr}"));
            }
            // Append body insights for this method
            let insights = body::analyze_body(child, source, crate::index::Language::Rust);
            for line in insights.format_lines() {
                methods.push(format!("  {line}"));
            }
```

- [ ] **Step 2: Run existing Rust tests**

Run: `cargo test --lib -- rust --nocapture 2>&1`
Expected: All pass (existing tests may show new insight lines in output — verify they don't break assertions).

- [ ] **Step 3: Commit**

```bash
git add src/index/languages/rust.rs
git commit -m "feat: add method-level body insights to Rust extractor"
```

### Task 10: Wire method-level insights into Python extractor

**Files:**
- Modify: `src/index/languages/python.rs:22-71` (extract_class)

- [ ] **Step 1: Add import and modify extract_class**

Add import at top of `python.rs`:

```rust
use crate::index::body;
```

In `extract_class` (line 22-71), after the method signature is pushed (line 64), add insight lines. Replace:

```rust
                methods.push(format!("{fn_name}{params}{ret_str} {lr}"));
```

With:

```rust
                methods.push(format!("{fn_name}{params}{ret_str} {lr}"));
                // Append body insights for this method
                let insights = body::analyze_body(fn_node, source, crate::index::Language::Python);
                for line in insights.format_lines() {
                    methods.push(format!("  {line}"));
                }
```

- [ ] **Step 2: Run existing Python tests**

Run: `cargo test --lib -- python --nocapture 2>&1`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add src/index/languages/python.rs
git commit -m "feat: add method-level body insights to Python extractor"
```

### Task 11: Wire method-level insights into TypeScript extractor

**Files:**
- Modify: `src/index/languages/typescript.rs:38-73` (extract_class)

- [ ] **Step 1: Add import and modify extract_class**

Add import at top of `typescript.rs`:

```rust
use crate::index::body;
```

In `extract_class` (line 38-73), after the method line is pushed (line 63), add insight lines. Replace:

```rust
                    methods.push(format!("{mn}{params_str}{ret_str} {lr}"));
```

With:

```rust
                    methods.push(format!("{mn}{params_str}{ret_str} {lr}"));
                    // Append body insights for this method
                    if child.kind() == "method_definition" {
                        let insights = body::analyze_body(child, source, crate::index::Language::TypeScript);
                        for line in insights.format_lines() {
                            methods.push(format!("  {line}"));
                        }
                    }
```

Note: Only analyze `method_definition` nodes, not `public_field_definition` which have no callable body.

- [ ] **Step 2: Run existing TypeScript tests**

Run: `cargo test --lib -- typescript --nocapture 2>&1`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add src/index/languages/typescript.rs
git commit -m "feat: add method-level body insights to TypeScript extractor"
```

### Task 12: Wire method-level insights into Java extractor

**Files:**
- Modify: `src/index/languages/java.rs:86-114` (extract_class_body)

- [ ] **Step 1: Add import and modify extract_class_body**

Add import at top of `java.rs`:

```rust
use crate::index::body;
```

In `extract_class_body` (line 86-114), after the method signature is pushed (line 98), add insight lines. Replace:

```rust
                "method_declaration" | "constructor_declaration" => {
                    let sig = self.method_signature(child, source);
                    let lr =
                        line_range(child.start_position().row + 1, child.end_position().row + 1);
                    members.push(format!("{sig} {lr}"));
                }
```

With:

```rust
                "method_declaration" | "constructor_declaration" => {
                    let sig = self.method_signature(child, source);
                    let lr =
                        line_range(child.start_position().row + 1, child.end_position().row + 1);
                    members.push(format!("{sig} {lr}"));
                    // Append body insights for this method
                    let insights = body::analyze_body(child, source, crate::index::Language::Java);
                    for line in insights.format_lines() {
                        members.push(format!("  {line}"));
                    }
                }
```

- [ ] **Step 2: Run existing Java tests**

Run: `cargo test --lib -- java --nocapture 2>&1`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add src/index/languages/java.rs
git commit -m "feat: add method-level body insights to Java extractor"
```

---

## Chunk 6: Golden Snapshot Tests and Regression Verification

### Task 13: Golden snapshot tests for analyze_body

**Files:**
- Modify: `src/index/body.rs` (add golden tests)

- [ ] **Step 1: Write Rust golden snapshot test**

```rust
#[test]
fn test_golden_rust() {
    let src = r#"
fn handler(cmd: &str) -> Result<(), AppError> {
    let config = load_config()?;
    let db = connect()?;
    match cmd {
        "create" => create(&db),
        "delete" => delete(&db),
        "list" => list(&db),
        _ => return Err(AppError::UnknownCommand),
    }
    Ok(())
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::Rust);
    let lines = insights.format_lines();
    // Note: Err() and Ok() appear as calls too, since they are call_expressions
    assert_eq!(lines, vec![
        "→ calls: Err, Ok, connect, create, delete, list, load_config",
        "→ match: cmd → \"create\", \"delete\", \"list\", _",
        "→ errors: AppError::UnknownCommand, 2× ?",
    ]);
}
```

- [ ] **Step 2: Write TypeScript golden snapshot test**

```rust
#[test]
fn test_golden_typescript() {
    let src = r#"
function processEvent(event: Event): void {
    validate(event);
    const result = transform(event.data);
    switch (event.type) {
        case "click":
            handleClick(result);
            break;
        case "hover":
            handleHover(result);
            break;
        default:
            throw new UnhandledEventError("unknown");
    }
    log(result);
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::TypeScript);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::TypeScript);
    let lines = insights.format_lines();
    assert_eq!(lines, vec![
        "→ calls: handleClick, handleHover, log, transform, validate",
        "→ match: event.type → \"click\", \"hover\", default",
        "→ errors: UnhandledEventError",
    ]);
}
```

- [ ] **Step 3: Write Python golden snapshot test**

```rust
#[test]
fn test_golden_python() {
    let src = r#"
def process(action, data):
    validate(data)
    result = transform(data)
    if not result:
        raise ValueError("empty")
    save(result)
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Python);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::Python);
    let lines = insights.format_lines();
    assert_eq!(lines, vec![
        "→ calls: save, transform, validate",
        "→ errors: ValueError",
    ]);
}
```

- [ ] **Step 4: Write Go golden snapshot test**

```rust
#[test]
fn test_golden_go() {
    let src = r#"
package main
import "errors"
func handle(cmd string) error {
    validate(cmd)
    switch cmd {
    case "start":
        start()
    case "stop":
        stop()
    default:
        return errors.New("unknown command")
    }
    return nil
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Go);
    let root = tree.root_node();
    let mut cursor = root.walk();
    let fn_node = root.children(&mut cursor)
        .find(|c| c.kind() == "function_declaration")
        .unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::Go);
    let lines = insights.format_lines();
    assert_eq!(lines, vec![
        "→ calls: start, stop, validate",
        "→ match: cmd → \"start\", \"stop\", default",
        "→ errors: errors.New",
    ]);
}
```

- [ ] **Step 5: Write Java golden snapshot test**

```rust
#[test]
fn test_golden_java() {
    let src = r#"
class Handler {
    void handle(String cmd) {
        validate(cmd);
        switch (cmd) {
            case "start":
                start();
                break;
            case "stop":
                stop();
                break;
            default:
                throw new IllegalArgumentException("unknown");
        }
        log(cmd);
    }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Java);
    let root = tree.root_node();
    let class_node = root.child(0).unwrap();
    let body = class_node.child_by_field_name("body").unwrap();
    let mut cursor = body.walk();
    let method = body.children(&mut cursor)
        .find(|c| c.kind() == "method_declaration")
        .unwrap();
    let insights = analyze_body(method, &bytes, Language::Java);
    let lines = insights.format_lines();
    assert_eq!(lines, vec![
        "→ calls: log, start, stop, validate",
        "→ match: cmd → \"start\", \"stop\", default",
        "→ errors: IllegalArgumentException",
    ]);
}
```

- [ ] **Step 6: Run all golden tests**

Run: `cargo test --lib body::tests::test_golden -- --nocapture 2>&1`
Expected: All 5 PASS (Rust, TypeScript, Python, Go, Java).

- [ ] **Step 7: Commit**

```bash
git add src/index/body.rs
git commit -m "test: add golden snapshot tests for body insights"
```

### Task 14: Full regression test and clippy

**Files:** (none modified — verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass (52 existing + new body insight tests). Some existing `*_all_sections` tests may need minor output assertion updates if their test sources now produce insight lines.

- [ ] **Step 2: Fix any failing existing tests**

If existing tests in `src/index/mod.rs` fail because their test source code now triggers insight extraction (e.g., test functions that contain calls), update the expected output in those tests to include the new `→` lines.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy 2>&1`
Expected: No new warnings (existing `framing` warning in `main.rs` is known and acceptable).

- [ ] **Step 4: Commit any test fixes**

```bash
git add -A
git commit -m "fix: update existing test assertions for body insights output"
```

### Task 15: Manual verification with taoki's own codebase

**Files:** (none modified — verification only)

- [ ] **Step 1: Build and run index on taoki's own source**

Run: `cargo run -- --version && echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}' | cargo run 2>/dev/null | head -1`
Expected: Prints version, then an initialize response.

- [ ] **Step 2: Test index tool on a source file**

Create a test script to call the index tool:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{"path":"'$(pwd)/src/index/mod.rs'"}}}' | cargo run 2>/dev/null | tail -1 | python3 -c "import sys,json; print(json.loads(sys.stdin.read())['result']['content'][0]['text'])" 2>/dev/null | head -50
```

Expected: Skeleton output for `mod.rs` with `→ calls:`, `→ match:`, and `→ errors:` lines on function entries.

- [ ] **Step 3: Verify token overhead is within 15-25% target**

Compare the output length of `index` on the benchmark fixtures before and after. The insight lines should add roughly 15-25% more tokens, not more.

- [ ] **Step 4: Final commit if any cleanup needed**

```bash
git add -A
git commit -m "chore: cleanup after manual verification"
```
