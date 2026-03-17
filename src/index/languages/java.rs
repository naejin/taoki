use tree_sitter::Node;

use crate::index::{
    FIELD_TRUNCATE_THRESHOLD, LanguageExtractor, Section, SkeletonEntry, find_child, line_range,
    node_text, prefixed, truncate, PublicApi,
};
use crate::index::body;

pub(crate) struct JavaExtractor;

impl JavaExtractor {
    fn extract_import(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let text = node_text(node, source);
        let cleaned = text
            .strip_prefix("import ")
            .unwrap_or(text)
            .trim_end_matches(';')
            .trim();
        let normalized = cleaned.replace('.', "::");
        Some(SkeletonEntry::new(Section::Import, node, normalized))
    }

    fn extract_package(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let text = node_text(node, source);
        let cleaned = text
            .strip_prefix("package ")
            .unwrap_or(text)
            .trim_end_matches(';')
            .trim()
            .to_string();
        Some(SkeletonEntry::new(Section::Module, node, cleaned))
    }

    fn modifiers_text(&self, node: Node, source: &[u8]) -> String {
        let Some(mods) = find_child(node, "modifiers") else {
            return String::new();
        };
        let mut annotations = Vec::new();
        let mut keywords = Vec::new();
        let mut cursor = mods.walk();
        for child in mods.children(&mut cursor) {
            match child.kind() {
                "marker_annotation" | "annotation" => {
                    annotations.push(node_text(child, source));
                }
                _ => {
                    let text = node_text(child, source);
                    if matches!(
                        text,
                        "public"
                            | "private"
                            | "protected"
                            | "static"
                            | "final"
                            | "abstract"
                            | "default"
                            | "synchronized"
                    ) {
                        keywords.push(text);
                    }
                }
            }
        }
        annotations.extend(keywords);
        annotations.join(" ")
    }

    fn extract_class(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let mods = self.modifiers_text(node, source);
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let superclass = node
            .child_by_field_name("superclass")
            .and_then(|n| find_child(n, "type_identifier").or(Some(n)))
            .map(|n| format!(" extends {}", node_text(n, source)))
            .unwrap_or_default();

        let label = prefixed(&mods, format_args!("class {name}{superclass}"));

        let children = self.extract_class_body(node, source);
        let mut entry = SkeletonEntry::new(Section::Class, node, label);
        entry.children = children;
        Some(entry)
    }

    fn extract_class_body(&self, node: Node, source: &[u8]) -> Vec<String> {
        let Some(body) = node.child_by_field_name("body") else {
            return Vec::new();
        };
        let mut members = Vec::new();
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
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
        members
    }

