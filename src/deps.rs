#![allow(dead_code)]
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tree_sitter::Parser;

use crate::cache::CACHE_VERSION;
use crate::index::{Language, find_child, node_text};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportInfo {
    pub path: String,
    pub symbols: Vec<String>,
    pub external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileImports {
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub raw_imports: Vec<(String, Vec<String>)>,
    pub imports: Vec<ImportInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DepsGraph {
    pub version: u32,
    #[serde(default)]
    pub fingerprint: String,
    pub graph: HashMap<String, FileImports>,
}

const DEPS_FILE: &str = "deps.json";
const DEPS_DIR: &str = ".cache/taoki";
const SYMBOL_TRUNCATE_THRESHOLD: usize = 6;

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
    go_module_map: Option<&HashMap<String, PathBuf>>,
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
        Language::Go => match go_module_map {
            Some(map) if !map.is_empty() => resolve_go(import_path, all_files, map),
            _ => None,
        },
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
/// Tries `{base}/src/{path}.rs`, `{base}/src/{path}/mod.rs`,
/// `{base}/{path}.rs`, and `{base}/{path}/mod.rs` (for crates not using `src/` layout).
fn resolve_within_crate(rest: &str, base: &Path, all_files: &[String]) -> Option<String> {
    let parts: Vec<&str> = rest.split("::").collect();
    for take in (1..=parts.len()).rev() {
        let path_str = parts[..take].join("/");
        let candidates = [
            base.join("src").join(format!("{path_str}.rs")),
            base.join("src").join(&path_str).join("mod.rs"),
            base.join(format!("{path_str}.rs")),
            base.join(&path_str).join("mod.rs"),
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
        // Absolute import: discover source root via __init__.py — the Python-defined
        // package marker. No hardcoded directory names.
        let path_str = import_path.replace('.', "/");
        let top_level = import_path.split('.').next()?;
        let init_pattern = format!("{top_level}/__init__.py");

        // Find source root by locating the top-level package's __init__.py
        for file in all_files {
            if file == &init_pattern || file.ends_with(&format!("/{init_pattern}")) {
                let prefix = &file[..file.len() - init_pattern.len()];
                let candidates = [
                    format!("{prefix}{path_str}.py"),
                    format!("{prefix}{path_str}/__init__.py"),
                ];
                for candidate in &candidates {
                    if all_files.iter().any(|f| f == candidate) {
                        return Some(candidate.clone());
                    }
                }
            }
        }

        // Fallback: flat layout for namespace packages (no __init__.py)
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
    // Handle wildcard imports: "org.springframework.boot.*" → find any file in that package
    if let Some(pkg) = import_path.strip_suffix(".*") {
        let dir_suffix = pkg.replace('.', "/") + "/";
        return all_files
            .iter()
            .find(|f| {
                if !f.ends_with(".java") {
                    return false;
                }
                // Check if file is in the target directory (with any source root prefix)
                if let Some(pos) = f.rfind(&dir_suffix) {
                    // Boundary check: must be preceded by "/" or be at start
                    (pos == 0 || f.as_bytes()[pos - 1] == b'/')
                        && !f[pos + dir_suffix.len()..].contains('/')
                } else {
                    false
                }
            })
            .cloned();
    }

    // Progressive suffix matching: try full path, then strip segments from the end
    // (handles normal imports, static imports, inner classes)
    let segments: Vec<&str> = import_path.split('.').collect();
    for take in (1..=segments.len()).rev() {
        let suffix = segments[..take].join("/") + ".java";
        // Check with "/" boundary to prevent false substring matches
        let with_sep = format!("/{suffix}");
        if let Some(found) = all_files
            .iter()
            .find(|f| f.ends_with(&with_sep) || *f == &suffix)
        {
            return Some(found.clone());
        }
    }
    None
}

fn resolve_go(
    import_path: &str,
    all_files: &[String],
    module_map: &HashMap<String, PathBuf>,
) -> Option<String> {
    for (module_name, module_dir) in module_map {
        let rest = if import_path == module_name {
            ""
        } else if let Some(r) = import_path.strip_prefix(&format!("{module_name}/")) {
            r
        } else {
            continue;
        };

        let pkg_dir = {
            let base = module_dir.to_string_lossy().replace('\\', "/");
            if base.is_empty() {
                rest.to_string()
            } else if rest.is_empty() {
                base
            } else {
                format!("{base}/{rest}")
            }
        };

        // Find the first non-test .go file directly inside this directory (no subdirs)
        let prefix = if pkg_dir.is_empty() {
            String::new()
        } else {
            format!("{pkg_dir}/")
        };
        if let Some(found) = all_files.iter().find(|f| {
            f.ends_with(".go")
                && !f.ends_with("_test.go")
                && f.starts_with(&prefix)
                && !f[prefix.len()..].contains('/')
        }) {
            return Some(found.clone());
        }
    }
    None
}

/// Build a map of Go module paths to their directories by scanning for go.mod files.
pub(crate) fn build_go_module_map(root: &Path) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    for entry in ignore::WalkBuilder::new(root).build().flatten() {
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false)
            && entry.file_name() == "go.mod"
        {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Some(module_name) = extract_go_module_name(&content) {
                    let dir = entry.path().parent().unwrap_or(root);
                    let rel = dir.strip_prefix(root).unwrap_or(dir);
                    map.insert(module_name, rel.to_path_buf());
                }
            }
        }
    }
    map
}

fn extract_go_module_name(content: &str) -> Option<String> {
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("module ") {
            let name = rest.trim();
            if !name.is_empty() {
                return Some(name.to_string());
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
/// Build a dependency graph from a list of files under `root`.
/// If `cache` is provided, reuses extraction results for unchanged files
/// and skips tree-sitter parsing. Re-resolves all imports when the file list
/// or workspace configuration changes (detected via fingerprint).
pub fn build_deps_graph(root: &Path, files: &[PathBuf], cache: Option<&DepsGraph>) -> DepsGraph {
    let crate_map = build_crate_map(root);
    let go_module_map = build_go_module_map(root);

    // Build list of relative paths (as strings) for resolution
    let all_files: Vec<String> = files
        .iter()
        .filter_map(|p| {
            p.strip_prefix(root)
                .ok()
                .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        })
        .collect();

    let fingerprint = crate::cache::compute_fingerprint(&all_files, &crate_map, &go_module_map);
    let fingerprint_changed = cache.is_none_or(|c| c.fingerprint != fingerprint);

    let mut graph: HashMap<String, FileImports> = HashMap::new();

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

        let content_hash = blake3::hash(&source).to_hex().to_string();

        // Check if we can reuse cached data for this file
        if let Some(cached_entry) = cache
            .and_then(|c| c.graph.get(&rel))
            .filter(|entry| entry.content_hash == content_hash)
        {
            if !fingerprint_changed {
                // Nothing changed at all — reuse everything
                graph.insert(rel, cached_entry.clone());
                continue;
            }
            // File list/config changed — re-resolve from cached raw_imports (no tree-sitter)
            let imports = resolve_raw_imports(
                &cached_entry.raw_imports, lang, &rel, &all_files, &crate_map, &go_module_map,
            );
            graph.insert(rel, FileImports {
                content_hash,
                raw_imports: cached_entry.raw_imports.clone(),
                imports,
            });
            continue;
        }

        // Content changed or new file — full extraction + resolution
        let raw_imports = extract_imports(&source, lang);
        let imports = resolve_raw_imports(
            &raw_imports, lang, &rel, &all_files, &crate_map, &go_module_map,
        );

        graph.insert(rel, FileImports {
            content_hash,
            raw_imports,
            imports,
        });
    }

    DepsGraph {
        version: CACHE_VERSION,
        fingerprint,
        graph,
    }
}

/// Resolve a list of raw imports to ImportInfo entries.
fn resolve_raw_imports(
    raw_imports: &[(String, Vec<String>)],
    lang: Language,
    current_file: &str,
    all_files: &[String],
    crate_map: &HashMap<String, PathBuf>,
    go_module_map: &HashMap<String, PathBuf>,
) -> Vec<ImportInfo> {
    let mut imports = Vec::new();
    for (import_path, symbols) in raw_imports {
        let resolved = resolve_import(
            import_path,
            lang,
            current_file,
            all_files,
            if lang == Language::Rust { Some(crate_map) } else { None },
            if lang == Language::Go { Some(go_module_map) } else { None },
        );
        let external = resolved.is_none();
        let path = resolved.unwrap_or_else(|| import_path.clone());
        imports.push(ImportInfo {
            path,
            symbols: symbols.clone(),
            external,
        });
    }
    imports
}

fn format_symbols(symbols: &[String]) -> String {
    if symbols.len() <= SYMBOL_TRUNCATE_THRESHOLD {
        symbols.join(", ")
    } else {
        let shown: Vec<&str> = symbols.iter().take(SYMBOL_TRUNCATE_THRESHOLD).map(|s| s.as_str()).collect();
        format!("{}, ... +{} more", shown.join(", "), symbols.len() - SYMBOL_TRUNCATE_THRESHOLD)
    }
}

/// Query the dependency graph for a specific file.
/// Returns formatted string with depends_on, used_by, and external sections.
pub fn query_deps(graph: &DepsGraph, file: &str, depth: u32) -> String {
    let mut out = String::new();

    // depends_on: internal files this file imports (always depth 1)
    // Deduplicate by path, merge symbols from duplicate imports
    let mut depends_map: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    if let Some(fi) = graph.graph.get(file) {
        for imp in &fi.imports {
            if !imp.external {
                let entry = depends_map.entry(imp.path.clone()).or_default();
                for sym in &imp.symbols {
                    if !entry.contains(sym) {
                        entry.push(sym.clone());
                    }
                }
            }
        }
    }

    out.push_str("depends_on:\n");
    for (path, symbols) in depends_map.iter_mut().map(|(k, v)| { v.sort(); (k, v) }) {
        if symbols.is_empty() {
            out.push_str(&format!("  {path}\n"));
        } else {
            out.push_str(&format!("  {path} ({})\n", format_symbols(symbols)));
        }
    }

    // used_by with depth expansion
    if depth > 1 {
        out.push_str(&format!("used_by (depth={depth}):\n"));
    } else {
        out.push_str("used_by:\n");
    }
    let mut visited = std::collections::HashSet::new();
    visited.insert(file.to_string());
    collect_used_by(graph, file, depth, 1, &mut visited, &mut out);

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

fn collect_used_by(
    graph: &DepsGraph,
    target: &str,
    max_depth: u32,
    current_depth: u32,
    visited: &mut std::collections::HashSet<String>,
    out: &mut String,
) {
    if current_depth > max_depth {
        return;
    }

    // Merge symbols from all imports of target per file, then sort for stable output
    let mut user_map: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for (other_file, fi) in &graph.graph {
        for imp in &fi.imports {
            if !imp.external && imp.path == target {
                let entry = user_map.entry(other_file.clone()).or_default();
                for sym in &imp.symbols {
                    if !entry.contains(sym) {
                        entry.push(sym.clone());
                    }
                }
            }
        }
    }
    let users: Vec<(String, Vec<String>)> = user_map.into_iter().map(|(k, mut v)| {
        v.sort();
        (k, v)
    }).collect();

    let indent = if current_depth == 1 {
        "  ".to_string()
    } else {
        format!("{}→ ", "  ".repeat(current_depth as usize))
    };

    for (user, symbols) in &users {
        if visited.contains(user) {
            out.push_str(&format!("{indent}{user} (cycle)\n"));
            continue;
        }
        if symbols.is_empty() {
            out.push_str(&format!("{indent}{user}\n"));
        } else {
            out.push_str(&format!("{indent}{user} ({})\n", format_symbols(symbols)));
        }
        if current_depth < max_depth {
            visited.insert(user.clone());
            collect_used_by(graph, user, max_depth, current_depth + 1, visited, out);
            visited.remove(user);
        }
    }
}

pub fn deps_cache_path(root: &Path) -> PathBuf {
    root.join(DEPS_DIR).join(DEPS_FILE)
}

pub fn load_deps_cache(root: &Path) -> Option<DepsGraph> {
    let path = deps_cache_path(root);
    let data = std::fs::read_to_string(&path).ok()?;
    let graph: DepsGraph = serde_json::from_str(&data).ok()?;
    if graph.version != CACHE_VERSION {
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
            if len >= best_len {
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

    fn fi(imports: Vec<ImportInfo>) -> FileImports {
        FileImports {
            content_hash: String::new(),
            raw_imports: Vec::new(),
            imports,
        }
    }

    #[test]
    fn rust_crate_import_resolves() {
        let all_files = vec![
            "src/codemap.rs".to_string(),
            "src/index/mod.rs".to_string(),
            "src/mcp.rs".to_string(),
        ];
        let result =
            resolve_import("crate::codemap", Language::Rust, "src/mcp.rs", &all_files, None, None);
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
            None,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn python_relative_import_resolves() {
        let all_files = vec!["src/auth.py".to_string(), "src/api.py".to_string()];
        let result = resolve_import(".auth", Language::Python, "src/api.py", &all_files, None, None);
        assert_eq!(result, Some("src/auth.py".to_string()));
    }

    #[test]
    fn ts_relative_import_resolves() {
        let all_files = vec!["src/utils.ts".to_string(), "src/main.ts".to_string()];
        let result =
            resolve_import("./utils", Language::TypeScript, "src/main.ts", &all_files, None, None);
        assert_eq!(result, Some("src/utils.ts".to_string()));
    }

    #[test]
    fn ts_bare_import_is_external() {
        let all_files = vec!["src/main.ts".to_string()];
        let result =
            resolve_import("express", Language::TypeScript, "src/main.ts", &all_files, None, None);
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
            resolve_import("./types", Language::TypeScript, "src/main.ts", &all_files, None, None);
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
            None,
        );
        assert_eq!(result, Some("crate-a/src/utils.rs".to_string()));

        // Cross-crate import
        let result = resolve_import(
            "crate_b::types",
            Language::Rust,
            "crate-a/src/lib.rs",
            &all_files,
            Some(&crate_map),
            None,
        );
        assert_eq!(result, Some("crate-b/src/types.rs".to_string()));
    }

    #[test]
    fn resolve_rust_single_crate_fallback() {
        let all_files = vec!["src/mcp.rs".to_string(), "src/index/mod.rs".to_string()];
        // No crate map -- single crate
        let result =
            resolve_import("crate::mcp", Language::Rust, "src/main.rs", &all_files, None, None);
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
            None,
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
            None,
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
            version: CACHE_VERSION,
            fingerprint: String::new(),
            graph: std::collections::HashMap::new(),
        };
        graph.graph.insert(
            "src/main.rs".to_string(),
            fi(vec![
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
            ]),
        );
        let out = query_deps(&graph, "src/main.rs", 1);
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
        let graph = build_deps_graph(dir.path(), &files, None);

        let main_deps = query_deps(&graph, "src/main.rs", 1);
        assert!(
            main_deps.contains("depends_on:"),
            "main should depend on helper:\n{main_deps}"
        );
        assert!(
            main_deps.contains("src/helper.rs"),
            "main should depend on helper:\n{main_deps}"
        );

        let helper_deps = query_deps(&graph, "src/helper.rs", 1);
        assert!(
            helper_deps.contains("used_by:"),
            "helper should be used by main:\n{helper_deps}"
        );
        assert!(
            helper_deps.contains("src/main.rs"),
            "helper should be used by main:\n{helper_deps}"
        );
    }

    #[test]
    fn python_absolute_import_flat_layout_resolves() {
        let all_files = vec![
            "canopi/enrichment/merge.py".to_string(),
            "canopi/enrichment/__init__.py".to_string(),
            "canopi/__init__.py".to_string(),
        ];
        let result = resolve_import(
            "canopi.enrichment.merge",
            Language::Python,
            "canopi/pipeline.py",
            &all_files,
            None,
            None,
        );
        assert_eq!(result, Some("canopi/enrichment/merge.py".to_string()));
    }

    #[test]
    fn python_absolute_import_src_layout_resolves() {
        // The reported bug: package is under src/, but resolver was generating bare paths
        let all_files = vec![
            "src/canopi/enrichment/merge.py".to_string(),
            "src/canopi/enrichment/__init__.py".to_string(),
            "src/canopi/__init__.py".to_string(),
        ];
        let result = resolve_import(
            "canopi.enrichment.merge",
            Language::Python,
            "src/canopi/pipeline.py",
            &all_files,
            None,
            None,
        );
        assert_eq!(result, Some("src/canopi/enrichment/merge.py".to_string()));
    }

    #[test]
    fn python_absolute_import_package_resolves() {
        // Import resolves to __init__.py when no direct .py match
        let all_files = vec![
            "src/canopi/__init__.py".to_string(),
            "src/canopi/enrichment/__init__.py".to_string(),
        ];
        let result = resolve_import(
            "canopi.enrichment",
            Language::Python,
            "src/canopi/pipeline.py",
            &all_files,
            None,
            None,
        );
        assert_eq!(
            result,
            Some("src/canopi/enrichment/__init__.py".to_string())
        );
    }

    #[test]
    fn python_absolute_import_custom_layout_resolves() {
        // Source root is discovered via __init__.py, not hardcoded directory names.
        // Works for any layout — "backend/", "code/", or any arbitrary prefix.
        let all_files = vec![
            "backend/mypackage/__init__.py".to_string(),
            "backend/mypackage/utils.py".to_string(),
        ];
        let result = resolve_import(
            "mypackage.utils",
            Language::Python,
            "backend/mypackage/main.py",
            &all_files,
            None,
            None,
        );
        assert_eq!(result, Some("backend/mypackage/utils.py".to_string()));
    }

    #[test]
    fn python_namespace_package_flat_resolves() {
        // Namespace packages (no __init__.py) resolve via flat layout fallback
        let all_files = vec!["mylib/core.py".to_string()];
        let result = resolve_import(
            "mylib.core",
            Language::Python,
            "app.py",
            &all_files,
            None,
            None,
        );
        assert_eq!(result, Some("mylib/core.py".to_string()));
    }

    #[test]
    fn go_resolves_internal_package() {
        let all_files = vec![
            "pkg/parser/parser.go".to_string(),
            "pkg/parser/ast.go".to_string(),
            "cmd/main.go".to_string(),
        ];
        let mut module_map = HashMap::new();
        module_map.insert(
            "github.com/owner/repo".to_string(),
            PathBuf::from(""),
        );
        let result = resolve_import(
            "github.com/owner/repo/pkg/parser",
            Language::Go,
            "cmd/main.go",
            &all_files,
            None,
            Some(&module_map),
        );
        assert!(
            result == Some("pkg/parser/parser.go".to_string())
                || result == Some("pkg/parser/ast.go".to_string()),
            "expected a file in pkg/parser/, got: {result:?}"
        );
    }

    #[test]
    fn go_external_import_returns_none() {
        let all_files = vec!["cmd/main.go".to_string()];
        let mut module_map = HashMap::new();
        module_map.insert(
            "github.com/owner/repo".to_string(),
            PathBuf::from(""),
        );
        let result = resolve_import(
            "github.com/some/external/pkg",
            Language::Go,
            "cmd/main.go",
            &all_files,
            None,
            Some(&module_map),
        );
        assert_eq!(result, None);
    }

    #[test]
    fn go_resolves_submodule_package() {
        // Monorepo: go.mod in a subdirectory
        let all_files = vec![
            "backend/internal/service/service.go".to_string(),
            "backend/cmd/main.go".to_string(),
        ];
        let mut module_map = HashMap::new();
        module_map.insert(
            "github.com/owner/repo/backend".to_string(),
            PathBuf::from("backend"),
        );
        let result = resolve_import(
            "github.com/owner/repo/backend/internal/service",
            Language::Go,
            "backend/cmd/main.go",
            &all_files,
            None,
            Some(&module_map),
        );
        assert_eq!(
            result,
            Some("backend/internal/service/service.go".to_string())
        );
    }

    #[test]
    fn build_go_module_map_reads_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("go.mod"),
            "module github.com/owner/myrepo\n\ngo 1.21\n",
        )
        .unwrap();
        let map = build_go_module_map(dir.path());
        assert_eq!(map.len(), 1);
        assert!(
            map.contains_key("github.com/owner/myrepo"),
            "expected module key, got: {map:?}"
        );
        assert_eq!(map["github.com/owner/myrepo"], PathBuf::from(""));
    }

    #[test]
    fn query_deps_renders_symbols() {
        let mut graph = DepsGraph { version: CACHE_VERSION, fingerprint: String::new(), graph: HashMap::new() };
        graph.graph.insert("a.py".to_string(), fi(vec![ImportInfo {
            path: "b.py".to_string(),
            symbols: vec!["Foo".to_string(), "Bar".to_string()],
            external: false,
        }]));
        graph.graph.insert("b.py".to_string(), fi(vec![]));

        let out = query_deps(&graph, "a.py", 1);
        assert!(out.contains("b.py (Bar, Foo)"), "should show sorted symbols: {out}");
    }

    #[test]
    fn query_deps_used_by_merges_symbols_from_multiple_imports() {
        let mut graph = DepsGraph { version: CACHE_VERSION, fingerprint: String::new(), graph: HashMap::new() };
        graph.graph.insert("target.py".to_string(), fi(vec![]));
        // user.py has two separate import statements from target.py
        graph.graph.insert("user.py".to_string(), fi(vec![
            ImportInfo { path: "target.py".to_string(), symbols: vec!["A".to_string()], external: false },
            ImportInfo { path: "target.py".to_string(), symbols: vec!["B".to_string()], external: false },
        ]));

        let out = query_deps(&graph, "target.py", 1);
        assert!(out.contains("A"), "should include symbol A: {out}");
        assert!(out.contains("B"), "should include symbol B: {out}");
        assert!(out.contains("user.py (A, B)"), "should merge symbols: {out}");
    }

    #[test]
    fn query_deps_depth_2_shows_transitive() {
        let mut graph = DepsGraph { version: CACHE_VERSION, fingerprint: String::new(), graph: HashMap::new() };
        graph.graph.insert("a.py".to_string(), fi(vec![]));
        graph.graph.insert("b.py".to_string(), fi(vec![
            ImportInfo { path: "a.py".to_string(), symbols: vec!["X".to_string()], external: false },
        ]));
        graph.graph.insert("c.py".to_string(), fi(vec![
            ImportInfo { path: "b.py".to_string(), symbols: vec!["Y".to_string()], external: false },
        ]));

        let out = query_deps(&graph, "a.py", 2);
        assert!(out.contains("b.py"), "depth 1: b.py uses a.py: {out}");
        assert!(out.contains("c.py"), "depth 2: c.py uses b.py: {out}");
    }

    #[test]
    fn query_deps_cycle_detection() {
        let mut graph = DepsGraph { version: CACHE_VERSION, fingerprint: String::new(), graph: HashMap::new() };
        graph.graph.insert("a.py".to_string(), fi(vec![
            ImportInfo { path: "b.py".to_string(), symbols: vec![], external: false },
        ]));
        graph.graph.insert("b.py".to_string(), fi(vec![
            ImportInfo { path: "a.py".to_string(), symbols: vec![], external: false },
        ]));

        let out = query_deps(&graph, "a.py", 3);
        assert!(out.contains("(cycle)"), "should detect cycle: {out}");
    }

    #[test]
    fn query_deps_depth_header() {
        let mut graph = DepsGraph { version: CACHE_VERSION, fingerprint: String::new(), graph: HashMap::new() };
        graph.graph.insert("a.py".to_string(), fi(vec![]));
        graph.graph.insert("b.py".to_string(), fi(vec![
            ImportInfo { path: "a.py".to_string(), symbols: vec![], external: false },
        ]));

        let out1 = query_deps(&graph, "a.py", 1);
        assert!(out1.contains("used_by:\n"), "depth 1 has plain header: {out1}");

        let out2 = query_deps(&graph, "a.py", 2);
        assert!(out2.contains("used_by (depth=2):"), "depth 2 has annotated header: {out2}");
    }

    #[test]
    fn format_symbols_below_threshold() {
        let syms: Vec<String> = (1..=6).map(|i| format!("Sym{i}")).collect();
        let out = format_symbols(&syms);
        assert_eq!(out, "Sym1, Sym2, Sym3, Sym4, Sym5, Sym6");
    }

    #[test]
    fn format_symbols_above_threshold() {
        let syms: Vec<String> = (1..=10).map(|i| format!("Sym{i}")).collect();
        let out = format_symbols(&syms);
        assert_eq!(out, "Sym1, Sym2, Sym3, Sym4, Sym5, Sym6, ... +4 more");
    }

    #[test]
    fn query_deps_truncates_long_symbol_lists() {
        let mut graph = DepsGraph { version: CACHE_VERSION, fingerprint: String::new(), graph: HashMap::new() };
        let many_symbols: Vec<String> = (1..=10).map(|i| format!("Type{i}")).collect();
        graph.graph.insert("a.py".to_string(), fi(vec![ImportInfo {
            path: "b.py".to_string(),
            symbols: many_symbols,
            external: false,
        }]));
        graph.graph.insert("b.py".to_string(), fi(vec![]));

        let out = query_deps(&graph, "a.py", 1);
        assert!(out.contains("... +4 more"), "should truncate long symbol list: {out}");
        assert!(out.contains("Type1"), "should show first symbols: {out}");
        assert!(!out.contains("Type7"), "should not show symbols past threshold: {out}");
    }

    #[test]
    fn build_deps_graph_incremental_reuses_cache() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("main.rs"), "use crate::helper;\nfn main() {}\n").unwrap();
        fs::write(src.join("helper.rs"), "pub fn run() {}\n").unwrap();

        let files = vec![src.join("main.rs"), src.join("helper.rs")];

        // Build from scratch
        let graph1 = build_deps_graph(dir.path(), &files, None);
        assert!(!graph1.fingerprint.is_empty(), "should have fingerprint");

        // Build again with cache — should produce identical result
        let graph2 = build_deps_graph(dir.path(), &files, Some(&graph1));
        assert_eq!(graph1.fingerprint, graph2.fingerprint);
        assert_eq!(graph1.graph.len(), graph2.graph.len());

        // Verify content hashes are populated
        for (_, fi) in &graph2.graph {
            assert!(!fi.content_hash.is_empty(), "content_hash should be populated");
        }
    }

    #[test]
    fn build_deps_graph_detects_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("main.rs"), "use crate::helper;\nfn main() {}\n").unwrap();
        fs::write(src.join("helper.rs"), "pub fn run() {}\n").unwrap();

        let files1 = vec![src.join("main.rs"), src.join("helper.rs")];
        let graph1 = build_deps_graph(dir.path(), &files1, None);

        // Add a new file
        fs::write(src.join("utils.rs"), "pub fn util() {}\n").unwrap();
        let files2 = vec![src.join("main.rs"), src.join("helper.rs"), src.join("utils.rs")];
        let graph2 = build_deps_graph(dir.path(), &files2, Some(&graph1));

        assert_ne!(graph1.fingerprint, graph2.fingerprint, "fingerprint should change");
        assert_eq!(graph2.graph.len(), 3, "new file should be in graph");
        assert!(graph2.graph.contains_key("src/utils.rs"));
    }

    #[test]
    fn build_deps_graph_detects_content_change() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(src.join("helper.rs"), "pub fn run() {}\n").unwrap();

        let files = vec![src.join("main.rs"), src.join("helper.rs")];
        let graph1 = build_deps_graph(dir.path(), &files, None);

        // main.rs now imports helper
        fs::write(src.join("main.rs"), "use crate::helper;\nfn main() {}\n").unwrap();
        let graph2 = build_deps_graph(dir.path(), &files, Some(&graph1));

        // Fingerprint unchanged (same files), but main.rs should have new imports
        assert_eq!(graph1.fingerprint, graph2.fingerprint, "same files = same fingerprint");
        let main_imports = &graph2.graph["src/main.rs"].imports;
        assert!(
            main_imports.iter().any(|i| i.path == "src/helper.rs" && !i.external),
            "should pick up new import: {main_imports:?}"
        );
    }

    #[test]
    fn java_resolve_guava_style_layout() {
        let all_files = vec![
            "guava/src/com/google/common/collect/ImmutableMap.java".to_string(),
            "guava/src/com/google/common/collect/ImmutableList.java".to_string(),
        ];
        let result = resolve_import(
            "com.google.common.collect.ImmutableMap",
            Language::Java,
            "guava/src/com/google/common/collect/ImmutableList.java",
            &all_files,
            None,
            None,
        );
        assert_eq!(
            result,
            Some("guava/src/com/google/common/collect/ImmutableMap.java".to_string()),
            "should resolve via suffix matching regardless of source root"
        );
    }

    #[test]
    fn java_resolve_spring_boot_deep_layout() {
        let all_files = vec![
            "core/spring-boot/src/main/java/org/springframework/boot/SpringApplication.java"
                .to_string(),
            "core/spring-boot/src/main/java/org/springframework/boot/Banner.java".to_string(),
        ];
        let result = resolve_import(
            "org.springframework.boot.Banner",
            Language::Java,
            "core/spring-boot/src/main/java/org/springframework/boot/SpringApplication.java",
            &all_files,
            None,
            None,
        );
        assert_eq!(
            result,
            Some(
                "core/spring-boot/src/main/java/org/springframework/boot/Banner.java".to_string()
            ),
        );
    }

    #[test]
    fn java_resolve_static_import() {
        let all_files = vec!["src/main/java/com/example/Preconditions.java".to_string()];
        let result = resolve_import(
            "com.example.Preconditions.checkNotNull",
            Language::Java,
            "src/main/java/com/example/App.java",
            &all_files,
            None,
            None,
        );
        assert_eq!(
            result,
            Some("src/main/java/com/example/Preconditions.java".to_string()),
            "should strip method name and resolve to class file"
        );
    }

    #[test]
    fn java_resolve_deep_static_import() {
        let all_files = vec!["src/com/example/ImmutableMap.java".to_string()];
        let result = resolve_import(
            "com.example.ImmutableMap.Builder.of",
            Language::Java,
            "src/com/example/App.java",
            &all_files,
            None,
            None,
        );
        assert_eq!(
            result,
            Some("src/com/example/ImmutableMap.java".to_string()),
            "should progressively strip segments to find the file"
        );
    }

    #[test]
    fn java_resolve_wildcard_import() {
        let all_files = vec![
            "src/main/java/org/springframework/boot/SpringApplication.java".to_string(),
            "src/main/java/org/springframework/boot/Banner.java".to_string(),
        ];
        let result = resolve_import(
            "org.springframework.boot.*",
            Language::Java,
            "src/main/java/org/springframework/boot/autoconfigure/App.java",
            &all_files,
            None,
            None,
        );
        assert!(
            result.is_some(),
            "wildcard import should resolve to some file in the package"
        );
        let resolved = result.unwrap();
        assert!(
            resolved.contains("org/springframework/boot/"),
            "should be in the correct package directory: {resolved}"
        );
    }

    #[test]
    fn java_resolve_boundary_no_false_match() {
        let all_files = vec!["src/mycom/example/Bar.java".to_string()];
        let result = resolve_import(
            "com.example.Bar",
            Language::Java,
            "src/com/example/Foo.java",
            &all_files,
            None,
            None,
        );
        assert_eq!(result, None, "should not match across word boundaries");
    }

    #[test]
    fn java_resolve_flat_layout() {
        let all_files = vec!["com/example/Foo.java".to_string()];
        let result = resolve_import(
            "com.example.Foo",
            Language::Java,
            "com/example/Bar.java",
            &all_files,
            None,
            None,
        );
        assert_eq!(result, Some("com/example/Foo.java".to_string()));
    }
}
