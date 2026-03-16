use tree_sitter::Node;

use crate::index::{
    LanguageExtractor, PublicApi, Section, SkeletonEntry, find_child, line_range, node_text,
    truncate,
};
use crate::index::body;

fn ts_return_type(node: Node, source: &[u8]) -> String {
    let r = node_text(node, source);
    if r.starts_with(':') {
        r.to_string()
    } else {
        format!(": {r}")
    }
}

pub(crate) struct TsJsExtractor;

impl TsJsExtractor {
    fn extract_import(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let text = node_text(node, source);
        let cleaned = text
            .strip_prefix("import ")
            .unwrap_or(text)
            .trim_end_matches(';')
            .to_string();
        Some(SkeletonEntry::new(Section::Import, node, cleaned))
    }

    fn export_prefix(&self, node: Node) -> &'static str {
        if self.is_exported(node) {
            "export "
        } else {
            ""
        }
    }

    fn extract_class(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;

        let body = node.child_by_field_name("body")?;
        let mut methods = Vec::new();
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "method_definition" | "public_field_definition" => {
                    let mn = child
                        .child_by_field_name("name")
                        .map(|n| node_text(n, source))
                        .unwrap_or("_");
                    let params = child
                        .child_by_field_name("parameters")
                        .map(|n| node_text(n, source));
                    let ret = child
                        .child_by_field_name("return_type")
                        .map(|n| ts_return_type(n, source));
                    let params_str = params.unwrap_or_default();
                    let ret_str = ret.unwrap_or_default();
                    let lr =
                        line_range(child.start_position().row + 1, child.end_position().row + 1);
                    methods.push(format!("{mn}{params_str}{ret_str} {lr}"));
                    // Append body insights for this method
                    if child.kind() == "method_definition" {
                        let insights = body::analyze_body(child, source, crate::index::Language::TypeScript);
                        for line in insights.format_lines() {
                            methods.push(format!("  {line}"));
                        }
                    }
                }
                _ => {}
            }
        }

        let ep = self.export_prefix(node);
        let mut entry = SkeletonEntry::new(Section::Class, node, format!("{ep}{name}"));
        entry.children = methods;
        Some(entry)
    }

    fn extract_function(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let params = node
            .child_by_field_name("parameters")
            .map(|n| node_text(n, source))
            .unwrap_or("()");
        let ret_str = node
            .child_by_field_name("return_type")
            .map(|n| ts_return_type(n, source))
            .unwrap_or_default();

        let ep = self.export_prefix(node);
        Some(SkeletonEntry::new(
            Section::Function,
            node,
            format!("{ep}{name}{params}{ret_str}"),
        ))
    }

    fn extract_interface(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let body = node.child_by_field_name("body")?;

        let mut fields = Vec::new();
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "property_signature" || child.kind() == "method_signature" {
                let text = node_text(child, source)
                    .trim_end_matches([',', ';'])
                    .to_string();
                fields.push(text);
            }
        }

        let ep = self.export_prefix(node);
        let mut entry = SkeletonEntry::new(Section::Type, node, format!("{ep}interface {name}"));
        entry.children = fields;
        Some(entry)
    }

    fn extract_type_alias(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let val_str = node
            .child_by_field_name("value")
            .map(|n| format!(" = {}", truncate(node_text(n, source), 80)))
            .unwrap_or_default();
        let ep = self.export_prefix(node);
        Some(SkeletonEntry::new(
            Section::Type,
            node,
            format!("{ep}type {name}{val_str}"),
        ))
    }

    fn extract_const(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let decl = find_child(node, "variable_declarator")?;
        let name = decl
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let type_str = decl
            .child_by_field_name("type")
            .map(|n| ts_return_type(n, source))
            .unwrap_or_default();
        let val_str = decl
            .child_by_field_name("value")
            .map(|n| format!(" = {}", truncate(node_text(n, source), 60)))
            .unwrap_or_default();
        let ep = self.export_prefix(node);
        Some(SkeletonEntry::new(
            Section::Constant,
            node,
            format!("{ep}{name}{type_str}{val_str}"),
        ))
    }

    fn extract_enum(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let ep = self.export_prefix(node);
        Some(SkeletonEntry::new(
            Section::Type,
            node,
            format!("{ep}enum {name}"),
        ))
    }

    fn is_exported(&self, node: Node) -> bool {
        node.parent()
            .is_some_and(|p| p.kind() == "export_statement")
    }

    fn extract_export_statement(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "class_declaration" => return self.extract_class(child, source),
                "function_declaration" => return self.extract_function(child, source),
                "interface_declaration" => return self.extract_interface(child, source),
                "type_alias_declaration" => return self.extract_type_alias(child, source),
                "lexical_declaration" => return self.extract_lexical_declaration(child, source),
                "enum_declaration" => return self.extract_enum(child, source),
                _ => {}
            }
        }
        None
    }

    const NOISY_RECEIVERS: &'static [&'static str] = &[
        "console", "process", "logging", "log", "logger", "Math", "Object", "Array", "JSON",
    ];

    fn extract_assignment_expression(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let child = node.child(0)?;
        if child.kind() != "assignment_expression" {
            return None;
        }
        let text = truncate(node_text(child, source).trim(), 80);
        Some(SkeletonEntry::new(Section::Expression, node, text.to_string()))
    }

    fn extract_dotted_call(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let child = node.child(0)?;
        if child.kind() != "call_expression" {
            return None;
        }
        let func = child.child_by_field_name("function")?;
        if func.kind() != "member_expression" {
            return None;
        }
        let receiver = func.child_by_field_name("object").map(|n| node_text(n, source)).unwrap_or("");
        if Self::NOISY_RECEIVERS.contains(&receiver) {
            return None;
        }
        let text = truncate(node_text(child, source).trim(), 80);
        Some(SkeletonEntry::new(Section::Expression, node, text.to_string()))
    }

    fn extract_export_default(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let mut cursor = node.walk();
        let has_default = node.children(&mut cursor).any(|c| node_text(c, source) == "default");
        if !has_default {
            return None;
        }
        let text = truncate(node_text(node, source).trim().trim_end_matches(';'), 80);
        Some(SkeletonEntry::new(Section::Expression, node, text.to_string()))
    }

    fn extract_lexical_declaration(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let kind_text = node.child(0).map(|n| node_text(n, source)).unwrap_or("");
        if kind_text == "const" {
            self.extract_const(node, source)
        } else {
            // let/var declarations
            let decl = find_child(node, "variable_declarator")?;
            let name = decl.child_by_field_name("name").map(|n| node_text(n, source))?;
            let type_str = decl.child_by_field_name("type").map(|n| ts_return_type(n, source)).unwrap_or_default();
            let val_str = decl.child_by_field_name("value")
                .map(|n| format!(" = {}", truncate(node_text(n, source), 60)))
                .unwrap_or_default();
            let ep = self.export_prefix(node);
            Some(SkeletonEntry::new(
                Section::Expression,
                node,
                format!("{ep}{kind_text} {name}{type_str}{val_str}"),
            ))
        }
    }
}

