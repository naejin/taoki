use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::cache::CACHE_VERSION;
use crate::codemap;
use crate::deps;
use crate::index;

// ---------------------------------------------------------------------------
// In-memory xray cache (thread-local, per-session)
// ---------------------------------------------------------------------------

thread_local! {
    static INDEX_CACHE: RefCell<HashMap<PathBuf, (String, String)>> = RefCell::new(HashMap::new());
}

// ---------------------------------------------------------------------------
// Disk cache types and operations
// ---------------------------------------------------------------------------

const XRAY_CACHE_DIR: &str = ".cache/taoki";
const XRAY_CACHE_FILE: &str = "xray.json";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct XrayDiskCache {
    pub(crate) version: u32,
    pub(crate) files: HashMap<String, XrayDiskEntry>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct XrayDiskEntry {
    pub(crate) hash: String,
    pub(crate) skeleton: String,
}

fn find_repo_root(file_path: &Path) -> Option<PathBuf> {
    let mut dir = file_path.parent()?;
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

pub(crate) fn xray_cache_path(root: &Path) -> PathBuf {
    root.join(XRAY_CACHE_DIR).join(XRAY_CACHE_FILE)
}

pub(crate) fn load_xray_cache(root: &Path) -> XrayDiskCache {
    let path = xray_cache_path(root);
    let lock_path = path.with_extension("lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path);
    let _lock_guard = if let Ok(f) = lock_file {
        if f.lock_shared().is_ok() { Some(f) } else { None }
    } else {
        None
    };

    let result = match std::fs::read_to_string(&path) {
        Ok(data) => match serde_json::from_str::<XrayDiskCache>(&data) {
            Ok(c) if c.version == CACHE_VERSION => c,
            _ => XrayDiskCache { version: CACHE_VERSION, files: HashMap::new() },
        },
        Err(_) => XrayDiskCache { version: CACHE_VERSION, files: HashMap::new() },
    };
    if let Some(f) = _lock_guard {
        let _ = f.unlock();
    }
    result
}

pub(crate) fn save_xray_cache(root: &Path, cache: &XrayDiskCache) {
    let path = xray_cache_path(root);
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
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
        Err(_) => return,
    };
    if lock_file.lock_exclusive().is_err() {
        return;
    }
    if let Ok(data) = serde_json::to_string_pretty(cache) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, &data).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    let _ = lock_file.unlock();
}

fn upsert_xray_cache(root: &Path, key: String, entry: XrayDiskEntry) {
    let mut cache = load_xray_cache(root);
    cache.files.insert(key, entry);
    save_xray_cache(root, &cache);
}

// ---------------------------------------------------------------------------
// Test filename detection
// ---------------------------------------------------------------------------

pub fn is_test_filename(path: &Path) -> bool {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    name.ends_with("_test.go")
        || (matches!(ext, "py" | "pyi") && (stem.starts_with("test_") || stem.ends_with("_test")))
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || (ext == "java" && (stem.ends_with("Test") || stem.ends_with("Tests")))
        || is_test_data_path(path)
}

fn is_test_data_path(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    const PATTERNS: &[&str] = &[
        "/testdata/",
        "/tests/data/",
        "/tests/fixtures/",
        "/test/fixtures/",
        "/test/data/",
        "/__fixtures__/",
        "/src/test/resources/",
    ];
    PATTERNS.iter().any(|p| s.contains(p))
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

/// Xray tool: returns structural skeleton of a source file.
/// Ok(text) on success, Err(text) on user-facing error.
pub fn call_xray(path_str: &str) -> Result<String, String> {
    if path_str.is_empty() {
        return Err("missing required parameter: path".to_string());
    }

    let path = Path::new(path_str);

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = index::Language::from_extension(ext)
        .ok_or_else(|| format!("unsupported file type: .{ext}"))?;

    let meta = std::fs::metadata(path)
        .map_err(|e| format!("read error: {e}"))?;

    if meta.len() > index::MAX_FILE_SIZE {
        return Err(format!(
            "file too large ({} bytes, max {})",
            meta.len(),
            index::MAX_FILE_SIZE,
        ));
    }

    let source = std::fs::read(path)
        .map_err(|e| format!("read error: {e}"))?;

    let hash = blake3::hash(&source).to_hex().to_string();
    let path_buf = path.to_path_buf();

    // Check in-memory cache
    let cached = INDEX_CACHE.with(|cache| {
        let cache = cache.borrow();
        cache.get(&path_buf).and_then(|(h, skeleton)| {
            if *h == hash { Some(skeleton.clone()) } else { None }
        })
    });
    if let Some(skeleton) = cached {
        return Ok(skeleton);
    }

    // Check disk cache
    let repo_root = find_repo_root(path);
    let rel_path = repo_root.as_ref().and_then(|root| {
        path.strip_prefix(root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))
    });

    if let (Some(root), Some(ref rel)) = (&repo_root, &rel_path) {
        let disk_cache = load_xray_cache(root);
        if let Some(entry) = disk_cache.files.get(rel) {
            if entry.hash == hash {
                INDEX_CACHE.with(|cache| {
                    cache.borrow_mut().insert(path_buf.clone(), (hash.clone(), entry.skeleton.clone()));
                });
                return Ok(entry.skeleton.clone());
            }
        }
    }

    // Test file by naming convention — collapse entirely
    if is_test_filename(path) {
        let total_lines = source.iter().filter(|&&b| b == b'\n').count() + 1;
        let skeleton = format!("tests: [1-{}]\n", total_lines);
        INDEX_CACHE.with(|cache| {
            cache.borrow_mut().insert(path_buf, (hash.clone(), skeleton.clone()));
        });
        if let (Some(root), Some(ref rel)) = (&repo_root, &rel_path) {
            upsert_xray_cache(root, rel.clone(), XrayDiskEntry {
                hash,
                skeleton: skeleton.clone(),
            });
        }
        return Ok(skeleton);
    }

    // Cache miss — parse and store
    let skeleton = index::index_source(&source, lang)
        .map_err(|e| e.to_string())?;

    INDEX_CACHE.with(|cache| {
        cache.borrow_mut().insert(path_buf, (hash.clone(), skeleton.clone()));
    });
    if let (Some(root), Some(ref rel)) = (&repo_root, &rel_path) {
        upsert_xray_cache(root, rel.clone(), XrayDiskEntry {
            hash,
            skeleton: skeleton.clone(),
        });
    }
    Ok(skeleton)
}

/// Radar tool: builds a structural map of a repository.
pub fn call_radar(path: &str, globs: &[String]) -> Result<String, String> {
    if path.is_empty() {
        return Err("missing required parameter: path".to_string());
    }
    codemap::build_code_map(Path::new(path), globs)
        .map_err(|e| e.to_string())
}

/// Ripple tool: traces import/export dependencies for a file.
pub fn call_ripple(file_str: &str, root_str: &str, depth: u32) -> Result<String, String> {
    if file_str.is_empty() || root_str.is_empty() {
        return Err("missing required parameters: file, repo_root".to_string());
    }

    let root = Path::new(root_str);
    let file_path = Path::new(file_str);
    let rel = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .to_string();

    let files = codemap::walk_files_public(root)
        .map_err(|e| format!("failed to walk files: {e}"))?;

    let old_cache = deps::load_deps_cache(root);
    let graph = deps::build_deps_graph(root, &files, old_cache.as_ref());
    deps::save_deps_cache(root, &graph);

    Ok(deps::query_deps(&graph, &rel, depth))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn dependencies_tool_returns_deps() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("main.rs"), "use crate::helper;\nfn main() { helper::run(); }\n").unwrap();
        fs::write(src.join("helper.rs"), "pub fn run() {}\n").unwrap();

        let result = call_ripple(
            src.join("main.rs").to_str().unwrap(),
            dir.path().to_str().unwrap(),
            1,
        );
        assert!(result.is_ok(), "should not error: {:?}", result);
        let text = result.unwrap();
        assert!(text.contains("depends_on:") || text.contains("external:"),
            "should show dependencies:\n{text}");
    }

    #[test]
    fn test_file_by_name_collapses_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test_auth.py");
        fs::write(&test_file, "def test_login():\n    assert True\n\ndef test_logout():\n    pass\n").unwrap();

        let result = call_xray(test_file.to_str().unwrap());
        assert!(result.is_ok());
        let text = result.unwrap();
        assert!(text.contains("tests:"), "should collapse entire file as tests:\n{text}");
        assert!(!text.contains("test_login"), "individual test names should not appear:\n{text}");
    }

    #[test]
    fn test_data_path_detected() {
        assert!(is_test_filename(Path::new("project/tests/data/cases/pep_654.py")));
        assert!(is_test_filename(Path::new("project/testdata/input.go")));
        assert!(is_test_filename(Path::new("project/test/fixtures/sample.ts")));
        assert!(is_test_filename(Path::new("project/__fixtures__/mock.js")));
        assert!(is_test_filename(Path::new("project/src/test/resources/Config.java")));
    }

    #[test]
    fn xray_disk_cache_persists() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();

        let r1 = call_xray(file.to_str().unwrap());
        assert!(r1.is_ok());

        INDEX_CACHE.with(|c| c.borrow_mut().clear());

        let r2 = call_xray(file.to_str().unwrap());
        assert!(r2.is_ok());
        assert_eq!(r1.unwrap(), r2.unwrap());

        let cache_path = dir.path().join(".cache/taoki/xray.json");
        assert!(cache_path.exists(), "disk cache should exist");
    }

    #[test]
    fn xray_disk_cache_invalidated_on_change() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();

        let r1 = call_xray(file.to_str().unwrap()).unwrap();

        fs::write(&file, "pub fn hello() {}\npub fn world() {}\n").unwrap();
        INDEX_CACHE.with(|c| c.borrow_mut().clear());

        let r2 = call_xray(file.to_str().unwrap()).unwrap();
        assert_ne!(r1, r2, "should re-parse changed file");
    }

    #[test]
    fn xray_works_outside_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();

        let result = call_xray(file.to_str().unwrap());
        assert!(result.is_ok(), "should work without git repo");
        assert!(result.unwrap().contains("hello"));
    }

    #[test]
    fn xray_corrupt_cache_falls_back_to_parse() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();

        let cache_dir = dir.path().join(".cache/taoki");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("xray.json"), "{{not valid json}}").unwrap();

        let result = call_xray(file.to_str().unwrap());
        assert!(result.is_ok(), "should gracefully fall back to parsing");
        assert!(result.unwrap().contains("hello"));
    }

    #[test]
    fn non_test_data_path_not_detected() {
        assert!(!is_test_filename(Path::new("src/data/models.py")));
        assert!(!is_test_filename(Path::new("src/fixtures.rs")));
        assert!(!is_test_filename(Path::new("lib/data/parser.ts")));
        assert!(!is_test_filename(Path::new("src/fixtures/models.py")));
        assert!(!is_test_filename(Path::new("app/fixtures/seed.ts")));
    }

    #[test]
    fn ripple_detects_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("main.rs"), "use crate::helper;\nfn main() {}\n").unwrap();
        fs::write(src.join("helper.rs"), "pub fn run() {}\n").unwrap();

        let r1 = call_ripple(
            src.join("helper.rs").to_str().unwrap(),
            dir.path().to_str().unwrap(),
            1,
        );
        assert!(r1.is_ok());
        assert!(r1.as_ref().unwrap().contains("src/main.rs"), "helper used by main: {}", r1.unwrap());

        fs::write(src.join("utils.rs"), "use crate::helper;\npub fn util() {}\n").unwrap();

        let r2 = call_ripple(
            src.join("helper.rs").to_str().unwrap(),
            dir.path().to_str().unwrap(),
            1,
        );
        assert!(r2.is_ok());
        let text = r2.unwrap();
        assert!(text.contains("src/main.rs"), "helper still used by main: {text}");
        assert!(text.contains("src/utils.rs"), "helper now also used by utils: {text}");
    }
}
