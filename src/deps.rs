#![allow(dead_code)]
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tree_sitter::Parser;

use crate::index::{Language, find_child, node_text};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportInfo {
    pub path: String,
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

pub const DEPS_VERSION: u32 = 2;
const DEPS_FILE: &str = "deps.json";
const DEPS_DIR: &str = ".cache/taoki";

/// Extract raw imports from source. Returns Vec<(import_path, symbols)>.
pub fn extract_imports(source: &[u8], lang: Language) -> Vec<(String, Vec<String>)> {
    let mut parser = Parser::new();
    if parser.set_language(&lang.ts_language()).is_err() {
        return Vec::new();
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Vec::new(),
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
    let mut result = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "use_declaration" {
            let text = node_text(child, source);
            let cleaned = text
                .strip_prefix("use ")
                .unwrap_or(text)
                .trim_end_matches(';')
                .trim()
                .to_string();
            for path in expand_rust_use(&cleaned) {
                if !path.is_empty() {
                    result.push((path, Vec::new()));
                }
            }
        }
    }
    result
}

fn expand_rust_use(path: &str) -> Vec<String> {
    let path = path.trim();
    let (start, end) = match find_top_level_braces(path) {
        Some(r) => r,
        None => return vec![path.to_string()],
    };

    let prefix = path[..start].trim().trim_end_matches("::").to_string();
    let inner = path[start + 1..end].trim();

    let mut out = Vec::new();
    for item in split_top_level_items(inner) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if item == "self" {
            if !prefix.is_empty() {
                out.push(prefix.clone());
            }
            continue;
        }
        let combined = if item.starts_with("self::") {
            let rest = item.trim_start_matches("self::");
            join_prefix(&prefix, rest)
        } else if item.contains('{') {
            let nested = join_prefix(&prefix, item);
            out.extend(expand_rust_use(&nested));
            continue;
        } else {
            join_prefix(&prefix, item)
        };
        if !combined.is_empty() {
            out.push(combined);
        }
    }

    out
}

fn find_top_level_braces(s: &str) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut start = None;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return start.map(|s| (s, i));
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level_items(s: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(s[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    if start <= s.len() {
        items.push(s[start..].to_string());
    }
    items
}

fn join_prefix(prefix: &str, item: &str) -> String {
    if prefix.is_empty() {
        item.trim().to_string()
    } else if item.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}::{item}")
    }
}

fn extract_python_imports(root: tree_sitter::Node, source: &[u8]) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "import_statement" => {
                let text = node_text(child, source);
                let cleaned = text
                    .strip_prefix("import ")
                    .unwrap_or(text)
                    .trim()
                    .to_string();
                if !cleaned.is_empty() {
                    result.push((cleaned, Vec::new()));
                }
            }
            "import_from_statement" => {
                // "from X import Y, Z"
                let text = node_text(child, source);
                // Extract module name: between "from " and " import"
                if let Some(rest) = text.strip_prefix("from ") {
                    if let Some(idx) = rest.find(" import ") {
                        let module = rest[..idx].trim().to_string();
                        let symbols_str = rest[idx + " import ".len()..].trim();
                        let symbols: Vec<String> = symbols_str
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if !module.is_empty() {
                            result.push((module, symbols));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    result
}

fn extract_ts_imports(root: tree_sitter::Node, source: &[u8]) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "import_statement" {
            // Get the source (module path)
            let src_node = child.child_by_field_name("source");
            let import_path = if let Some(src) = src_node {
                let raw = node_text(src, source);
                raw.trim_matches(|c| c == '\'' || c == '"').to_string()
            } else {
                continue;
            };

            // Extract named symbols from import_clause
            let mut symbols = Vec::new();
            if let Some(clause) = find_child(child, "import_clause") {
                // Look for named_imports inside import_clause
                if let Some(named) = find_child(clause, "named_imports") {
                    let mut nc = named.walk();
                    for item in named.children(&mut nc) {
                        if item.kind() == "import_specifier" {
                            if let Some(name_node) = item.child_by_field_name("name") {
                                symbols.push(node_text(name_node, source).to_string());
                            } else {
                                // Fallback: first child
                                if let Some(first) = item.child(0) {
                                    symbols.push(node_text(first, source).to_string());
                                }
                            }
                        }
                    }
                }
            }

            if !import_path.is_empty() {
                result.push((import_path, symbols));
            }
        }
    }
    result
}

fn extract_go_imports(root: tree_sitter::Node, source: &[u8]) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "import_declaration" {
            if let Some(spec_list) = find_child(child, "import_spec_list") {
                let mut sc = spec_list.walk();
                for spec in spec_list.children(&mut sc) {
                    if spec.kind() == "import_spec" {
                        let path = spec
                            .child_by_field_name("path")
                            .map(|n| node_text(n, source).trim_matches('"').to_string())
                            .unwrap_or_default();
                        if !path.is_empty() {
                            result.push((path, Vec::new()));
                        }
                    }
                }
            } else if let Some(spec) = find_child(child, "import_spec") {
                let path = spec
                    .child_by_field_name("path")
                    .map(|n| node_text(n, source).trim_matches('"').to_string())
                    .unwrap_or_default();
                if !path.is_empty() {
                    result.push((path, Vec::new()));
                }
            }
        }
    }
    result
}

