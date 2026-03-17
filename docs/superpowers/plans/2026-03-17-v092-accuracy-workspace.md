# Taoki v0.9.2 — Accuracy & Workspace Improvements Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 6 accuracy gaps and add workspace-aware dependency resolution, released as v0.9.2.

**Architecture:** Six independent changes touching `body.rs`, `java.rs`, `rust.rs`, `mod.rs`, and `deps.rs`. Changes 5 and 6 share `body.rs` and must be done in order. All other changes are independent. Cache version bumps ensure backward compatibility.

**Tech Stack:** Rust, tree-sitter 0.26, inline `#[cfg(test)]` tests with `tempfile`

**Spec:** `docs/superpowers/specs/2026-03-17-v092-accuracy-workspace-design.md`

---

### Task 1: Test Range End-Line Fix

**Files:**
- Modify: `src/index/mod.rs:488` (`test_lines` type), `src/index/mod.rs:497` (push call), `src/index/mod.rs:376-380` (`format_skeleton` range calculation)

- [ ] **Step 1: Write the failing test**

In `src/index/mod.rs`, inside the existing `#[cfg(test)] mod tests` block (starts at line 538), add:

```rust
#[test]
fn test_range_includes_last_test_body() {
    // Two test functions — the range should span from first start to last END
    let src = r#"
def add(a, b):
    return a + b

def test_add():
    assert add(1, 2) == 3

def test_subtract():
    result = add(5, 3)
    assert result == 2
"#;
    let out = idx(src, Language::Python);
    // test_add starts at line 5, test_subtract ends at line 10
    // The tests range must include line 10, not stop at line 8
    has(&out, &["tests: [5-10]"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_range_includes_last_test_body -- --nocapture`
Expected: FAIL — the range will be `[5-8]` (start-to-start) instead of `[5-10]`

- [ ] **Step 3: Fix `build_skeleton` to push start+end pairs**

In `src/index/mod.rs`, change line 488:
```rust
// Before:
let mut test_lines: Vec<usize> = Vec::new();
// After:
let mut test_lines: Vec<(usize, usize)> = Vec::new();
```

Change line 497:
```rust
// Before:
test_lines.push(child.start_position().row + 1);
// After:
test_lines.push((child.start_position().row + 1, child.end_position().row + 1));
```

- [ ] **Step 4: Fix `format_skeleton` signature and range calculation**

In `src/index/mod.rs`, change the `format_skeleton` parameter type at line 331:
```rust
// Before:
test_lines: &[usize],
// After:
test_lines: &[(usize, usize)],
```

Then change lines 377-378:
```rust
// Before:
let min = *test_lines.iter().min().unwrap();
let max = *test_lines.iter().max().unwrap();
// After:
let min = test_lines.iter().map(|(s, _)| *s).min().unwrap();
let max = test_lines.iter().map(|(_, e)| *e).max().unwrap();
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_range_includes_last_test_body -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass (no existing test asserts a specific `tests:` range number)

- [ ] **Step 7: Commit**

```bash
git add src/index/mod.rs
git commit -m "fix: test section range includes last test's end line"
```

---

### Task 2: Java Enum Method Extraction

**Files:**
- Modify: `src/index/languages/java.rs:187-211` (`extract_enum` method)

- [ ] **Step 1: Write the failing test**

In `src/index/mod.rs`, inside the existing `#[cfg(test)] mod tests` block (starts at line 538), add (note: `java.rs` has no test module — tests go here where `index_source` and `Language` are directly available):

```rust
#[test]
fn java_enum_with_methods() {
    let src = r#"
public enum Role {
    ADMIN,
    EDITOR,
    VIEWER;

    public boolean canEdit() {
        return this == ADMIN || this == EDITOR;
    }

    public String label() {
        return name().toLowerCase();
    }
}
"#;
    let out = idx(src, Language::Java);
    // Constants should appear
    assert!(out.contains("ADMIN"), "missing ADMIN constant");
    assert!(out.contains("EDITOR"), "missing EDITOR constant");
    assert!(out.contains("VIEWER"), "missing VIEWER constant");
    // Methods should appear with signatures
    assert!(out.contains("public boolean canEdit()"), "missing canEdit method");
    assert!(out.contains("public String label()"), "missing label method");
}

#[test]
fn java_enum_no_methods_unchanged() {
    let src = r#"
public enum Color {
    RED,
    GREEN,
    BLUE;
}
"#;
    let out = idx(src, Language::Java);
    assert!(out.contains("RED"));
    assert!(out.contains("GREEN"));
    assert!(out.contains("BLUE"));
}

#[test]
fn java_enum_with_fields_and_constructor() {
    let src = r#"
public enum Planet {
    MERCURY(3.303e+23, 2.4397e6),
    VENUS(4.869e+24, 6.0518e6);

    private final double mass;
    private final double radius;

    Planet(double mass, double radius) {
        this.mass = mass;
        this.radius = radius;
    }

    public double surfaceGravity() {
        return 6.67300E-11 * mass / (radius * radius);
    }
}
"#;
    let out = idx(src, Language::Java);
    assert!(out.contains("MERCURY"), "missing MERCURY");
    assert!(out.contains("VENUS"), "missing VENUS");
    assert!(out.contains("private final double mass"), "missing mass field");
    assert!(out.contains("Planet(double mass, double radius)"), "missing constructor");
    assert!(out.contains("public double surfaceGravity()"), "missing method");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test java_enum_with_methods java_enum_no_methods_unchanged java_enum_with_fields_and_constructor -- --nocapture`
