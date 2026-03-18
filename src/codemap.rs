use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::index::{self, Language};

#[derive(Debug, thiserror::Error)]
pub enum CodeMapError {
    #[error("path does not exist: {0}")]
    PathNotFound(PathBuf),
    #[error("invalid glob pattern: {0}")]
    InvalidGlob(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    version: u32,
    files: HashMap<String, CacheEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    hash: String,
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
    tags: Vec<String>,
}

const CACHE_VERSION: u32 = 1;
const CACHE_DIR: &str = ".cache/taoki";
const CACHE_FILE: &str = "radar.json";
const GROUPING_THRESHOLD: usize = 100;
const FN_TRUNCATE_THRESHOLD: usize = 8;
const TYPE_TRUNCATE_THRESHOLD: usize = 12;
const FN_TRUNCATE_SHOW: usize = 6;
const TYPE_TRUNCATE_SHOW: usize = 10;

struct FileResult {
    path: String,
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
    tags: Vec<String>,
    parse_error: bool,
}

fn walk_files(root: &Path, globs: &[String]) -> Result<Vec<PathBuf>, CodeMapError> {
    use globset::{Glob, GlobSetBuilder};
    use ignore::WalkBuilder;

    if !root.exists() {
        return Err(CodeMapError::PathNotFound(root.to_path_buf()));
    }

    let glob_set = if globs.is_empty() {
        None
    } else {
        let mut builder = GlobSetBuilder::new();
        for g in globs {
            let glob = Glob::new(g).map_err(|_| CodeMapError::InvalidGlob(g.clone()))?;
            builder.add(glob);
        }
        Some(builder.build().map_err(|_| CodeMapError::InvalidGlob("globset".into()))?)
    };

    let mut files = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .standard_filters(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if Language::from_extension(ext).is_none() {
            continue;
        }
        if let Some(ref gs) = glob_set {
            let rel = path.strip_prefix(root).unwrap_or(path);
            if !gs.is_match(rel) {
                continue;
            }
        }
        files.push(path.to_path_buf());
    }

    files.sort();
    Ok(files)
}

pub fn walk_files_public(root: &Path) -> Result<Vec<PathBuf>, CodeMapError> {
    walk_files(root, &[])
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    let data = std::fs::read(path)?;
    Ok(blake3::hash(&data).to_hex().to_string())
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(CACHE_DIR).join(CACHE_FILE)
}

fn load_cache(root: &Path) -> Cache {
    let path = cache_path(root);
    let lock_path = path.with_extension("lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path);
    let _lock_guard = if let Ok(f) = lock_file {
        if f.lock_shared().is_ok() {
            Some(f)
        } else {
            None
        }
    } else {
        None
    };

    let result = match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or(Cache {
            version: CACHE_VERSION,
            files: HashMap::new(),
        }),
        Err(_) => Cache {
            version: CACHE_VERSION,
            files: HashMap::new(),
        },
    };
    if let Some(f) = _lock_guard {
        let _ = f.unlock();
    }
    result
}

fn save_cache(root: &Path, cache: &Cache) {
    let path = cache_path(root);
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
            eprintln!("warning: could not open cache lock: {e}");
            return;
        }
    };
    if lock_file.lock_exclusive().is_err() {
        eprintln!("warning: could not lock cache file");
        return;
    }
    if let Ok(data) = serde_json::to_string_pretty(cache) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        } else {
            eprintln!("warning: could not write cache temp file");
        }
    }
    let _ = lock_file.unlock();
}

/// Check if any line in source starts with one of the given patterns.
/// This avoids false positives from string literals containing patterns.
fn any_line_starts_with(source: &str, patterns: &[&str]) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        patterns.iter().any(|p| trimmed.starts_with(p))
    })
}

/// Count lines starting with any of the given patterns.
fn count_lines_starting_with(source: &str, patterns: &[&str]) -> usize {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            patterns.iter().any(|p| trimmed.starts_with(p))
        })
        .count()
}