fn extract_java_imports(root: tree_sitter::Node, source: &[u8]) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "import_declaration" {
            let text = node_text(child, source);
            let cleaned = text
                .strip_prefix("import ")
                .unwrap_or(text)
                .trim_end_matches(';')
                .trim()
                .to_string();
            if !cleaned.is_empty() {
                let cleaned = cleaned.strip_prefix("static ").unwrap_or(&cleaned).to_string();
                if let Some(dot_pos) = cleaned.rfind('.') {
                    let symbol = cleaned[dot_pos + 1..].to_string();
                    result.push((cleaned, vec![symbol]));
                } else {
                    result.push((cleaned, Vec::new()));
                }
            }
        }
    }
    result
}

/// Resolve an import path to a file in `all_files` (relative paths as strings).
/// Returns Some(relative_path) if resolved to an internal file, None for external.
pub fn resolve_import(
    import_path: &str,
    lang: Language,
    current_file: &str,
    all_files: &[String],
    crate_map: Option<&HashMap<String, PathBuf>>,
) -> Option<String> {
    match lang {
        Language::Rust => match crate_map {
            Some(map) if !map.is_empty() => {
                resolve_rust_workspace(import_path, current_file, all_files, map)
            }
            _ => resolve_rust(import_path, all_files),
        },
        Language::Python => resolve_python(import_path, current_file, all_files),
        Language::TypeScript | Language::JavaScript => {
            resolve_ts(import_path, current_file, all_files)
        }
        Language::Go => None,
        Language::Java => resolve_java(import_path, all_files),
    }
}

fn resolve_rust(import_path: &str, all_files: &[String]) -> Option<String> {
    // Only resolve crate:: imports
    let rest = import_path.strip_prefix("crate::")?;

    // Convert :: to / and try progressively shorter paths (last segments may be symbols)
    let parts: Vec<&str> = rest.split("::").collect();

    for take in (1..=parts.len()).rev() {
        let path_str = parts[..take].join("/");
        let candidates = [
            format!("src/{path_str}.rs"),
            format!("src/{path_str}/mod.rs"),
            format!("{path_str}.rs"),
            format!("{path_str}/mod.rs"),
        ];
        for candidate in &candidates {
            if all_files.iter().any(|f| f == candidate) {
                return Some(candidate.clone());
            }
        }
    }
    None
}