Expected: `java_enum_with_methods` and `java_enum_with_fields_and_constructor` FAIL (methods/fields missing); `java_enum_no_methods_unchanged` should PASS

- [ ] **Step 3: Rewrite `extract_enum` to capture methods and fields**

In `src/index/languages/java.rs`, replace the `extract_enum` method (lines 187-211) with:

```rust
fn extract_enum(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
    let mods = self.modifiers_text(node, source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(n, source))?;

    let label = prefixed(&mods, format_args!("enum {name}"));

    let body = node.child_by_field_name("body")?;
    let mut constants = Vec::new();
    let mut members = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        match child.kind() {
            "enum_constant" => {
                let cname = child
                    .child_by_field_name("name")
                    .map(|n| node_text(n, source))
                    .unwrap_or("_");
                constants.push(cname.to_string());
            }
            "method_declaration" | "constructor_declaration" => {
                let sig = self.method_signature(child, source);
                let lr = line_range(
                    child.start_position().row + 1,
                    child.end_position().row + 1,
                );
                members.push(format!("{sig} {lr}"));
                let insights =
                    body::analyze_body(child, source, crate::index::Language::Java);
                for line in insights.format_lines() {
                    members.push(format!("  {line}"));
                }
            }
            "field_declaration" => {
                if members.len() < FIELD_TRUNCATE_THRESHOLD {
                    let text = self.field_text(child, source);
                    let lr = line_range(
                        child.start_position().row + 1,
                        child.end_position().row + 1,
                    );
                    members.push(format!("{text} {lr}"));
                }
            }
            _ => {}
        }
    }
    constants.extend(members);

    let mut entry = SkeletonEntry::new(Section::Type, node, label);
    entry.children = constants;
    Some(entry)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test java_enum -- --nocapture`
Expected: All three tests PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/java.rs
git commit -m "fix: extract methods and fields from Java enum bodies"
```

---

### Task 3: `pub(crate)` Visibility in Code Map

**Files:**
- Modify: `src/index/languages/rust.rs:276-300` (`extract_public_api` method)

- [ ] **Step 1: Write the failing test**

In `src/index/mod.rs`, inside the existing `#[cfg(test)] mod tests` block (note: `rust.rs` has no test module — tests go here where `extract_public_api` is directly available):

```rust
#[test]
fn rust_pub_crate_in_public_api() {
    let src = r#"
pub(crate) struct Foo {
    pub(crate) field: i32,
}

pub(crate) fn bar() -> bool { true }

pub struct Visible;

pub(super) fn baz() -> i32 { 42 }

fn private_fn() {}

struct Private;
"#;
    let (types, functions) =
        extract_public_api(src.as_bytes(), Language::Rust).unwrap();
    // pub(crate) items should now appear
    assert!(types.contains(&"Foo".to_string()), "missing pub(crate) struct Foo");
    assert!(types.contains(&"Visible".to_string()), "missing pub struct Visible");
    assert!(!types.contains(&"Private".to_string()), "private struct should be excluded");
    // pub(crate) fn should appear
    assert!(functions.iter().any(|f| f.contains("bar")), "missing pub(crate) fn bar");
    // pub(super) fn should also appear
    assert!(functions.iter().any(|f| f.contains("baz")), "missing pub(super) fn baz");
    // private fn should not
    assert!(!functions.iter().any(|f| f.contains("private_fn")), "private fn should be excluded");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test rust_pub_crate_in_public_api -- --nocapture`
Expected: FAIL — `Foo` and `bar` not found (current code only matches exact `"pub"`)

- [ ] **Step 3: Change visibility check to prefix match**

In `src/index/languages/rust.rs`, in the `extract_public_api` method (line 276-300), change both visibility checks:

```rust
// Line 283 — before:
if vis_prefix(child, source) == "pub" {
// After:
if vis_prefix(child, source).starts_with("pub") {

// Line 290 — before:
if vis_prefix(child, source) == "pub" {
// After:
if vis_prefix(child, source).starts_with("pub") {
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test rust_pub_crate_in_public_api -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/rust.rs
git commit -m "feat: include pub(crate) items in code_map visible API"
```

---

### Task 4: Split Calls/Methods in Body Insights

**Files:**
- Modify: `src/index/body.rs:10` (add `MAX_METHODS` const), `src/index/body.rs:21-26` (`BodyInsights` struct), `src/index/body.rs:28-64` (`format_lines`), `src/index/body.rs:69-84` (`analyze_body`), `src/index/body.rs:194-216` (`extract_calls`)

- [ ] **Step 1: Write the failing tests**

In `src/index/body.rs`, in the `#[cfg(test)] mod tests` block, add:

```rust
#[test]
fn test_format_lines_split_calls_methods() {
    let insights = BodyInsights {
        calls: vec!["HashMap::new".into(), "Ok".into()],
        method_calls: vec!["clone".into(), "push".into()],
        ..Default::default()
    };
    let lines = insights.format_lines();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "→ calls: HashMap::new, Ok");
    assert_eq!(lines[1], "→ methods: clone, push");
}

#[test]
fn test_format_lines_calls_only_no_methods_line() {
    let insights = BodyInsights {
        calls: vec!["foo".into()],
        method_calls: vec![],
        ..Default::default()
    };
    let lines = insights.format_lines();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0], "→ calls: foo");
}

#[test]
fn test_format_lines_methods_only_no_calls_line() {
    let insights = BodyInsights {
        calls: vec![],
        method_calls: vec!["push".into()],
        ..Default::default()
    };
    let lines = insights.format_lines();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0], "→ methods: push");
}

#[test]
fn test_format_lines_methods_truncated() {
    let methods: Vec<String> = (0..12).map(|i| format!("m_{i}")).collect();
    let insights = BodyInsights {
        method_calls: methods,
        ..Default::default()
    };
    let lines = insights.format_lines();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("→ methods: "));
    assert!(lines[0].ends_with(", ..."));
    // Should contain exactly 8 method names (MAX_METHODS)
    assert_eq!(lines[0].matches(',').count(), 8); // 7 commas + ", ..."
}

#[test]
fn test_extract_calls_split_rust() {
    let src = r#"
fn example() {
    let v = Vec::new();
    v.push(1);
    v.extend(vec![2, 3]);
    let s = String::from("hi");
    s.len();
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::Rust);
    // Vec::new and String::from are free/scoped calls
    assert!(insights.calls.contains(&"Vec::new".to_string()), "Vec::new should be a call");
    assert!(insights.calls.contains(&"String::from".to_string()), "String::from should be a call");
    // push, extend, len are method calls
    assert!(insights.method_calls.contains(&"push".to_string()), "push should be a method");
    assert!(insights.method_calls.contains(&"extend".to_string()), "extend should be a method");
    assert!(insights.method_calls.contains(&"len".to_string()), "len should be a method");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_format_lines_split test_format_lines_calls_only_no_methods test_format_lines_methods_only test_format_lines_methods_truncated test_extract_calls_split_rust -- --nocapture`
Expected: FAIL — `method_calls` field doesn't exist yet

- [ ] **Step 3: Add `MAX_METHODS` constant**

In `src/index/body.rs`, after line 10 (`pub(crate) const MAX_CALLS: usize = 12;`), add:

```rust
pub(crate) const MAX_METHODS: usize = 8;
```

- [ ] **Step 4: Add `method_calls` field to `BodyInsights`**

In `src/index/body.rs`, change the `BodyInsights` struct (lines 21-26):

```rust
#[derive(Debug, Clone, Default)]
pub(crate) struct BodyInsights {
    pub(crate) calls: Vec<String>,
    pub(crate) method_calls: Vec<String>,
    pub(crate) match_arms: Vec<MatchInsight>,
    pub(crate) error_returns: Vec<String>,
    pub(crate) try_count: usize,
}
```

- [ ] **Step 5: Update `format_lines` to display both tiers**

In `src/index/body.rs`, replace the `format_lines` method (lines 29-63):

```rust
pub(crate) fn format_lines(&self) -> Vec<String> {
    let mut lines = Vec::new();

    // Free/scoped calls (domain orchestration)
    if !self.calls.is_empty() {
        let display: Vec<&str> = self.calls.iter().take(MAX_CALLS).map(|s| s.as_str()).collect();
        let suffix = if self.calls.len() > MAX_CALLS { ", ..." } else { "" };
        lines.push(format!("→ calls: {}{suffix}", display.join(", ")));
    }

    // Method calls (plumbing)
    if !self.method_calls.is_empty() {
        let display: Vec<&str> = self.method_calls.iter().take(MAX_METHODS).map(|s| s.as_str()).collect();
        let suffix = if self.method_calls.len() > MAX_METHODS { ", ..." } else { "" };
        lines.push(format!("→ methods: {}{suffix}", display.join(", ")));
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
```

- [ ] **Step 6: Update `extract_calls` to return two vecs**

In `src/index/body.rs`, replace `extract_calls` (lines 194-216):

```rust
fn extract_calls(body: Node, source: &[u8], lang: Language) -> (Vec<String>, Vec<String>) {
    let mut primary = std::collections::BTreeSet::new();
    let mut methods = std::collections::BTreeSet::new();
    walk_body(body, lang, &mut |node| {
        if is_call_node(node, lang) {
            if let Some((name, is_method)) = extract_callee_name(node, source, lang) {
                if !is_noise_call(name, lang) {
                    let truncated = truncate(name, INSIGHT_CALL_TRUNCATE);
                    if is_method {
                        methods.insert(truncated);
                    } else {
                        primary.insert(truncated);
                    }
                }
            }
        }
    });
    let calls: Vec<String> = primary.into_iter().collect();
    let method_calls: Vec<String> = methods.into_iter().collect();
    (calls, method_calls)
}
```

- [ ] **Step 7: Update `analyze_body` to wire the new return type**

In `src/index/body.rs`, change lines 75-84 in `analyze_body`:

```rust
// Before:
let calls = extract_calls(body, source, lang);
// ...
BodyInsights {
    calls,
    match_arms,
    error_returns,
    try_count,
}

// After:
let (calls, method_calls) = extract_calls(body, source, lang);
// ...
BodyInsights {
    calls,
    method_calls,
    match_arms,
    error_returns,
    try_count,
}
```

- [ ] **Step 8: Fix existing test `test_format_lines_all_sections` (body.rs:612-628)**

This test uses an exhaustive struct literal without `..Default::default()`. It will NOT compile after adding `method_calls`. Add the missing field:

```rust
// At body.rs:614, change to:
let insights = BodyInsights {
    calls: vec!["alpha".into(), "beta".into()],
    method_calls: vec![],
    match_arms: vec![MatchInsight {
        target: "x".into(),
        arms: vec!["1".into(), "2".into()],
    }],
    error_returns: vec!["Err(NotFound)".into()],
    try_count: 1,
};
```

Other existing tests use `..Default::default()` and will compile without changes.

- [ ] **Step 9: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 10: Commit**

```bash
git add src/index/body.rs
git commit -m "feat: split body insights into calls (free/scoped) and methods tiers"
```

---

### Task 5: Method Receiver Context

**Files:**
- Modify: `src/index/body.rs:119-175` (`extract_callee_name`), `src/index/body.rs:194` (`extract_calls` — update `truncate` call for owned String)

- [ ] **Step 1: Write the failing tests**

In `src/index/body.rs` tests, add:

```rust
#[test]
fn test_receiver_context_rust_chained() {
    // self.client.get() → client.get
    let src = r#"
struct S { client: Client }
impl S {
    fn example(&self) {
        self.client.get("url");
        self.items.push(1);
        plain_call();
        foo.bar();
    }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
    let root = tree.root_node();
    // Find the impl block, then the method inside
    let impl_node = root.child(1).unwrap();
    let mut cursor = impl_node.walk();
    let fn_node = impl_node.children(&mut cursor)
        .find(|c| c.kind() == "function_item")
        .unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::Rust);
    assert!(insights.calls.contains(&"plain_call".to_string()), "free call missing");
    assert!(insights.method_calls.contains(&"client.get".to_string()),
        "expected client.get, got: {:?}", insights.method_calls);
    assert!(insights.method_calls.contains(&"items.push".to_string()),
        "expected items.push, got: {:?}", insights.method_calls);
    // foo.bar() — foo is a simple identifier, no prefix
    assert!(insights.method_calls.contains(&"bar".to_string()),
        "expected bar (no prefix for simple receiver), got: {:?}", insights.method_calls);
}

#[test]
fn test_receiver_context_python() {
    let src = r#"
def example(self):
    self.client.get("url")
    items.append(1)
    free_call()
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Python);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::Python);
    assert!(insights.calls.contains(&"free_call".to_string()));
    assert!(insights.method_calls.contains(&"client.get".to_string()),
        "expected client.get, got: {:?}", insights.method_calls);
    // items.append — items is a simple identifier
    assert!(insights.method_calls.contains(&"append".to_string()),
        "expected append, got: {:?}", insights.method_calls);
}

#[test]
fn test_receiver_context_typescript() {
    let src = r#"
function example() {
    this.client.get("url");
    items.push(1);
    freeFn();
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::TypeScript);
    let root = tree.root_node();
    let fn_node = root.child(0).unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::TypeScript);
    assert!(insights.calls.contains(&"freeFn".to_string()));
    assert!(insights.method_calls.contains(&"client.get".to_string()),
        "expected client.get, got: {:?}", insights.method_calls);
    assert!(insights.method_calls.contains(&"push".to_string()),
        "expected push (simple receiver), got: {:?}", insights.method_calls);
}

#[test]
fn test_receiver_context_go() {
    let src = r#"
package main
func example() {
    resp.Body.Close()
    fmt.Println("hi")
    s.Do()
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Go);
    let root = tree.root_node();
    let mut cursor = root.walk();
    let fn_node = root.children(&mut cursor)
        .find(|c| c.kind() == "function_declaration")
        .unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::Go);
    assert!(insights.method_calls.contains(&"Body.Close".to_string()),
        "expected Body.Close, got: {:?}", insights.method_calls);
    // fmt.Println — fmt is identifier, no prefix
    assert!(insights.method_calls.contains(&"Println".to_string()),
        "expected Println, got: {:?}", insights.method_calls);
    // s.Do — s is identifier, no prefix
    assert!(insights.method_calls.contains(&"Do".to_string()),
        "expected Do, got: {:?}", insights.method_calls);
}

#[test]
fn test_receiver_context_java() {
    let src = r#"
class Example {
    void run() {
        this.service.process();
        items.add(1);
        staticCall();
    }
}
"#;
    let (tree, bytes) = parse_and_get_fn_body(src, Language::Java);
    let root = tree.root_node();
    let class_node = root.child(0).unwrap();
    let body = class_node.child_by_field_name("body").unwrap();
    let mut cursor = body.walk();
    let fn_node = body.children(&mut cursor)
        .find(|c| c.kind() == "method_declaration")
        .unwrap();
    let insights = analyze_body(fn_node, &bytes, Language::Java);
    assert!(insights.method_calls.contains(&"service.process".to_string()),
        "expected service.process, got: {:?}", insights.method_calls);
    // items.add — items is simple identifier
    assert!(insights.method_calls.contains(&"add".to_string()),
        "expected add, got: {:?}", insights.method_calls);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_receiver_context -- --nocapture`
