use tree_sitter::Node;

use crate::index::{Language, node_text, truncate};

// Truncation constants (single source of truth)
pub(crate) const INSIGHT_CALL_TRUNCATE: usize = 40;
pub(crate) const INSIGHT_MATCH_TARGET_TRUNCATE: usize = 30;
pub(crate) const INSIGHT_ARM_TRUNCATE: usize = 30;
pub(crate) const INSIGHT_ERROR_TRUNCATE: usize = 40;
pub(crate) const MAX_CALLS: usize = 12;
pub(crate) const MAX_METHODS: usize = 8;
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
    pub(crate) method_calls: Vec<String>,
    pub(crate) match_arms: Vec<MatchInsight>,
    pub(crate) error_returns: Vec<String>,
    pub(crate) try_count: usize,
}

impl BodyInsights {
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
}

/// Analyze a function/method declaration node and extract body insights.
/// Pass the function declaration node itself (e.g., `function_item`), not the body.
/// Returns empty insights if the node has no body (abstract/interface methods).
pub(crate) fn analyze_body(node: Node, source: &[u8], lang: Language) -> BodyInsights {
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return BodyInsights::default(),
    };

    let (calls, method_calls) = extract_calls(body, source, lang);
    let match_arms = extract_match_arms(body, source, lang);
    let (error_returns, try_count) = extract_error_returns(body, source, lang);

    BodyInsights {
        calls,
        method_calls,
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
        Language::Java => matches!(node.kind(), "method_declaration" | "lambda_expression" | "class_declaration" | "anonymous_class_body"),
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

/// Check if a node is a call expression for the given language.
fn is_call_node(node: Node, lang: Language) -> bool {
    match lang {
        Language::Java => node.kind() == "method_invocation",
        Language::Python => node.kind() == "call",
        _ => node.kind() == "call_expression",
    }
}

/// No name-based filtering — call prioritization is purely AST-structural.
/// Free/scoped calls appear before method calls based on call-site node kind,
/// not on what the function is named. This keeps the system universal across
/// all projects and languages.
fn is_noise_call(_name: &str, _lang: Language) -> bool {
    false
}

fn extract_calls(body: Node, source: &[u8], lang: Language) -> (Vec<String>, Vec<String>) {
    let mut primary = std::collections::BTreeSet::new();
    let mut methods = std::collections::BTreeSet::new();
    walk_body(body, lang, &mut |node| {
        if is_call_node(node, lang) {
            if let Some((name, is_method)) = extract_callee_name(node, source, lang) {
                if !is_noise_call(&name, lang) {
                    let truncated = truncate(&name, INSIGHT_CALL_TRUNCATE);
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
                // Macro invocations: panic!, todo!, unimplemented! (stdlib only)
                // Accepts unqualified (panic!) or std::/core:: namespaced (std::panic!)
                if node.kind() == "macro_invocation" {
                    if let Some(mac) = node.child(0) {
                        let full_name = node_text(mac, source);
                        let is_stdlib_macro = if full_name.contains("::") {
                            let leaf = full_name.rsplit("::").next().unwrap_or("");
                            (full_name.starts_with("std::") || full_name.starts_with("core::"))
                                && matches!(leaf, "panic" | "todo" | "unimplemented")
                        } else {
                            matches!(full_name, "panic" | "todo" | "unimplemented")
                        };
                        if is_stdlib_macro {
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
        assert!(insights.format_lines().is_empty());
    }

    #[test]
    fn test_format_lines_calls_only() {
        let insights = BodyInsights {
            calls: vec!["bar".into(), "foo".into(), "qux".into()],
            ..Default::default()
        };
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
            method_calls: vec![],
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

        let (calls, methods) = extract_calls(body, &bytes, Language::Rust);
        assert_eq!(calls, vec!["alpha", "bar::baz", "beta", "foo", "gamma"]);
        assert_eq!(methods, vec!["method"]);
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
        let (calls, methods) = extract_calls(body, &bytes, Language::Python);
        assert_eq!(calls, vec!["alpha", "beta", "foo"]);
        assert_eq!(methods, vec!["method"]);
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
        let (calls, methods) = extract_calls(body, &bytes, Language::TypeScript);
        assert_eq!(calls, vec!["alpha", "beta", "foo"]);
        assert_eq!(methods, vec!["method"]);
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
        let (calls, methods) = extract_calls(body, &bytes, Language::Go);
        assert_eq!(calls, vec!["alpha", "beta", "foo"]);
        assert_eq!(methods, vec!["Method"]);
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
        let (calls, methods) = extract_calls(method_body, &bytes, Language::Java);
        assert_eq!(calls, vec!["alpha", "beta", "foo"]);
        assert_eq!(methods, vec!["method"]);
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
    panic!("oh no");
    Ok(())
}
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let body = fn_node.child_by_field_name("body").unwrap();
        let (errors, try_count) = extract_error_returns(body, &bytes, Language::Rust);
        assert_eq!(try_count, 2);
        // Only stdlib: Err() calls and panic!/todo!/unimplemented! macros
        assert_eq!(errors, vec!["MyError::NotFound", "panic!"]);
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
            "→ calls: Err, Ok, connect, create, delete, list, load_config",
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
            "→ calls: start, stop, validate",
            "→ methods: New",
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

    // --- Rust namespaced macro test (stdlib only) ---

    #[test]
    fn test_extract_errors_rust_stdlib_macros_only() {
        let src = r#"
fn example() -> Result<()> {
    anyhow::bail!("something went wrong");
    std::panic!("fatal");
    mycrate::panic!("not stdlib");
    core::unimplemented!("todo");
    tracing::error!("not an error return");
    Ok(())
}
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let body = fn_node.child_by_field_name("body").unwrap();
        let (errors, _) = extract_error_returns(body, &bytes, Language::Rust);
        // Only stdlib (std::/core::) and unqualified macros detected
        // anyhow::bail! and mycrate::panic! are not matched
        assert_eq!(errors, vec!["std::panic!", "core::unimplemented!"]);
    }

    // --- No name-based filtering: purely structural ---

    #[test]
    fn test_extract_calls_rust_no_name_filtering() {
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
        let (calls, _methods) = extract_calls(body, &bytes, Language::Rust);
        // No name-based filtering — Ok is a free call and appears in primary tier
        assert!(calls.contains(&"bar".to_string()));
        assert!(calls.contains(&"foo".to_string()));
        assert!(calls.contains(&"Ok".to_string()), "Ok should not be filtered — no name heuristics");
    }

    // --- Call priority ordering tests ---

    #[test]
    fn test_calls_priority_rust_free_before_methods() {
        let src = r#"
fn example() {
    let x = items.clone();
    let y = items.iter().map(|i| i).collect();
    compute(x);
    Config::new();
    y.is_empty();
    process(y);
}
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let body = fn_node.child_by_field_name("body").unwrap();
        let (calls, methods) = extract_calls(body, &bytes, Language::Rust);
        assert_eq!(calls, vec!["Config::new", "compute", "process"]);
        assert_eq!(methods, vec!["clone", "collect", "is_empty", "iter", "map"]);
    }

    #[test]
    fn test_calls_priority_python_free_before_methods() {
        let src = r#"
def example(items):
    validate(items)
    result = items.filter(lambda x: x)
    result.sort()
    save(result)
    result.append(1)
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::Python);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let body = fn_node.child_by_field_name("body").unwrap();
        let (calls, methods) = extract_calls(body, &bytes, Language::Python);
        assert_eq!(calls, vec!["save", "validate"]);
        assert_eq!(methods, vec!["append", "filter", "sort"]);
    }

    #[test]
    fn test_calls_priority_ts_free_before_methods() {
        let src = r#"
function example(items: any[]) {
    validate(items);
    const result = items.filter(x => x);
    result.push(1);
    process(result);
}
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::TypeScript);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let body = fn_node.child_by_field_name("body").unwrap();
        let (calls, methods) = extract_calls(body, &bytes, Language::TypeScript);
        assert_eq!(calls, vec!["process", "validate"]);
        assert_eq!(methods, vec!["filter", "push"]);
    }

    #[test]
    fn test_calls_priority_java_free_before_methods() {
        let src = r#"
class Example {
    void example() {
        validate(data);
        obj.save();
        process(data);
        obj.notify();
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
        let (calls, methods) = extract_calls(method_body, &bytes, Language::Java);
        assert_eq!(calls, vec!["process", "validate"]);
        assert_eq!(methods, vec!["notify", "save"]);
    }

    #[test]
    fn test_calls_priority_methods_fill_remaining_budget() {
        // When free calls fill the budget, method calls are separate
        let mut fn_body = String::from("fn example() {\n");
        for i in 0..14 {
            fn_body.push_str(&format!("    free_fn_{i}();\n"));
        }
        fn_body.push_str("    x.method_a();\n");
        fn_body.push_str("    x.method_b();\n");
        fn_body.push_str("}\n");
        let (tree, bytes) = parse_and_get_fn_body(&fn_body, Language::Rust);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let body = fn_node.child_by_field_name("body").unwrap();
        let (calls, methods) = extract_calls(body, &bytes, Language::Rust);
        assert_eq!(calls.len(), 14, "all primary calls should be included");
        assert_eq!(methods.len(), 2, "method calls are now separate");
        assert!(methods.contains(&"method_a".to_string()));
        assert!(methods.contains(&"method_b".to_string()));
    }

    #[test]
    fn test_calls_priority_only_methods() {
        // When there are no free calls, all method calls appear
        let src = r#"
fn example(items: Vec<i32>) {
    items.iter().map(|x| x + 1).collect();
    items.len();
    items.is_empty();
}
"#;
        let (tree, bytes) = parse_and_get_fn_body(src, Language::Rust);
        let root = tree.root_node();
        let fn_node = root.child(0).unwrap();
        let body = fn_node.child_by_field_name("body").unwrap();
        let (calls, methods) = extract_calls(body, &bytes, Language::Rust);
        assert!(calls.is_empty(), "no free calls");
        assert_eq!(methods, vec!["collect", "is_empty", "iter", "len", "map"]);
    }

    #[test]
    fn test_extract_calls_java_local_variable_initializer() {
        let src = r#"
class Example {
    void example() {
        Foo x = make();
        var y = validate(data);
        process(x, y);
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
        assert!(calls.contains(&"make".to_string()), "should capture call in variable initializer");
        assert!(calls.contains(&"validate".to_string()), "should capture call in var initializer");
        assert!(calls.contains(&"process".to_string()));
    }

    // --- Receiver context tests ---

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
        // Find the impl block → declaration_list → function_item
        let mut cursor = root.walk();
        let impl_node = root.children(&mut cursor)
            .find(|c| c.kind() == "impl_item")
            .unwrap();
        let decl_list = impl_node.children(&mut impl_node.walk())
            .find(|c| c.kind() == "declaration_list")
            .unwrap();
        let fn_node = decl_list.children(&mut decl_list.walk())
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
}