    fn method_signature(&self, node: Node, source: &[u8]) -> String {
        let mods = self.modifiers_text(node, source);
        let ret = node
            .child_by_field_name("type")
            .map(|n| node_text(n, source))
            .unwrap_or("");
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))
            .unwrap_or("_");
        let params = node
            .child_by_field_name("parameters")
            .map(|n| node_text(n, source))
            .unwrap_or("()");
        let base = if ret.is_empty() {
            format!("{name}{params}")
        } else {
            format!("{ret} {name}{params}")
        };
        prefixed(&mods, format_args!("{base}"))
    }

    fn field_text(&self, node: Node, source: &[u8]) -> String {
        let mods = self.modifiers_text(node, source);
        let ty = node
            .child_by_field_name("type")
            .map(|n| node_text(n, source))
            .unwrap_or("_");
        let name = find_child(node, "variable_declarator")
            .and_then(|n| n.child_by_field_name("name"))
            .map(|n| node_text(n, source))
            .unwrap_or("_");
        prefixed(&mods, format_args!("{ty} {name}"))
    }

    fn extract_interface(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let mods = self.modifiers_text(node, source);
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;

        let label = prefixed(&mods, format_args!("interface {name}"));

        let children = self.extract_interface_body(node, source);
        let mut entry = SkeletonEntry::new(Section::Trait, node, label);
        entry.children = children;
        Some(entry)
    }

    fn extract_interface_body(&self, node: Node, source: &[u8]) -> Vec<String> {
        let Some(body) = node.child_by_field_name("body") else {
            return Vec::new();
        };
        let mut members = Vec::new();
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "method_declaration" || child.kind() == "constant_declaration" {
                let sig = self.method_signature(child, source);
                let lr = line_range(child.start_position().row + 1, child.end_position().row + 1);
                members.push(format!("{sig} {lr}"));
            }
        }
        members
    }

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
                "enum_body_declarations" => {
                    let mut inner_cursor = child.walk();
                    for member in child.children(&mut inner_cursor) {
                        self.collect_enum_member(member, source, &mut members);
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

    fn collect_enum_member(
        &self,
        child: Node,
        source: &[u8],
        members: &mut Vec<String>,
    ) {
        match child.kind() {
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

    fn extract_record(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let mods = self.modifiers_text(node, source);
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let params = find_child(node, "formal_parameters")
            .map(|n| node_text(n, source))
            .unwrap_or("()");

        let label = prefixed(&mods, format_args!("record {name}{params}"));

        Some(SkeletonEntry::new(Section::Type, node, label))
    }

    fn extract_annotation_type(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let mods = self.modifiers_text(node, source);
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let label = prefixed(&mods, format_args!("@interface {name}"));
        Some(SkeletonEntry::new(Section::Type, node, label))
    }
}

impl LanguageExtractor for JavaExtractor {
    fn extract_nodes(&self, node: Node, source: &[u8], _attrs: &[Node]) -> Vec<SkeletonEntry> {
        match node.kind() {
            "import_declaration" => self.extract_import(node, source).into_iter().collect(),
            "package_declaration" => self.extract_package(node, source).into_iter().collect(),
            "class_declaration" => self.extract_class(node, source).into_iter().collect(),
            "interface_declaration" => self.extract_interface(node, source).into_iter().collect(),
            "enum_declaration" => self.extract_enum(node, source).into_iter().collect(),
            "record_declaration" => self.extract_record(node, source).into_iter().collect(),
            "annotation_type_declaration" => self
                .extract_annotation_type(node, source)
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn is_test_node(&self, _node: Node, _source: &[u8], _attrs: &[Node]) -> bool {
        false
    }

    fn is_doc_comment(&self, node: Node, source: &[u8]) -> bool {
        node.kind() == "block_comment" && node_text(node, source).starts_with("/**")
    }

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
            // Skip non-comment extras; stop at non-doc comments
            // (is_extra() includes // comments in Java, which must not be skipped)
            if p.is_extra() && p.kind() != "line_comment" && p.kind() != "block_comment" {
                prev = p.prev_sibling();
                continue;
            }
            break;
        }
        None
    }

    fn is_module_doc(&self, _node: Node, _source: &[u8]) -> bool {
        false
    }

    fn extract_public_api(&self, root: Node, source: &[u8]) -> PublicApi {
        let mut types = Vec::new();
        let mut functions = Vec::new();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration" => {
                    let mods = self.modifiers_text(child, source);
                    if mods.contains("public") {
                        if let Some(name) =
                            child.child_by_field_name("name").map(|n| node_text(n, source))
                        {
                            types.push(name.to_string());
                        }
                        if let Some(body) = child.child_by_field_name("body") {
                            let mut bc = body.walk();
                            for member in body.children(&mut bc) {
                                if member.kind() == "method_declaration" {
                                    let sig = self.method_signature(member, source);
                                    if child.kind() == "interface_declaration" {
                                        functions.push(sig);
                                    } else {
                                        let mmods = self.modifiers_text(member, source);
                                        if mmods.contains("public") {
                                            functions.push(sig);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        PublicApi { types, functions }
    }
}
