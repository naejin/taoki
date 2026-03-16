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

// --- Body walker ---

/// Check if a node is a nested definition that should not be descended into.
/// This includes functions, closures, and class/object definitions to prevent
/// insights from leaking out of nested scopes.
fn is_nested_definition(node: Node, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(node.kind(), "function_item" | "closure_expression"),
        Language::Python => matches!(node.kind(), "function_definition" | "lambda" | "class_definition"),
        Language::TypeScript | Language::JavaScript => {
            matches!(node.kind(), "function_declaration" | "arrow_function" | "function" | "class_declaration" | "class")
        }
        Language::Go => matches!(node.kind(), "func_literal" | "function_declaration"),
        Language::Java => matches!(node.kind(), "method_declaration" | "lambda_expression" | "class_declaration" | "local_variable_declaration" | "anonymous_class_body"),
    }
}

/// Recursively walk a function body, visiting every node except those inside
/// nested function/closure definitions. The visitor sees each node once.
fn walk_body(node: Node, lang: Language, visitor: &mut impl FnMut(Node)) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_nested_definition(child, lang) {
            continue;
        }
        visitor(child);
        walk_body(child, lang, visitor);
    }
}

// --- Call extraction ---

/// Extract the callee name from a call expression node.
fn extract_callee_name<'a>(node: Node<'a>, source: &'a [u8], lang: Language) -> Option<&'a str> {
    match lang {
        Language::Rust => {
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => Some(node_text(func, source)),
                "scoped_identifier" => Some(node_text(func, source)),
                "field_expression" => {
                    let field = func.child_by_field_name("field")?;
                    Some(node_text(field, source))
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

/// Names to exclude from call lists per language (noise that duplicates other insights).
fn is_noise_call(name: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(name, "Ok" | "Err" | "Some" | "None"),
        _ => false,
    }
}

fn extract_calls(body: Node, source: &[u8], lang: Language) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    walk_body(body, lang, &mut |node| {
        if is_call_node(node, lang) {
            if let Some(name) = extract_callee_name(node, source, lang) {
                if !is_noise_call(name, lang) {
                    let truncated = truncate(name, INSIGHT_CALL_TRUNCATE);
                    seen.insert(truncated);
                }
            }
        }
    });
    seen.into_iter().collect() // BTreeSet gives sorted order
}

// --- Match/switch arm extraction ---

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

    let mut arms = Vec::new();
    let body = node.child_by_field_name("body");
    let search_node = body.unwrap_or(node);
    let mut cursor = search_node.walk();
    for child in search_node.children(&mut cursor) {
        if child.kind() == "case_clause" {
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
        match child.kind() {
            // Traditional colon-style: case X: ...
            "switch_block_statement_group" => {
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
            // Arrow-style: case X -> ...
            "switch_rule" => {
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
            _ => {}
        }
    }
    Some(MatchInsight { target, arms })
}

// --- Error return extraction ---

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
                // Also handles namespaced variants like anyhow::bail!
                if node.kind() == "macro_invocation" {
                    if let Some(mac) = node.child(0) {
                        let full_name = node_text(mac, source);
                        // Extract the leaf name for matching (e.g. "anyhow::bail" -> "bail")
                        let leaf = full_name.rsplit("::").next().unwrap_or(full_name);
                        if matches!(leaf, "bail" | "anyhow" | "panic" | "todo" | "unimplemented") {
                            let text = format!("{full_name}!");
                            if seen.insert(text.clone()) {
                                errors.push(text);
                            }
                        }
                    }
                }
            }
            Language::Python => {
                if node.kind() == "raise_statement" {
                    if let Some(expr) = node.children(&mut node.walk()).find(|c| c.is_named()) {
                        let name = match expr.kind() {
                            "call" => {
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

#[cfg(test)]
mod tests {
    use super::*;
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

    // --- walk_body tests ---

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

    // --- extract_calls tests ---

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
        assert_eq!(calls, vec!["alpha", "bar::baz", "beta", "foo", "gamma", "method"]);
    }

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
        let mut cursor = body.walk();
        let method = body.children(&mut cursor)
            .find(|c| c.kind() == "method_declaration")
            .unwrap();
        let method_body = method.child_by_field_name("body").unwrap();
        let calls = extract_calls(method_body, &bytes, Language::Java);
        assert_eq!(calls, vec!["alpha", "beta", "foo", "method"]);
    }

    // --- extract_match_arms tests ---

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

    // --- extract_error_returns tests ---

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

    // --- Golden snapshot tests ---

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
        assert_eq!(lines, vec![
            "→ calls: connect, create, delete, list, load_config",
            "→ match: cmd → \"create\", \"delete\", \"list\", _",
            "→ errors: AppError::UnknownCommand, 2× ?",
        ]);
    }

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
            "→ calls: ValueError, save, transform, validate",
            "→ errors: ValueError",
        ]);
    }

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
            "→ calls: New, start, stop, validate",
            "→ match: cmd → \"start\", \"stop\", default",
            "→ errors: errors.New",
        ]);
    }

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

    // --- Nested class/object skipping tests ---

    #[test]
    fn test_walk_body_skips_nested_class_python() {
        let src = r#"
def outer():
    foo()
    class Inner:
        def method(self):
            bar()
    baz()
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::Python);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let insights = analyze_body(fn_node, &bytes, Language::Python);
        let calls = &insights.calls;
        assert!(calls.contains(&"foo".to_string()));
        assert!(calls.contains(&"baz".to_string()));
        assert!(!calls.contains(&"bar".to_string()), "should not include calls from nested class");
    }

    #[test]
    fn test_walk_body_skips_nested_class_typescript() {
        let src = r#"
function outer() {
    foo();
    class Inner {
        method() { bar(); }
    }
    baz();
}
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::TypeScript);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let insights = analyze_body(fn_node, &bytes, Language::TypeScript);
        let calls = &insights.calls;
        assert!(calls.contains(&"foo".to_string()));
        assert!(calls.contains(&"baz".to_string()));
        assert!(!calls.contains(&"bar".to_string()), "should not include calls from nested class");
    }

    #[test]
    fn test_walk_body_skips_nested_class_java() {
        let src = r#"
class Outer {
    void outer() {
        foo();
        class Inner {
            void method() { bar(); }
        }
        baz();
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
        let calls = &insights.calls;
        assert!(calls.contains(&"foo".to_string()));
        assert!(calls.contains(&"baz".to_string()));
        assert!(!calls.contains(&"bar".to_string()), "should not include calls from nested class");
    }

    // --- Java arrow-style switch test ---

    #[test]
    fn test_extract_match_java_arrow_style() {
        let src = r#"
class Example {
    String example(int x) {
        return switch (x) {
            case 1 -> "one";
            case 2 -> "two";
            default -> "other";
        };
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
        assert!(!insights.match_arms.is_empty(), "should extract arrow-style switch arms");
        let m = &insights.match_arms[0];
        assert_eq!(m.target, "x");
        assert_eq!(m.arms, vec!["1", "2", "default"]);
    }

    // --- Rust namespaced macro test ---

    #[test]
    fn test_extract_errors_rust_namespaced_macros() {
        let src = r#"
fn example() -> Result<()> {
    anyhow::bail!("something went wrong");
    tracing::error!("not an error return");
    Ok(())
}
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let body = fn_node.child_by_field_name("body").unwrap();
        let (errors, _) = extract_error_returns(body, &bytes, Language::Rust);
        assert_eq!(errors, vec!["anyhow::bail!"]);
    }

    // --- Rust noise call filtering test ---

    #[test]
    fn test_extract_calls_rust_filters_noise() {
        let src = r#"
fn example() -> Result<(), Error> {
    let x = foo()?;
    Ok(bar())
}
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let body = fn_node.child_by_field_name("body").unwrap();
        let calls = extract_calls(body, &bytes, Language::Rust);
        assert!(calls.contains(&"bar".to_string()));
        assert!(calls.contains(&"foo".to_string()));
        assert!(!calls.contains(&"Ok".to_string()), "Ok should be filtered as noise");
    }
}