fn resolve_rust_workspace(
    import_path: &str,
    current_file: &str,
    all_files: &[String],
    crate_map: &HashMap<String, PathBuf>,
) -> Option<String> {
    if let Some(rest) = import_path.strip_prefix("crate::") {
        // crate:: import — resolve relative to the current file's crate
        let crate_root = find_crate_root(current_file, crate_map);
        let base = crate_root
            .map(|(_, dir)| dir.to_path_buf())
            .unwrap_or_default();
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
fn resolve_within_crate(rest: &str, base: &Path, all_files: &[String]) -> Option<String> {
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

fn resolve_python(import_path: &str, current_file: &str, all_files: &[String]) -> Option<String> {
    if import_path.starts_with('.') {
        // Relative import
        let current_dir = Path::new(current_file).parent().unwrap_or(Path::new(""));
        // Strip leading dots to determine how many levels up
        let dots = import_path.chars().take_while(|c| *c == '.').count();
        let module_part = &import_path[dots..];

        // Walk up `dots - 1` levels from current_dir
        let mut base = current_dir.to_path_buf();
        for _ in 1..dots {
            base = base.parent().unwrap_or(Path::new("")).to_path_buf();
        }

        let rel_path = if module_part.is_empty() {
            base
        } else {
            base.join(module_part.replace('.', "/"))
        };

        let candidates = [
            format!("{}.py", rel_path.display()),
            format!("{}/__init__.py", rel_path.display()),
        ];
        for candidate in &candidates {
            let normalized = candidate.replace('\\', "/");
            if all_files.iter().any(|f| f == &normalized) {
                return Some(normalized);
            }
        }
        None
    } else {
        // Absolute import: replace . with /
        let path_str = import_path.replace('.', "/");
        let candidates = [
            format!("{path_str}.py"),
            format!("{path_str}/__init__.py"),
        ];
        for candidate in &candidates {
            if all_files.iter().any(|f| f == candidate) {
                return Some(candidate.clone());
            }
        }
        None
    }
}

fn resolve_ts(import_path: &str, current_file: &str, all_files: &[String]) -> Option<String> {
    // Only resolve relative imports
    if !import_path.starts_with("./") && !import_path.starts_with("../") {
        return None;
    }

    let current_dir = Path::new(current_file).parent().unwrap_or(Path::new(""));
    let joined = current_dir.join(import_path);
    let normalized = normalize_path(&joined);

    let extensions = [
        "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "d.ts", "d.tsx", "d.mts", "d.cts",
    ];

    // Try with each extension appended
    for ext in &extensions {
        let candidate = normalized.with_extension(ext);
        let candidate_str = candidate.to_string_lossy().replace('\\', "/");
        if all_files.iter().any(|f| f == &candidate_str) {
            return Some(candidate_str);
        }
    }

    // Try as directory with index file
    for ext in &extensions {
        let candidate = normalized.join(format!("index.{ext}"));
        let candidate_str = candidate.to_string_lossy().replace('\\', "/");
        if all_files.iter().any(|f| f == &candidate_str) {
            return Some(candidate_str);
        }
    }

    None
}

fn resolve_java(import_path: &str, all_files: &[String]) -> Option<String> {
    let prefixes = ["", "src/main/java/", "src/"];
    let path_str = import_path.replace('.', "/");
    for prefix in &prefixes {
        let candidate = format!("{prefix}{path_str}.java");
        if all_files.iter().any(|f| f == &candidate) {
            return Some(candidate);
        }
    }

    if let Some(dot_pos) = import_path.rfind('.') {
        let module = &import_path[..dot_pos];
        let module_path = module.replace('.', "/");
        for prefix in &prefixes {
            let candidate = format!("{prefix}{module_path}.java");
            if all_files.iter().any(|f| f == &candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                components.pop();
            }
            Component::CurDir => {}
            _ => {
                components.push(component);
            }
        }
    }
    components.iter().collect()
}

/// Build a dependency graph from a list of files under `root`.
pub fn build_deps_graph(root: &Path, files: &[PathBuf]) -> DepsGraph {
    let mut graph: HashMap<String, FileImports> = HashMap::new();
    let crate_map = build_crate_map(root);

    // Build list of relative paths (as strings) for resolution
    let all_files: Vec<String> = files
        .iter()
        .filter_map(|p| {
            p.strip_prefix(root)
                .ok()
                .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        })
        .collect();

    for file_path in files {
        let rel = match file_path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let lang = match Language::from_extension(ext) {
            Some(l) => l,
            None => continue,
        };

        let source = match std::fs::read(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

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
            let external = resolved.is_none();
            let path = resolved.unwrap_or_else(|| import_path.clone());
            imports.push(ImportInfo {
                path,
                symbols,
                external,
            });
        }

        graph.insert(rel, FileImports { imports });
    }

    DepsGraph {
        version: DEPS_VERSION,
        graph,
    }
}

/// Query the dependency graph for a specific file.
/// Returns formatted string with depends_on, used_by, and external sections.
pub fn query_deps(graph: &DepsGraph, file: &str) -> String {
    let mut out = String::new();

    // depends_on: internal files this file imports
    let depends_on: Vec<String> = graph
        .graph
        .get(file)
        .map(|fi| {
            fi.imports
                .iter()
                .filter(|i| !i.external)
                .map(|i| i.path.clone())
                .collect()
        })
        .unwrap_or_default();

    let mut depends_on = depends_on;
    depends_on.sort();
    depends_on.dedup();

    out.push_str("depends_on:\n");
    for dep in &depends_on {
        out.push_str(&format!("  {dep}\n"));
    }

    // used_by: other files that import this file
    let mut used_by: Vec<String> = Vec::new();
    for (other_file, fi) in &graph.graph {
        if other_file == file {
            continue;
        }
        if fi
            .imports
            .iter()
            .any(|i| !i.external && i.path == file)
        {
            used_by.push(other_file.clone());
        }
    }
    used_by.sort();

    out.push_str("used_by:\n");
    for user in &used_by {
        out.push_str(&format!("  {user}\n"));
    }

    // external: deduplicated external dependencies
    let mut external: Vec<String> = graph
        .graph
        .get(file)
        .map(|fi| {
            fi.imports
                .iter()
                .filter(|i| i.external)
                .map(|i| i.path.clone())
                .collect()
        })
        .unwrap_or_default();
    external.sort();
    external.dedup();

    out.push_str("external:\n");
    for ext in &external {
        out.push_str(&format!("  {ext}\n"));
    }

    out
}

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
        Err(e) => {
            eprintln!("warning: could not open deps cache lock: {e}");
            return;
        }
    };
    if lock_file.lock_exclusive().is_err() {
        eprintln!("warning: could not lock deps cache file");
        return;
    }
    if let Ok(data) = serde_json::to_string_pretty(graph) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        } else {
            eprintln!("warning: could not write deps cache temp file");
        }
    }
    let _ = lock_file.unlock();
}

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
        let result =
            resolve_import("crate::codemap", Language::Rust, "src/mcp.rs", &all_files, None);
        assert_eq!(result, Some("src/codemap.rs".to_string()));
    }

    #[test]
    fn rust_external_import_unresolved() {
        let all_files = vec!["src/main.rs".to_string()];
        let result = resolve_import(
            "serde::Serialize",
            Language::Rust,
            "src/main.rs",
            &all_files,
            None,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn python_relative_import_resolves() {
        let all_files = vec!["src/auth.py".to_string(), "src/api.py".to_string()];
        let result = resolve_import(".auth", Language::Python, "src/api.py", &all_files, None);
        assert_eq!(result, Some("src/auth.py".to_string()));
    }

    #[test]
    fn ts_relative_import_resolves() {
        let all_files = vec!["src/utils.ts".to_string(), "src/main.ts".to_string()];
        let result =
            resolve_import("./utils", Language::TypeScript, "src/main.ts", &all_files, None);
        assert_eq!(result, Some("src/utils.ts".to_string()));
    }

    #[test]
    fn ts_bare_import_is_external() {
        let all_files = vec!["src/main.ts".to_string()];
        let result =
            resolve_import("express", Language::TypeScript, "src/main.ts", &all_files, None);
        assert_eq!(result, None);
    }

    #[test]
    fn rust_grouped_use_expands() {
        let source = b"use crate::{a, b::c, self::d, e::{f, g}};\n";
        let imports = extract_imports(source, Language::Rust);
        let paths: Vec<String> = imports.into_iter().map(|(p, _)| p).collect();
        assert!(paths.contains(&"crate::a".to_string()));
        assert!(paths.contains(&"crate::b::c".to_string()));
        assert!(paths.contains(&"crate::d".to_string()));
        assert!(paths.contains(&"crate::e::f".to_string()));
        assert!(paths.contains(&"crate::e::g".to_string()));
    }

    #[test]
    fn java_import_resolves_internal_class() {
        let all_files = vec!["src/main/java/com/example/Foo.java".to_string()];
        let result = resolve_import(
            "com.example.Foo",
            Language::Java,
            "src/main/java/com/example/App.java",
            &all_files,
            None,
        );
        assert_eq!(
            result,
            Some("src/main/java/com/example/Foo.java".to_string())
        );
    }

    #[test]
    fn ts_d_ts_resolves() {
        let all_files = vec!["src/types.d.ts".to_string()];
        let result =
            resolve_import("./types", Language::TypeScript, "src/main.ts", &all_files, None);
        assert_eq!(result, Some("src/types.d.ts".to_string()));
    }

    #[test]
    fn build_crate_map_workspace() {
        let dir = tempfile::tempdir().unwrap();
        // Root workspace Cargo.toml (virtual -- no [package])
        fs::create_dir_all(dir.path().join("crate-a/src")).unwrap();
        fs::create_dir_all(dir.path().join("crate-b/src")).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crate-a\", \"crate-b\"]\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("crate-a/Cargo.toml"),
            "[package]\nname = \"crate-a\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("crate-b/Cargo.toml"),
            "[package]\nname = \"crate-b\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
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
        )
        .unwrap();
        let map = build_crate_map(dir.path());
        assert_eq!(map.len(), 1);
        assert!(
            map.contains_key("my_crate"),
            "should have my_crate, got: {:?}",
            map
        );
        assert!(!map.contains_key("my_binary"), "should not have my_binary");
    }

    #[test]
    fn build_crate_map_virtual_workspace_skipped() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\"]\n",
        )
        .unwrap();
        let map = build_crate_map(dir.path());
        // Virtual workspace has no [package], should produce empty map for this file
        assert!(!map.contains_key("workspace"));
    }

    #[test]
    fn find_crate_root_matches_longest_prefix() {
        let mut map = std::collections::HashMap::new();
        map.insert("parent_crate".to_string(), PathBuf::from("parent"));
        map.insert(
            "nested_tool".to_string(),
            PathBuf::from("parent/tools/nested"),
        );
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
        let all_files = vec!["src/mcp.rs".to_string(), "src/index/mod.rs".to_string()];
        // No crate map -- single crate
        let result =
            resolve_import("crate::mcp", Language::Rust, "src/main.rs", &all_files, None);
        assert_eq!(result, Some("src/mcp.rs".to_string()));
    }

    #[test]
    fn resolve_rust_external_with_crate_map() {
        let all_files = vec!["src/lib.rs".to_string()];
        let crate_map = std::collections::HashMap::new();
        let result = resolve_import(
            "serde::Serialize",
            Language::Rust,
            "src/lib.rs",
            &all_files,
            Some(&crate_map),
        );
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
        let result = resolve_import(
            "taoki::mcp",
            Language::Rust,
            "src/main.rs",
            &all_files,
            Some(&crate_map),
        );
        assert_eq!(result, Some("src/mcp.rs".to_string()));
    }

    #[test]
    fn find_crate_root_file_outside_workspace() {
        let mut map = std::collections::HashMap::new();
        map.insert("my_crate".to_string(), PathBuf::from("crates/my-crate"));
        let result = find_crate_root("scripts/build.rs", &map);
        assert!(
            result.is_none(),
            "file outside all crate dirs should return None"
        );
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
                    ImportInfo {
                        path: "src/utils.rs".to_string(),
                        symbols: vec![],
                        external: false,
                    },
                    ImportInfo {
                        path: "src/utils.rs".to_string(),
                        symbols: vec![],
                        external: false,
                    },
                ],
            },
        );
        let out = query_deps(&graph, "src/main.rs");
        // Should only list src/utils.rs once
        assert_eq!(
            out.matches("src/utils.rs").count(),
            1,
            "depends_on should be deduped: {}",
            out
        );
    }

    #[test]
    fn build_graph_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(
            src.join("main.rs"),
            "use crate::helper;\nfn main() { helper::run(); }\n",
        )
        .unwrap();
        fs::write(src.join("helper.rs"), "pub fn run() {}\n").unwrap();

        let files = vec![src.join("main.rs"), src.join("helper.rs")];
        let graph = build_deps_graph(dir.path(), &files);

        let main_deps = query_deps(&graph, "src/main.rs");
        assert!(
            main_deps.contains("depends_on:"),
            "main should depend on helper:\n{main_deps}"
        );
        assert!(
            main_deps.contains("src/helper.rs"),
            "main should depend on helper:\n{main_deps}"
        );

        let helper_deps = query_deps(&graph, "src/helper.rs");
        assert!(
            helper_deps.contains("used_by:"),
            "helper should be used by main:\n{helper_deps}"
        );
        assert!(
            helper_deps.contains("src/main.rs"),
            "helper should be used by main:\n{helper_deps}"
        );
    }
}
