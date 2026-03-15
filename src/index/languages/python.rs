use tree_sitter::Node;

use crate::index::{
    LanguageExtractor, PublicApi, Section, SkeletonEntry, find_child, line_range, node_text,
    truncate,
};

pub(crate) struct PythonExtractor;

impl PythonExtractor {
    fn extract_import(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let text = node_text(node, source);
        let cleaned = text
            .strip_prefix("import ")
            .or_else(|| text.strip_prefix("from "))
            .unwrap_or(text)
            .trim();
        let normalized = cleaned.replace(" import ", "::");
        Some(SkeletonEntry::new(Section::Import, node, normalized))
    }

    fn extract_class(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let body = node.child_by_field_name("body")?;

        let mut methods = Vec::new();
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            let method_node = match child.kind() {
                "decorated_definition" => find_child(child, "function_definition"),
                "function_definition" => Some(child),
                _ => None,
            };

            if let Some(fn_node) = method_node {
                let fn_name = fn_node
                    .child_by_field_name("name")
                    .map(|n| node_text(n, source))
                    .unwrap_or("_");
                let params = fn_node
                    .child_by_field_name("parameters")
                    .map(|n| node_text(n, source))
                    .unwrap_or("()");
                let ret = fn_node
                    .child_by_field_name("return_type")
                    .map(|n| node_text(n, source));
                let ret_str = ret.map(|r| format!(" -> {r}")).unwrap_or_default();
                let lr = line_range(
                    fn_node.start_position().row + 1,
                    fn_node.end_position().row + 1,
                );

                if child.kind() == "decorated_definition" {
                    let mut dec_cursor = child.walk();
                    for dec in child.children(&mut dec_cursor) {
                        if dec.kind() == "decorator" {
                            methods.push(node_text(dec, source).to_string());
                        }
                    }
                }

                methods.push(format!("{fn_name}{params}{ret_str} {lr}"));
            }
        }

        let mut entry = SkeletonEntry::new(Section::Class, node, name.to_string());
        entry.children = methods;
        Some(entry)
    }

    fn extract_function(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let actual = if node.kind() == "decorated_definition" {
            find_child(node, "function_definition")?
        } else {
            node
        };

        let name = actual
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        let params = actual
            .child_by_field_name("parameters")
            .map(|n| node_text(n, source))
            .unwrap_or("()");
        let ret = actual
            .child_by_field_name("return_type")
            .map(|n| node_text(n, source));
        let ret_str = ret.map(|r| format!(" -> {r}")).unwrap_or_default();

        Some(SkeletonEntry::new(
            Section::Function,
            node,
            format!("{name}{params}{ret_str}"),
        ))
    }

    fn extract_assignment(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let left = node.child(0)?;
        let name = node_text(left, source);
        if !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            return None;
        }

        let value = {
            let mut found_eq = false;
            let mut val = None;
            for i in 0..node.child_count() {
                let c = node.child(i as u32).unwrap();
                if found_eq {
                    val = Some(c);
                    break;
                }
                if node_text(c, source) == "=" {
                    found_eq = true;
                }
            }
            val.map(|n| truncate(node_text(n, source), 60))
        };

        let val_str = value.map(|v| format!(" = {v}")).unwrap_or_default();
        Some(SkeletonEntry::new(
            Section::Constant,
            node,
            format!("{name}{val_str}"),
        ))
    }
}

impl LanguageExtractor for PythonExtractor {
    fn extract_nodes(&self, node: Node, source: &[u8], _attrs: &[Node]) -> Vec<SkeletonEntry> {
        let entry = match node.kind() {
            "import_statement" | "import_from_statement" => self.extract_import(node, source),
            "class_definition" => self.extract_class(node, source),
            "function_definition" => self.extract_function(node, source),
            "decorated_definition" => {
                let inner = find_child(node, "class_definition")
                    .or_else(|| find_child(node, "function_definition"));
                match inner {
                    Some(i) if i.kind() == "class_definition" => {
                        self.extract_class(i, source).map(|mut entry| {
                            entry.line_start = node.start_position().row + 1;
                            entry
                        })
                    }
                    Some(_) => self.extract_function(node, source),
                    None => None,
                }
            }
            "expression_statement" => node
                .child(0)
                .filter(|c| c.kind() == "assignment")
                .and_then(|c| self.extract_assignment(c, source)),
            _ => None,
        };
        entry.into_iter().collect()
    }

    fn is_test_node(&self, node: Node, source: &[u8], _attrs: &[Node]) -> bool {
        match node.kind() {
            "function_definition" => {
                node.child_by_field_name("name")
                    .map(|n| node_text(n, source).starts_with("test_"))
                    .unwrap_or(false)
            }
            "decorated_definition" => {
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

    fn is_doc_comment(&self, _node: Node, _source: &[u8]) -> bool {
        false
    }

    fn is_module_doc(&self, node: Node, source: &[u8]) -> bool {
        if node.kind() != "expression_statement" {
            return false;
        }
        let Some(child) = node.child(0) else {
            return false;
        };
        child.kind() == "string" && node_text(child, source).starts_with("\"\"\"")
    }

    fn extract_public_api(&self, root: Node, source: &[u8]) -> PublicApi {
        let mut types = Vec::new();
        let mut functions = Vec::new();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "class_definition" => {
                    if let Some(name) =
                        child.child_by_field_name("name").map(|n| node_text(n, source))
                    {
                        types.push(name.to_string());
                    }
                }
                "function_definition" => {
                    if let Some(name) =
                        child.child_by_field_name("name").map(|n| node_text(n, source))
                    {
                        if !name.starts_with('_') {
                            let params = child
                                .child_by_field_name("parameters")
                                .map(|n| node_text(n, source))
                                .unwrap_or("()");
                            let ret = child
                                .child_by_field_name("return_type")
                                .map(|n| format!(" -> {}", node_text(n, source)))
                                .unwrap_or_default();
                            functions.push(format!("{name}{params}{ret}"));
                        }
                    }
                }
                "decorated_definition" => {
                    if let Some(inner) = find_child(child, "class_definition") {
                        if let Some(name) =
                            inner.child_by_field_name("name").map(|n| node_text(n, source))
                        {
                            types.push(name.to_string());
                        }
                    } else if let Some(inner) = find_child(child, "function_definition") {
                        if let Some(name) =
                            inner.child_by_field_name("name").map(|n| node_text(n, source))
                        {
                            if !name.starts_with('_') {
                                let params = inner
                                    .child_by_field_name("parameters")
                                    .map(|n| node_text(n, source))
                                    .unwrap_or("()");
                                let ret = inner
                                    .child_by_field_name("return_type")
                                    .map(|n| format!(" -> {}", node_text(n, source)))
                                    .unwrap_or_default();
                                functions.push(format!("{name}{params}{ret}"));
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