fn compute_tags(
    filename: &str,
    public_types: &[String],
    public_functions: &[String],
    source: &[u8],
) -> Vec<String> {
    let mut tags = Vec::new();
    let source_str = std::str::from_utf8(source).unwrap_or("");
    let fpath = std::path::Path::new(filename);
    let stem = fpath.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = fpath.extension().and_then(|s| s.to_str()).unwrap_or("");

    // entry-point: has main() in public API
    if public_functions
        .iter()
        .any(|f| f.starts_with("main(") || f == "main()")
    {
        tags.push("entry-point".to_string());
    }
    // Also check non-public main via line-start matching, but only for the
    // file's own language to avoid false positives from embedded test strings
    if tags.is_empty() {
        let main_pattern: &[&str] = match ext {
            "rs" => &["fn main()"],
            "go" => &["func main()"],
            "py" | "pyi" => &["def main("],
            "java" => &["public static void main("],
            _ => &[],
        };
        if !main_pattern.is_empty() && any_line_starts_with(source_str, main_pattern) {
            tags.push("entry-point".to_string());
        }
    }

    // tests: filename convention (extension-aware)
    if filename.ends_with("_test.go")
        || (matches!(ext, "py" | "pyi") && (stem.starts_with("test_") || stem.ends_with("_test")))
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || (ext == "java" && (stem.ends_with("Test") || stem.ends_with("Tests")))
    {
        tags.push("tests".to_string());
    }

    // data-models: only types, no functions
    if !public_types.is_empty() && public_functions.is_empty() {
        tags.push("data-models".to_string());
    }

    // interfaces: defines traits/interfaces without implementations
    if any_line_starts_with(
        source_str,
        &["pub trait ", "export interface ", "public interface "],
    ) && !any_line_starts_with(source_str, &["impl "])
    {
        tags.push("interfaces".to_string());
    }

    // http-handlers: line-start anchored to avoid matching string literals
    if any_line_starts_with(
        source_str,
        &[
            "@GetMapping",
            "@PostMapping",
            "@RequestMapping",
            "@Path",
            "@app.route",
            "@router.",
            "#[get(",
            "#[post(",
            "#[put(",
            "#[delete(",
        ],
    ) || source_str
        .lines()
        .any(|l| {
            let t = l.trim();
            (t.contains("http.ResponseWriter") || t.contains("*http.Request"))
                && t.starts_with("func ")
        })
    {
        tags.push("http-handlers".to_string());
    }

    // error-types: types with Error/Exception in name
    if public_types
        .iter()
        .any(|t| t.contains("Error") || t.contains("Exception"))
    {
        tags.push("error-types".to_string());
    }

    // barrel-file: mostly re-exports (line-start anchored)
    let reexport_count = count_lines_starting_with(
        source_str,
        &["pub use ", "pub mod ", "export * from", "export {"],
    );
    let definition_count = public_functions.len() + public_types.len();
    if reexport_count > definition_count && reexport_count >= 3 {
        tags.push("barrel-file".to_string());
    }

    // cli: line-start anchored
    if any_line_starts_with(
        source_str,
        &[
            "use clap",
            "#[derive(Parser",
            "import argparse",
            "from argparse",
        ],
    ) || source_str.lines().any(|l| {
        let t = l.trim();
        t.starts_with("flag.Parse()") || t.starts_with("flag.String(")
    })
    {
        tags.push("cli".to_string());
    }

    // minified: bundled/minified code with no useful structure
    if index::is_minified(source) {
        tags.push("minified".to_string());
    }

    // module-root: specific filenames
    if filename.ends_with("mod.rs")
        || filename.ends_with("__init__.py")
        || filename.ends_with("/index.ts")
        || filename.ends_with("/index.js")
        || filename.ends_with("/index.tsx")
        || filename.ends_with("/index.jsx")
    {
        tags.push("module-root".to_string());
    }

    tags
}