Expected: FAIL — receiver prefix not implemented yet

- [ ] **Step 3: Update `extract_callee_name` to return `(String, bool)`**

In `src/index/body.rs`, replace the entire `extract_callee_name` function (lines 119-175):

```rust
/// Extract the callee name and whether it's a method call from a call expression node.
/// Returns `(name, is_method)` where `is_method` is true for calls on a receiver.
/// For method calls on compound receivers (field access depth >= 2), includes one level
/// of receiver context: `self.client.get()` → `"client.get"`.
fn extract_callee_name(node: Node, source: &[u8], lang: Language) -> Option<(String, bool)> {
    match lang {
        Language::Rust => {
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => Some((node_text(func, source).to_string(), false)),
                "scoped_identifier" => Some((node_text(func, source).to_string(), false)),
                "field_expression" => {
                    let field = func.child_by_field_name("field")?;
                    let value = func.child_by_field_name("value")?;
                    let prefix = if value.kind() == "field_expression" {
                        value.child_by_field_name("field").map(|f| node_text(f, source))
                    } else {
                        None
                    };
                    let name = match prefix {
                        Some(p) => format!("{}.{}", p, node_text(field, source)),
                        None => node_text(field, source).to_string(),
                    };
                    Some((name, true))
                }
                _ => Some((node_text(func, source).to_string(), false)),
            }
        }
        Language::Python => {
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => Some((node_text(func, source).to_string(), false)),
                "attribute" => {
                    let attr = func.child_by_field_name("attribute")?;
                    let obj = func.child_by_field_name("object")?;
                    let prefix = if obj.kind() == "attribute" {
                        obj.child_by_field_name("attribute").map(|a| node_text(a, source))
                    } else {
                        None
                    };
                    let name = match prefix {
                        Some(p) => format!("{}.{}", p, node_text(attr, source)),
                        None => node_text(attr, source).to_string(),
                    };
                    Some((name, true))
                }
                _ => None,
            }
        }
        Language::TypeScript | Language::JavaScript => {
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => Some((node_text(func, source).to_string(), false)),
                "member_expression" => {
                    let prop = func.child_by_field_name("property")?;
                    let obj = func.child_by_field_name("object")?;
                    let prefix = if obj.kind() == "member_expression" {
                        obj.child_by_field_name("property").map(|p| node_text(p, source))
                    } else {
                        None
                    };
                    let name = match prefix {
                        Some(p) => format!("{}.{}", p, node_text(prop, source)),
                        None => node_text(prop, source).to_string(),
                    };
                    Some((name, true))
                }
                _ => None,
            }
        }
        Language::Go => {
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => Some((node_text(func, source).to_string(), false)),
                "selector_expression" => {
                    let field = func.child_by_field_name("field")?;
                    let operand = func.child_by_field_name("operand")?;
                    let prefix = if operand.kind() == "selector_expression" {
                        operand.child_by_field_name("field").map(|f| node_text(f, source))
                    } else {
                        None
                    };
                    let name = match prefix {
                        Some(p) => format!("{}.{}", p, node_text(field, source)),
                        None => node_text(field, source).to_string(),
                    };
                    Some((name, true))
                }
                _ => None,
            }
        }
        Language::Java => {
            let call_name = node.child_by_field_name("name")?;
            let object = node.child_by_field_name("object");
            let is_method = object.is_some();
            let prefix = object.and_then(|obj| {
                if obj.kind() == "field_access" {
                    obj.child_by_field_name("field").map(|f| node_text(f, source))
                } else {
                    None
                }
            });
            let name = match prefix {
                Some(p) => format!("{}.{}", p, node_text(call_name, source)),
                None => node_text(call_name, source).to_string(),
            };
            Some((name, is_method))
        }
    }
}
```

- [ ] **Step 4: Update `extract_calls` to work with owned Strings**

In `src/index/body.rs`, in `extract_calls`, change line 200-201:

```rust
// Before:
if let Some((name, is_method)) = extract_callee_name(node, source, lang) {
    if !is_noise_call(name, lang) {
        let truncated = truncate(name, INSIGHT_CALL_TRUNCATE);
// After:
if let Some((name, is_method)) = extract_callee_name(node, source, lang) {
    if !is_noise_call(&name, lang) {
        let truncated = truncate(&name, INSIGHT_CALL_TRUNCATE);
```

Also update `is_noise_call` signature (line 190):
```rust
// Before:
fn is_noise_call(_name: &str, _lang: Language) -> bool {
// (no change needed — it already takes &str, and &String auto-derefs)
```

- [ ] **Step 5: Run receiver context tests**

Run: `cargo test test_receiver_context -- --nocapture`
Expected: All 5 language tests PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass. Some existing tests that assert specific call names may need updating if the names now include receiver prefixes. Fix any that fail.

- [ ] **Step 7: Commit**

```bash
git add src/index/body.rs
git commit -m "feat: include receiver context in method call insights"
```

---

### Task 6: Workspace-Aware Dependency Resolution

