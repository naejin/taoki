use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    #[serde(default)]
    tags: Vec<String>,
}

const CACHE_VERSION: u32 = 2;
const CACHE_DIR: &str = ".cache/taoki";
const CACHE_FILE: &str = "code-map.json";

/// (path, lines, public_types, public_functions, tags, parse_error)
type FileResult = (String, usize, Vec<String>, Vec<String>, Vec<String>, bool);

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
    use fs2::FileExt;
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

fn compute_tags(
    filename: &str,
    public_types: &[String],
    public_functions: &[String],
    source: &[u8],
) -> Vec<String> {
    let mut tags = Vec::new();
    let source_str = std::str::from_utf8(source).unwrap_or("");

    // entry-point: has main()
    if public_functions.iter().any(|f| f.starts_with("main(") || f == "main()")
    {
        tags.push("entry-point".to_string());
    }
    // Also check non-public main for languages where main isn't exported
    if tags.is_empty()
        && (source_str.contains("fn main()")
            || source_str.contains("func main()")
            || source_str.contains("def main(")
            || source_str.contains("public static void main("))
    {
        tags.push("entry-point".to_string());
    }

    // tests: filename convention (extension-aware)
    let fpath = std::path::Path::new(filename);
    let stem = fpath.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = fpath.extension().and_then(|s| s.to_str()).unwrap_or("");
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
    if (source_str.contains("pub trait ") || source_str.contains("export interface ")
        || source_str.contains("public interface "))
        && !source_str.contains("impl ")
    {
        tags.push("interfaces".to_string());
    }

    // http-handlers: heuristic pattern matching
    if source_str.contains("@GetMapping") || source_str.contains("@PostMapping")
        || source_str.contains("@RequestMapping") || source_str.contains("@Path")
        || source_str.contains("@app.route") || source_str.contains("@router.")
        || source_str.contains("http.ResponseWriter") || source_str.contains("*http.Request")
        || source_str.contains("#[get(") || source_str.contains("#[post(")
        || source_str.contains("#[put(") || source_str.contains("#[delete(")
    {
        tags.push("http-handlers".to_string());
    }

    // error-types: types with Error/Exception in name
    if public_types.iter().any(|t| t.contains("Error") || t.contains("Exception")) {
        tags.push("error-types".to_string());
    }

    // barrel-file: mostly re-exports
    let reexport_count = source_str.matches("pub use ").count()
        + source_str.matches("pub mod ").count()
        + source_str.matches("export * from").count()
        + source_str.matches("export {").count();
    let definition_count = public_functions.len() + public_types.len();
    if reexport_count > definition_count && reexport_count >= 3 {
        tags.push("barrel-file".to_string());
    }

    // cli: argument parsing patterns
    if source_str.contains("clap::") || source_str.contains("#[derive(Parser")
        || source_str.contains("argparse") || source_str.contains("ArgumentParser")
        || source_str.contains("flag.Parse()") || source_str.contains("flag.String(")
    {
        tags.push("cli".to_string());
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
            .to_string();

        let hash = match hash_file(file_path) {
            Ok(h) => h,
            Err(_) => continue,
        };

        // Check cache
        if let Some(cached) = cache.files.get(&rel) {
            if cached.hash == hash {
                results.push((
                    rel.clone(),
                    cached.lines,
                    cached.public_types.clone(),
                    cached.public_functions.clone(),
                    cached.tags.clone(),
                    false,
                ));
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
                Ok(api) => api,
                Err(_) => {
                    results.push((rel.clone(), lines, Vec::new(), Vec::new(), Vec::new(), true));
                    continue;
                }
            };

        let tags = compute_tags(&rel, &public_types, &public_functions, &source);

        new_files.insert(
            rel.clone(),
            CacheEntry {
                hash,
                lines,
                public_types: public_types.clone(),
                public_functions: public_functions.clone(),
                tags: tags.clone(),
            },
        );

        results.push((rel, lines, public_types, public_functions, tags, false));
    }

    // Update and save cache
    cache.files = new_files;
    save_cache(root, &cache);

    // Sort by path and format output
    results.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::new();
    for (path, lines, types, fns, tags, parse_error) in &results {
        if *parse_error {
            out.push_str(&format!("- {path} ({lines} lines) (parse error)\n"));
            continue;
        }
        let tags_str = if tags.is_empty() {
            String::new()
        } else {
            format!(" {}", tags.iter().map(|t| format!("[{t}]")).collect::<Vec<_>>().join(" "))
        };
        let types_str = if types.is_empty() {
            "(none)".to_string()
        } else {
            types.join(", ")
        };
        let fns_str = if fns.is_empty() {
            "(none)".to_string()
        } else {
            fns.join(", ")
        };
        out.push_str(&format!(
            "- {path} ({lines} lines){tags_str} - public_types: {types_str} - public_functions: {fns_str}\n"
        ));
    }

    Ok(out)
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
        assert!(dir.path().join(".cache/taoki/code-map.json").exists());

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
}
