use tree_sitter::Node;

use crate::index::{
    LanguageExtractor, PublicApi, Section, SkeletonEntry, find_child, line_range, node_text,
    truncate,
};
use crate::index::body;

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
                // Append body insights for this method
                let insights = body::analyze_body(fn_node, source, crate::index::Language::Python);
                for line in insights.format_lines() {
                    methods.push(format!("  {line}"));
                }
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

    const NOISY_RECEIVERS: &'static [&'static str] = &[
        "console", "process", "logging", "log", "logger", "Math", "Object", "Array", "JSON",
    ];

    fn extract_expression_assignment(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let left = node.child(0)?;
        let name = node_text(left, source);
        // ALL_CAPS are handled by extract_assignment as constants
        if name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            return None;
        }
        let text = truncate(node_text(node, source).trim(), 80);
        let parent = node.parent()?;
        Some(SkeletonEntry::new(Section::Expression, parent, text.to_string()))
    }

    fn extract_dotted_call(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        // node is a "call" node inside an "expression_statement"
        let func = node.child_by_field_name("function")?;
        if func.kind() != "attribute" {
            return None; // Not a dotted call — skip bare calls like print(), run()
        }
        let receiver = func.child_by_field_name("object").map(|n| node_text(n, source)).unwrap_or("");
        if Self::NOISY_RECEIVERS.contains(&receiver) {
            return None;
        }
        let text = truncate(node_text(node, source).trim(), 80);
        let parent = node.parent()?;
        Some(SkeletonEntry::new(Section::Expression, parent, text.to_string()))
    }

    fn extract_if_name_main(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let condition = node.child_by_field_name("condition")?;
        let cond_text = node_text(condition, source);
        if cond_text.contains("__name__") && cond_text.contains("__main__") {
            Some(SkeletonEntry::new(
                Section::Expression,
                node,
                "if __name__ == \"__main__\"".to_string(),
            ))
        } else {
            None
        }
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
            "expression_statement" => {
                if let Some(child) = node.child(0) {
                    match child.kind() {
                        "assignment" => {
                            self.extract_assignment(child, source)
                                .or_else(|| self.extract_expression_assignment(child, source))
                        }
                        "call" => self.extract_dotted_call(child, source),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            "if_statement" => self.extract_if_name_main(node, source),
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

    fn extract_doc_line(&self, node: Node, source: &[u8]) -> Option<String> {
        // For decorated_definition, unwrap to inner definition
        let def_node = if node.kind() == "decorated_definition" {
            find_child(node, "function_definition")
                .or_else(|| find_child(node, "class_definition"))
                .or_else(|| find_child(node, "async_function_definition"))?
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

    #[test]
    fn python_top_level_expressions() {
        let src = "\
from flask import Flask

app = Flask(__name__)
__version__ = '1.0'
db = SQLAlchemy()

app.register_blueprint(auth_bp)
db.init_app(app)

MAX_SIZE = 100

print('hello')
run()
logging.info('started')

if __name__ == '__main__':
    app.run(debug=True)
";
        let out = index_source(src.as_bytes(), Language::Python).unwrap();

        // Named assignments (non-ALL_CAPS) should appear in exprs:
        assert!(out.contains("exprs:"), "missing exprs section in:\n{out}");
        assert!(out.contains("app = Flask(__name__)"), "missing app assignment in:\n{out}");
        assert!(out.contains("__version__"), "missing __version__ in:\n{out}");
        assert!(out.contains("db = SQLAlchemy()"), "missing db assignment in:\n{out}");

        // Dotted method calls should appear in exprs:
        assert!(out.contains("app.register_blueprint"), "missing register_blueprint in:\n{out}");
        assert!(out.contains("db.init_app"), "missing db.init_app in:\n{out}");

        // ALL_CAPS should still be in consts:
        assert!(out.contains("consts:"), "missing consts section in:\n{out}");
        assert!(out.contains("MAX_SIZE"), "missing MAX_SIZE in:\n{out}");

        // Noise should NOT appear
        assert!(!out.contains("print('hello')"), "print() should be filtered in:\n{out}");
        assert!(!out.contains("  run()"), "run() should be filtered in:\n{out}");
        assert!(!out.contains("logging.info"), "logging.info should be filtered in:\n{out}");

        // if __name__ == '__main__' should be collapsed with line range
        assert!(out.contains("if __name__"), "missing if __name__ block in:\n{out}");
    }
}