**Files:**
- Modify: `src/deps.rs:28` (`DEPS_VERSION`), `src/deps.rs:314-355` (`resolve_import`, `resolve_rust`), `src/deps.rs:478-533` (`build_deps_graph`), `src/deps.rs:535-600` (`query_deps`)

- [ ] **Step 1: Write the failing tests**

In `src/deps.rs`, inside the existing `#[cfg(test)] mod tests` block, add:

```rust
#[test]
fn build_crate_map_workspace() {
    let dir = tempfile::tempdir().unwrap();
    // Root workspace Cargo.toml (virtual — no [package])
    fs::create_dir_all(dir.path().join("crate-a/src")).unwrap();
    fs::create_dir_all(dir.path().join("crate-b/src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crate-a\", \"crate-b\"]\n",
    ).unwrap();
    fs::write(
        dir.path().join("crate-a/Cargo.toml"),
        "[package]\nname = \"crate-a\"\nversion = \"0.1.0\"\n",
    ).unwrap();
    fs::write(
        dir.path().join("crate-b/Cargo.toml"),
        "[package]\nname = \"crate-b\"\nversion = \"0.1.0\"\n",
    ).unwrap();
    let map = build_crate_map(dir.path());
    assert_eq!(map.len(), 2);
    assert!(map.contains_key("crate_a"), "missing crate_a: {:?}", map);
    assert!(map.contains_key("crate_b"), "missing crate_b: {:?}", map);
}

#[test]
fn build_crate_map_ignores_bin_name() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"my-binary\"\npath = \"src/main.rs\"\n",
    ).unwrap();
    let map = build_crate_map(dir.path());
    assert_eq!(map.len(), 1);
    assert!(map.contains_key("my_crate"), "should have my_crate, got: {:?}", map);
    assert!(!map.contains_key("my_binary"), "should not have my_binary");
}

#[test]
fn build_crate_map_virtual_workspace_skipped() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\"]\n",
    ).unwrap();
    let map = build_crate_map(dir.path());
    // Virtual workspace has no [package], should produce empty map for this file
    assert!(!map.contains_key("workspace"));
}

#[test]
fn find_crate_root_matches_longest_prefix() {
    let mut map = std::collections::HashMap::new();
    map.insert("parent_crate".to_string(), PathBuf::from("parent"));
    map.insert("nested_tool".to_string(), PathBuf::from("parent/tools/nested"));
    let result = find_crate_root("parent/tools/nested/src/main.rs", &map);
    assert_eq!(result.unwrap().0, "nested_tool");
    let result2 = find_crate_root("parent/src/lib.rs", &map);
    assert_eq!(result2.unwrap().0, "parent_crate");
}

#[test]
fn resolve_rust_workspace_crate_import() {
    let all_files = vec![
        "crate-a/src/lib.rs".to_string(),
        "crate-a/src/utils.rs".to_string(),
        "crate-b/src/lib.rs".to_string(),
        "crate-b/src/types.rs".to_string(),
    ];
    let mut crate_map = std::collections::HashMap::new();
    crate_map.insert("crate_a".to_string(), PathBuf::from("crate-a"));
    crate_map.insert("crate_b".to_string(), PathBuf::from("crate-b"));

    // crate:: import from within crate-a
    let result = resolve_import(
        "crate::utils",
        Language::Rust,
        "crate-a/src/lib.rs",
        &all_files,
        Some(&crate_map),
    );
    assert_eq!(result, Some("crate-a/src/utils.rs".to_string()));

    // Cross-crate import
    let result = resolve_import(
        "crate_b::types",
        Language::Rust,
        "crate-a/src/lib.rs",
        &all_files,
        Some(&crate_map),
    );
    assert_eq!(result, Some("crate-b/src/types.rs".to_string()));
}

#[test]
fn resolve_rust_single_crate_fallback() {
    let all_files = vec![
        "src/mcp.rs".to_string(),
        "src/index/mod.rs".to_string(),
    ];
    // No crate map — single crate
    let result = resolve_import("crate::mcp", Language::Rust, "src/main.rs", &all_files, None);
    assert_eq!(result, Some("src/mcp.rs".to_string()));
}

#[test]
fn resolve_rust_external_with_crate_map() {
    let all_files = vec!["src/lib.rs".to_string()];
    let crate_map = std::collections::HashMap::new();
    let result = resolve_import("serde::Serialize", Language::Rust, "src/lib.rs", &all_files, Some(&crate_map));
    assert_eq!(result, None);
}

#[test]
fn resolve_rust_binary_imports_own_lib() {
    let all_files = vec![
        "src/main.rs".to_string(),
        "src/lib.rs".to_string(),
        "src/mcp.rs".to_string(),
    ];
    let mut crate_map = std::collections::HashMap::new();
    crate_map.insert("taoki".to_string(), PathBuf::from(""));
    let result = resolve_import("taoki::mcp", Language::Rust, "src/main.rs", &all_files, Some(&crate_map));
    assert_eq!(result, Some("src/mcp.rs".to_string()));
}

#[test]
fn find_crate_root_file_outside_workspace() {
    let mut map = std::collections::HashMap::new();
    map.insert("my_crate".to_string(), PathBuf::from("crates/my-crate"));
    let result = find_crate_root("scripts/build.rs", &map);
    assert!(result.is_none(), "file outside all crate dirs should return None");
}

#[test]
fn query_deps_dedup_internal() {
    let mut graph = DepsGraph {
        version: DEPS_VERSION,
        graph: std::collections::HashMap::new(),
    };
    graph.graph.insert(
        "src/main.rs".to_string(),
        FileImports {
            imports: vec![
                ImportInfo { path: "src/utils.rs".to_string(), symbols: vec![], external: false },
                ImportInfo { path: "src/utils.rs".to_string(), symbols: vec![], external: false },
            ],
        },
    );
    let out = query_deps(&graph, "src/main.rs");
    // Should only list src/utils.rs once
    assert_eq!(out.matches("src/utils.rs").count(), 1,
        "depends_on should be deduped: {}", out);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test build_crate_map resolve_rust_workspace find_crate_root_matches query_deps_dedup -- --nocapture`