impl LanguageExtractor for TsJsExtractor {
    fn extract_nodes(&self, node: Node, source: &[u8], _attrs: &[Node]) -> Vec<SkeletonEntry> {
        let entry = match node.kind() {
            "import_statement" => self.extract_import(node, source),
            "class_declaration" => self.extract_class(node, source),
            "function_declaration" => self.extract_function(node, source),
            "interface_declaration" => self.extract_interface(node, source),
            "type_alias_declaration" => self.extract_type_alias(node, source),
            "enum_declaration" => self.extract_enum(node, source),
            "lexical_declaration" => self.extract_lexical_declaration(node, source),
            "expression_statement" => {
                self.extract_assignment_expression(node, source)
                    .or_else(|| self.extract_dotted_call(node, source))
            }
            "variable_declaration" => self.extract_lexical_declaration(node, source),
            "export_statement" => {
                self.extract_export_statement(node, source)
                    .or_else(|| self.extract_export_default(node, source))
            }
            _ => None,
        };
        entry.into_iter().collect()
    }

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

    fn is_doc_comment(&self, node: Node, source: &[u8]) -> bool {
        node.kind() == "comment" && node_text(node, source).starts_with("/**")
    }

    fn is_module_doc(&self, _node: Node, _source: &[u8]) -> bool {
        false
    }

    fn extract_public_api(&self, root: Node, source: &[u8]) -> PublicApi {
        let mut types = Vec::new();
        let mut functions = Vec::new();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "export_statement" {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    match inner.kind() {
                        "class_declaration"
                        | "interface_declaration"
                        | "type_alias_declaration"
                        | "enum_declaration" => {
                            if let Some(name) = inner
                                .child_by_field_name("name")
                                .map(|n| node_text(n, source))
                            {
                                types.push(name.to_string());
                            }
                        }
                        "function_declaration" => {
                            if let Some(name) = inner
                                .child_by_field_name("name")
                                .map(|n| node_text(n, source))
                            {
                                let params = inner
                                    .child_by_field_name("parameters")
                                    .map(|n| node_text(n, source))
                                    .unwrap_or("()");
                                let ret = inner
                                    .child_by_field_name("return_type")
                                    .map(|n| ts_return_type(n, source))
                                    .unwrap_or_default();
                                functions.push(format!("{name}{params}{ret}"));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        PublicApi { types, functions }
    }
}

#[cfg(test)]
mod tests {
    use crate::index::{Language, index_source};

    #[test]
    fn ts_top_level_expressions() {
        let src = "\
import express from 'express';

const app = express();
let server;
var config = {};

module.exports = { app };
exports.handler = handler;

app.use(middleware());
app.get('/api', handler);
router.post('/login', auth);

console.log('starting');
process.exit(1);

export default class App {}
";
        let out = index_source(src.as_bytes(), Language::TypeScript).unwrap();

        // const should be in consts:
        assert!(out.contains("consts:"), "missing consts section in:\n{out}");
        assert!(out.contains("app"), "missing app const in:\n{out}");

        // let/var should be in exprs:
        assert!(out.contains("exprs:"), "missing exprs section in:\n{out}");
        assert!(out.contains("server"), "missing let server in:\n{out}");
        assert!(out.contains("config"), "missing var config in:\n{out}");

        // Assignment expressions should be in exprs:
        assert!(out.contains("module.exports"), "missing module.exports in:\n{out}");
        assert!(out.contains("exports.handler"), "missing exports.handler in:\n{out}");

        // Dotted method calls should be in exprs:
        assert!(out.contains("app.use"), "missing app.use in:\n{out}");
        assert!(out.contains("app.get"), "missing app.get in:\n{out}");
        assert!(out.contains("router.post"), "missing router.post in:\n{out}");

        // Noise should NOT appear
        assert!(!out.contains("console.log"), "console.log should be filtered in:\n{out}");
        assert!(!out.contains("process.exit"), "process.exit should be filtered in:\n{out}");

        // export default should appear
        assert!(out.contains("App"), "missing export default class App in:\n{out}");
    }

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