pub fn build_code_map(root: &Path, globs: &[String]) -> Result<String, CodeMapError> {
    let files = walk_files(root, globs)?;
    let mut cache = load_cache(root);

    // Invalidate cache if version changed
    if cache.version != CACHE_VERSION {
        cache = Cache {
            version: CACHE_VERSION,
            files: HashMap::new(),
        };
    }

    let mut new_files: HashMap<String, CacheEntry> = HashMap::new();
    let mut results: Vec<FileResult> = Vec::new();

    for file_path in &files {
        let rel = file_path
            .strip_prefix(root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .replace('\\', "/");

        let hash = match hash_file(file_path) {
            Ok(h) => h,
            Err(_) => continue,
        };

        // Check cache
        if let Some(cached) = cache.files.get(&rel) {
            if cached.hash == hash {
                results.push(FileResult {
                    path: rel.clone(),
                    lines: cached.lines,
                    public_types: cached.public_types.clone(),
                    public_functions: cached.public_functions.clone(),
                    tags: cached.tags.clone(),
                    parse_error: false,
                });
                new_files.insert(rel, CacheEntry {
                    hash,
                    lines: cached.lines,
                    public_types: cached.public_types.clone(),
                    public_functions: cached.public_functions.clone(),
                    tags: cached.tags.clone(),
                });
                continue;
            }
        }

        // Parse file
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = Language::from_extension(ext) else {
            continue;
        };

        let source = match std::fs::read(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines = source.iter().filter(|&&b| b == b'\n').count() + 1;

        let (public_types, public_functions) =
            match index::extract_public_api(&source, lang) {
                Ok((types, fns)) => (types, fns),
                Err(_) => {
                    results.push(FileResult {
                        path: rel.clone(),
                        lines,
                        public_types: Vec::new(),
                        public_functions: Vec::new(),
                        tags: Vec::new(),
                        parse_error: true,
                    });
                    continue;
                }
            };

        let tags = compute_tags(&rel, &public_types, &public_functions, &source);

        new_files.insert(
            rel.clone(),
            CacheEntry {
                hash: hash.clone(),
                lines,
                public_types: public_types.clone(),
                public_functions: public_functions.clone(),
                tags: tags.clone(),
            },
        );

        results.push(FileResult {
            path: rel,
            lines,
            public_types,
            public_functions,
            tags,
            parse_error: false,
        });
    }

    // Update and save cache
    cache.files = new_files;
    save_cache(root, &cache);

    // Build and cache dependency graph alongside code map
    // Note: This rebuilds the full graph on every code_map call. Per-file incremental
    // updates (skip unchanged files) is a Phase 2 optimization.
    let graph = crate::deps::build_deps_graph(root, &files);
    crate::deps::save_deps_cache(root, &graph);

    // Sort by path and format output
    results.sort_by(|a, b| a.path.cmp(&b.path));

    let out = if results.len() > GROUPING_THRESHOLD {
        format_grouped(&results)
    } else {
        format_flat(&results)
    };

    Ok(out)
}

fn truncate_list(items: &[String], threshold: usize, show: usize) -> (Vec<String>, usize) {
    if items.len() > threshold {
        let shown: Vec<String> = items[..show].to_vec();
        let remaining = items.len() - show;
        (shown, remaining)
    } else {
        (items.to_vec(), 0)
    }
}

fn format_names_only(items: &[String]) -> Vec<String> {
    items.iter().map(|s| {
        s.split(['(', ':'])
         .next()
         .unwrap_or(s)
         .split_whitespace()
         .last()
         .unwrap_or(s)
         .to_string()
    }).collect()
}

fn format_flat(results: &[FileResult]) -> String {
    let mut out = String::new();
    for fr in results {
        if fr.parse_error {
            out.push_str(&format!("- {} ({} lines) (parse error)\n", fr.path, fr.lines));
            continue;
        }
        let tags_str = if fr.tags.is_empty() {
            String::new()
        } else {
            format!(" {}", fr.tags.iter().map(|t| format!("[{t}]")).collect::<Vec<_>>().join(" "))
        };

        let (shown_types, more_types) = truncate_list(&fr.public_types, TYPE_TRUNCATE_THRESHOLD, TYPE_TRUNCATE_SHOW);
        let (shown_fns, more_fns) = truncate_list(&fr.public_functions, FN_TRUNCATE_THRESHOLD, FN_TRUNCATE_SHOW);

        let types_str = if shown_types.is_empty() {
            "(none)".to_string()
        } else {
            let mut s = shown_types.join(", ");
            if more_types > 0 {
                s.push_str(&format!(", ... +{more_types} more (use xray for full list)"));
            }
            s
        };
        let fns_str = if shown_fns.is_empty() {
            "(none)".to_string()
        } else {
            let mut s = shown_fns.iter()
                .map(|f| f.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join(", ");
            if more_fns > 0 {
                s.push_str(&format!(", ... +{more_fns} more (use xray for full list)"));
            }
            s
        };

        out.push_str(&format!(
            "- {} ({} lines){tags_str} - public_types: {types_str} - public_functions: {fns_str}\n",
            fr.path, fr.lines
        ));
    }
    out
}

fn format_grouped(results: &[FileResult]) -> String {
    let mut out = String::new();

    // Group by directory
    let mut groups: Vec<(String, Vec<&FileResult>)> = Vec::new();
    let mut current_dir = String::new();
    let mut current_files: Vec<&FileResult> = Vec::new();

    for fr in results {
        let dir = match fr.path.rfind('/') {
            Some(pos) => &fr.path[..=pos],
            None => "(root)/",
        };
        if dir != current_dir {
            if !current_files.is_empty() {
                groups.push((current_dir.clone(), std::mem::take(&mut current_files)));
            }
            current_dir = dir.to_string();
        }
        current_files.push(fr);
    }
    if !current_files.is_empty() {
        groups.push((current_dir, current_files));
    }

    for (dir, files) in &groups {
        let total_lines: usize = files.iter().map(|f| f.lines).sum();
        out.push_str(&format!("{dir} ({} files, {} lines)\n", files.len(), total_lines));

        for fr in files {
            let filename = fr.path.rsplit('/').next().unwrap_or(&fr.path);
            let tags_str = if fr.tags.is_empty() {
                String::new()
            } else {
                format!(" {}", fr.tags.iter().map(|t| format!("[{t}]")).collect::<Vec<_>>().join(" "))
            };

            // Name-only API
            let type_names = format_names_only(&fr.public_types);
            let fn_names = format_names_only(&fr.public_functions);
            let mut all_names: Vec<String> = Vec::new();

            let (shown_types, more_types) = truncate_list(&type_names, TYPE_TRUNCATE_THRESHOLD, TYPE_TRUNCATE_SHOW);
            all_names.extend(shown_types);
            if more_types > 0 {
                all_names.push(format!("... +{more_types} more types"));
            }

            let (shown_fns, more_fns) = truncate_list(&fn_names, FN_TRUNCATE_THRESHOLD, FN_TRUNCATE_SHOW);
            all_names.extend(shown_fns);
            if more_fns > 0 {
                all_names.push(format!("... +{more_fns} more fns (use xray for full list)"));
            }

            if all_names.is_empty() {
                out.push_str(&format!("  {filename} ({} lines){tags_str}\n", fr.lines));
            } else {
                out.push_str(&format!("  {filename} ({} lines){tags_str} - {}\n", fr.lines, all_names.join(", ")));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn extracts_public_types_and_functions_from_rust() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub struct Foo {}\npub fn bar() {}\nfn private() {}\n").unwrap();

        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(result.contains("lib.rs"));
        assert!(result.contains("Foo"));
        assert!(result.contains("bar()"));
        assert!(!result.contains("private"));
    }

    #[test]
    fn caching_reuses_results() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub struct Foo {}\n").unwrap();

        // First call — builds cache
        let r1 = build_code_map(dir.path(), &[]).unwrap();
        assert!(dir.path().join(".cache/taoki/radar.json").exists());

        // Second call — uses cache (same result)
        let r2 = build_code_map(dir.path(), &[]).unwrap();
        assert_eq!(r1, r2);

        // Modify file — cache miss
        fs::write(&file, "pub struct Bar {}\n").unwrap();
        let r3 = build_code_map(dir.path(), &[]).unwrap();
        assert!(r3.contains("Bar"));
        assert!(!r3.contains("Foo"));
    }

    #[test]
    fn tags_entry_point() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.rs");
        fs::write(&file, "fn main() {}\n").unwrap();

        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(result.contains("[entry-point]"), "missing entry-point tag in:\n{result}");
    }

    #[test]
    fn tags_tests_by_filename() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test_auth.py");
        fs::write(&file, "def test_login():\n    pass\n").unwrap();

        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(result.contains("[tests]"), "missing tests tag in:\n{result}");
    }

    #[test]
    fn tags_module_root() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("pkg");
        fs::create_dir(&sub).unwrap();
        let file = sub.join("mod.rs");
        fs::write(&file, "pub fn foo() {}\n").unwrap();

        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(result.contains("[module-root]"), "missing module-root tag in:\n{result}");
    }

    #[test]
    fn tags_error_types() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("errors.rs");
        fs::write(&file, "pub enum MyError { Io, Parse }\n").unwrap();

        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(result.contains("[error-types]"), "missing error-types tag in:\n{result}");
    }

    #[test]
    fn code_map_overview_has_no_file_headers() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();
        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(!result.contains("# lib.rs"), "overview should not have file headers:\n{result}");
        assert!(result.contains("lib.rs"), "overview should list file in one-liner:\n{result}");
    }

    #[test]
    fn cache_v2_triggers_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();
        let cache_dir = dir.path().join(".cache/taoki");
        fs::create_dir_all(&cache_dir).unwrap();
        let old_cache = serde_json::json!({
            "version": 2,
            "files": {
                "lib.rs": {
                    "hash": "oldhash",
                    "lines": 1,
                    "public_types": [],
                    "public_functions": ["hello()"],
                    "tags": []
                }
            }
        });
        fs::write(cache_dir.join("radar.json"), serde_json::to_string(&old_cache).unwrap()).unwrap();
        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(result.contains("lib.rs"), "should list file after cache rebuild:\n{result}");
        let cache_data: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(cache_dir.join("radar.json")).unwrap()).unwrap();
        assert_eq!(cache_data["version"], CACHE_VERSION);
    }

    #[test]
    fn code_map_globs_filter_overview() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let lib_dir = dir.path().join("lib");
        fs::create_dir(&src_dir).unwrap();
        fs::create_dir(&lib_dir).unwrap();
        fs::write(src_dir.join("main.rs"), "pub fn main() {}\n").unwrap();
        fs::write(lib_dir.join("helper.rs"), "pub fn help() {}\n").unwrap();
        let result = build_code_map(dir.path(), &["src/**/*.rs".to_string()]).unwrap();
        assert!(result.contains("src/main.rs"), "src/main.rs should appear:\n{result}");
        assert!(!result.contains("helper.rs"), "helper.rs outside glob should be excluded:\n{result}");
    }

    #[test]
    fn truncation_caps_long_function_lists() {
        let dir = tempfile::tempdir().unwrap();
        let mut src = String::new();
        for i in 0..15 {
            src.push_str(&format!("pub fn func_{i}() {{}}\n"));
        }
        fs::write(dir.path().join("big.rs"), &src).unwrap();

        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(result.contains("... +9 more"), "should truncate: {result}");
        assert!(result.contains("use xray for full list"), "should include xray cue: {result}");
    }

    #[test]
    fn directory_grouping_for_large_repos() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..60 {
            let subdir = dir.path().join(format!("src/mod_{}", i / 10));
            fs::create_dir_all(&subdir).unwrap();
            fs::write(subdir.join(format!("file_{i}.rs")), format!("pub fn f{i}() {{}}\n")).unwrap();
        }
        for i in 0..50 {
            let subdir = dir.path().join(format!("lib/pkg_{}", i / 10));
            fs::create_dir_all(&subdir).unwrap();
            fs::write(subdir.join(format!("mod_{i}.rs")), format!("pub struct S{i};\n")).unwrap();
        }

        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(result.contains("src/mod_0/"), "should have directory headers: {result}");
        assert!(!result.contains("public_types:"), "grouped mode should not use flat format labels");
    }

    #[test]
    fn code_map_multiline_signatures_collapsed_to_single_line() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "pub fn long_fn(\n    a: &str,\n    b: usize,\n    c: bool,\n) -> Option<String> {\n    None\n}\n",
        )
        .unwrap();
        let result = build_code_map(dir.path(), &[]).unwrap();
        let file_line = result
            .lines()
            .find(|l| l.contains("lib.rs"))
            .expect("lib.rs should appear in output");
        assert!(
            file_line.contains("long_fn("),
            "signature should appear in one-liner: {file_line}"
        );
        assert!(
            !file_line.contains('\n'),
            "one-liner must not contain embedded newlines: {file_line}"
        );
        assert_eq!(
            result.lines().filter(|l| l.contains("lib.rs")).count(),
            1,
            "file entry should be a single line:\n{result}"
        );
    }
}