Expected: FAIL — functions don't exist yet, `resolve_import` has wrong signature

- [ ] **Step 3: Add `build_crate_map` function**

In `src/deps.rs`, after the `save_deps_cache` function (around line 649), add before the `#[cfg(test)]` block:

```rust
/// Build a map of crate names to their directories by scanning for Cargo.toml files.
/// Only reads the [package] section to extract the crate name, ignoring [[bin]], [lib], etc.
pub(crate) fn build_crate_map(root: &Path) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    let walker = ignore::WalkBuilder::new(root).build();
    for entry in walker.flatten() {
        if entry.file_name() != "Cargo.toml" {
            continue;
        }
        let path = entry.path();
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Some(name) = extract_package_name(&content) {
            let crate_name = name.replace('-', "_");
            let dir = path.parent().unwrap_or(Path::new(""));
            let rel = dir.strip_prefix(root).unwrap_or(dir);
            map.insert(crate_name, rel.to_path_buf());
        }
    }
    map
}

/// Extract the crate name from the [package] section of a Cargo.toml.
/// Scopes to [package] only — ignores name fields in [[bin]], [dependencies], etc.
fn extract_package_name(content: &str) -> Option<String> {
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" {
            in_package = true;
            continue;
        }
        if trimmed.starts_with('[') {
            if in_package {
                break; // Left [package] section
            }
            continue;
        }
        if in_package {
            // Match: name = "crate-name"
            if let Some(rest) = trimmed.strip_prefix("name") {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('=') {
                    let rest = rest.trim();
                    if let Some(rest) = rest.strip_prefix('"') {
                        if let Some(name) = rest.strip_suffix('"') {
                            return Some(name.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Find the crate that contains a given file path.
/// Returns (crate_name, crate_dir) for the longest-prefix match.
pub(crate) fn find_crate_root<'a>(
    file: &str,
    crate_map: &'a HashMap<String, PathBuf>,
) -> Option<(&'a str, &'a Path)> {
    let mut best: Option<(&str, &Path)> = None;
    let mut best_len = 0;
    for (name, dir) in crate_map {
        let dir_str = dir.to_string_lossy();
        let prefix = if dir_str.is_empty() {
            String::new()
        } else {
            format!("{}/", dir_str)
        };
        if prefix.is_empty() || file.starts_with(&prefix) {
            let len = prefix.len();
            if len > best_len {
                best_len = len;
                best = Some((name.as_str(), dir.as_path()));
            }
        }
    }
    best
}
```

- [ ] **Step 4: Add `resolve_rust_workspace` and update `resolve_import`**

In `src/deps.rs`, change `resolve_import` signature (line 316) and add the workspace resolver:

```rust
pub fn resolve_import(
    import_path: &str,
    lang: Language,
    current_file: &str,
    all_files: &[String],
    crate_map: Option<&HashMap<String, PathBuf>>,
) -> Option<String> {
    match lang {
        Language::Rust => {
            match crate_map {
                Some(map) if !map.is_empty() => {
                    resolve_rust_workspace(import_path, current_file, all_files, map)
                }
                _ => resolve_rust(import_path, all_files),
            }
        }
        Language::Python => resolve_python(import_path, current_file, all_files),
        Language::TypeScript | Language::JavaScript => {
            resolve_ts(import_path, current_file, all_files)
        }
        Language::Go => None,
        Language::Java => resolve_java(import_path, all_files),
    }
}
```

Add the workspace resolver after `resolve_rust`:

```rust
fn resolve_rust_workspace(
    import_path: &str,
    current_file: &str,
    all_files: &[String],
    crate_map: &HashMap<String, PathBuf>,
) -> Option<String> {
    if let Some(rest) = import_path.strip_prefix("crate::") {
        // crate:: import — resolve relative to the current file's crate
        let crate_root = find_crate_root(current_file, crate_map);
        let base = crate_root.map(|(_, dir)| dir.to_path_buf()).unwrap_or_default();
        resolve_within_crate(rest, &base, all_files)
    } else {
        // Try cross-crate: first segment might be a workspace crate
        let first_segment = import_path.split("::").next()?;
        if let Some(dir) = crate_map.get(first_segment) {
            let rest = import_path.strip_prefix(first_segment)?.strip_prefix("::")?;
            resolve_within_crate(rest, dir, all_files)
        } else {
            None // External dependency
        }
    }
}

/// Resolve a `::` path within a crate's directory.
/// Tries `{base}/src/{path}.rs` and `{base}/src/{path}/mod.rs`.
fn resolve_within_crate(
    rest: &str,
    base: &Path,
    all_files: &[String],
) -> Option<String> {
    let parts: Vec<&str> = rest.split("::").collect();
    for take in (1..=parts.len()).rev() {
        let path_str = parts[..take].join("/");
        let candidates = [
            base.join("src").join(format!("{path_str}.rs")),
            base.join("src").join(&path_str).join("mod.rs"),
        ];
        for candidate in &candidates {
            let candidate_str = candidate.to_string_lossy().replace('\\', "/");
            if all_files.iter().any(|f| f == &candidate_str) {
                return Some(candidate_str);
            }
        }
    }
    None
}
```

- [ ] **Step 5: Update `build_deps_graph` to build and pass the crate map**

In `src/deps.rs`, in `build_deps_graph` (line 479), add the crate map construction and update the resolve call:

```rust
pub fn build_deps_graph(root: &Path, files: &[PathBuf]) -> DepsGraph {
    let mut graph: HashMap<String, FileImports> = HashMap::new();
    let crate_map = build_crate_map(root);

    // ... existing all_files code unchanged ...

    for file_path in files {
        // ... existing rel/ext/lang/source code unchanged ...

        let raw_imports = extract_imports(&source, lang);
        let mut imports = Vec::new();

        for (import_path, symbols) in raw_imports {
            let resolved = resolve_import(
                &import_path,
                lang,
                &rel,
                &all_files,
                if lang == Language::Rust { Some(&crate_map) } else { None },
            );
            // ... rest unchanged ...
        }
        // ... rest unchanged ...
    }
    // ...
}
```

- [ ] **Step 6: Add dedup to `query_deps`**

In `src/deps.rs`, in `query_deps` (around line 551), after building `depends_on`:

```rust
// Before:
out.push_str("depends_on:\n");

// After:
let mut depends_on = depends_on;
depends_on.sort();
depends_on.dedup();
out.push_str("depends_on:\n");
```

(The existing code uses a `let depends_on: Vec<String>` binding which is immutable. Change to `let mut depends_on` or shadow with `let mut depends_on = depends_on;`.)

- [ ] **Step 7: Update existing test calls to `resolve_import` to pass `None`**

All existing tests that call `resolve_import` need the new 5th parameter. Add `, None` to each call:

- `rust_crate_import_resolves` (line 663)
- `rust_external_import_unresolved` (line 670)
- `python_relative_import_resolves` (line 680)
- `ts_relative_import_resolves` (line 690)
- `ts_bare_import_is_external` (line 697)
- `java_import_resolves_internal_class` (line 716)
- `ts_d_ts_resolves` (line 731)

And update any other calls to `resolve_import` in `build_deps_graph`.

- [ ] **Step 8: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 9: Commit**

```bash
git add src/deps.rs
git commit -m "feat: workspace-aware Rust dependency resolution and dedup fix"
```

---

### Task 7: Release — Version Bumps, Cache Versions, Docs

**Files:**
- Modify: `Cargo.toml:3`, `.claude-plugin/plugin.json:3`, `src/codemap.rs:37`, `src/deps.rs:28`, `CLAUDE.md`, `README.md`

- [ ] **Step 1: Bump version in `Cargo.toml`**

Change line 3: `version = "0.9.1"` → `version = "0.9.2"`

- [ ] **Step 2: Bump version in `.claude-plugin/plugin.json`**

Change line 3: `"version": "0.9.1"` → `"version": "0.9.2"`

- [ ] **Step 3: Bump `CACHE_VERSION` in `src/codemap.rs`**

Change line 37: `const CACHE_VERSION: u32 = 5;` → `const CACHE_VERSION: u32 = 6;`

- [ ] **Step 4: Bump `DEPS_VERSION` in `src/deps.rs`**

Change line 28: `pub const DEPS_VERSION: u32 = 1;` → `pub const DEPS_VERSION: u32 = 2;`

- [ ] **Step 5: Update CLAUDE.md**

Update the following sections:
- `codemap.rs` description: change "public API summary" to "visible API summary" and note that `pub(crate)` items are now included
- `index/body.rs` description: document `→ calls:` (free/scoped) and `→ methods:` (method calls with receiver context) split
- Add to Key Conventions: "Workspace-aware dependency resolution — `crate::` imports resolve within each workspace crate, cross-crate imports (`crate_name::path`) resolve via Cargo.toml scanning"
- Update body insights description to mention `MAX_METHODS` (8) and receiver context

- [ ] **Step 6: Update README.md**

Update version references from 0.9.1 to 0.9.2. Add changelog entry noting the 6 improvements.

- [ ] **Step 7: Run full test suite and clippy**

```bash
cargo test && cargo clippy
```

Expected: All tests pass, no clippy warnings

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock .claude-plugin/plugin.json src/codemap.rs src/deps.rs CLAUDE.md README.md
git commit -m "release: bump version to 0.9.2 with cache invalidation and doc updates"
```
