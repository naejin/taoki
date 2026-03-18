use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::CACHE_VERSION;
use crate::codemap;
use crate::index;

thread_local! {
    static INDEX_CACHE: RefCell<HashMap<PathBuf, (String, String)>> = RefCell::new(HashMap::new());
}

const XRAY_CACHE_DIR: &str = ".cache/taoki";
const XRAY_CACHE_FILE: &str = "xray.json";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct XrayDiskCache {
    pub(crate) version: u32,
    pub(crate) files: HashMap<String, XrayDiskEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
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

/// Write the entire xray cache atomically (lock + tmp + rename).
pub(crate) fn save_xray_cache(root: &Path, cache: &XrayDiskCache) {
    use fs2::FileExt;
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

/// Atomically insert a single entry into the xray disk cache.
fn upsert_xray_cache(root: &Path, key: String, entry: XrayDiskEntry) {
    let mut cache = load_xray_cache(root);
    cache.files.insert(key, entry);
    save_xray_cache(root, &cache);
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ToolContent {
    pub r#type: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "is_false")]
    #[serde(rename = "isError")]
    pub is_error: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

pub fn tool_definitions() -> Value {
    serde_json::json!({
        "tools": [
            {
                "name": "xray",
                "description": "Return a compact structural skeleton of a source file: imports, type definitions, function signatures, and their line numbers. ~70-90% fewer tokens than reading the full file. Use this to understand a file's architecture before reading specific sections with the Read tool. Results are cached on disk (blake3) so repeated calls on unchanged files are instant. Supports: Rust, Python, TypeScript, JavaScript, Go, Java.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the source file to xray"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "radar",
                "description": "Sweep a repository and build a structural map — one line per file with public types, function signatures, and heuristic tags like [entry-point], [tests], [error-types]. Use this first to orient in an unfamiliar repo or find which files are relevant. Results are cached (blake3) so repeated calls are near-instant. Supports globs to narrow scope.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the repository root to scan"
                        },
                        "globs": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional glob patterns to filter files (e.g. [\"src/**/*.rs\"]). Defaults to all supported file types."
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "ripple",
                "description": "Trace the ripple effect of a file: what it imports, what imports it, and external dependencies. Use depth to expand the blast radius — see not just direct dependents but what depends on those. Automatically builds the dependency graph if not cached.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "Absolute path to the source file to query"
                        },
                        "repo_root": {
                            "type": "string",
                            "description": "Absolute path to the repository root"
                        },
                        "depth": {
                            "type": "integer",
                            "description": "How many levels of used_by to expand (1-3, default 1). Higher values show more of the blast radius.",
                            "default": 1,
                            "minimum": 1,
                            "maximum": 3
                        }
                    },
                    "required": ["file", "repo_root"]
                }
            }
        ]
    })
}

pub fn handle_request(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    // Notifications (no id) must never receive a response per JSON-RPC spec
    req.id.as_ref()?;

    match req.method.as_str() {
        "initialize" => Some(handle_initialize(req)),
        "ping" => Some(JsonRpcResponse::success(req.id.clone(), serde_json::json!({}))),
        "tools/list" => Some(handle_tools_list(req)),
        "tools/call" => Some(handle_tools_call(req)),
        _ => Some(JsonRpcResponse::error(
            req.id.clone(),
            -32601,
            format!("method not found: {}", req.method),
        )),
    }
}

fn handle_initialize(req: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse::success(
        req.id.clone(),
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "taoki",
                "version": "0.1.0"
            }
        }),
    )
}

fn handle_tools_list(req: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse::success(req.id.clone(), tool_definitions())
}

fn handle_tools_call(req: &JsonRpcRequest) -> JsonRpcResponse {
    let tool_name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = req.params.get("arguments").cloned().unwrap_or_default();

    let result = match tool_name {
        "xray" => call_xray(&arguments),
        "radar" => call_radar(&arguments),
        "ripple" => call_ripple(&arguments),
        _ => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: format!("unknown tool: {tool_name}"),
            }],
            is_error: true,
        },
    };

    JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
}

fn call_xray(args: &Value) -> ToolResult {
    let path_str = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path_str.is_empty() {
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: "missing required parameter: path".to_string(),
            }],
            is_error: true,
        };
    }

    let path = std::path::Path::new(path_str);

    // Read file and determine language
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = match index::Language::from_extension(ext) {
        Some(l) => l,
        None => {
            return ToolResult {
                content: vec![ToolContent {
                    r#type: "text".to_string(),
                    text: format!("unsupported file type: .{ext}"),
                }],
                is_error: true,
            };
        }
    };

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return ToolResult {
                content: vec![ToolContent {
                    r#type: "text".to_string(),
                    text: format!("read error: {e}"),
                }],
                is_error: true,
            };
        }
    };

    if meta.len() > index::MAX_FILE_SIZE {
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: format!(
                    "file too large ({} bytes, max {})",
                    meta.len(),
                    index::MAX_FILE_SIZE
                ),
            }],
            is_error: true,
        };
    }

    let source = match std::fs::read(path) {
        Ok(s) => s,
        Err(e) => {
            return ToolResult {
                content: vec![ToolContent {
                    r#type: "text".to_string(),
                    text: format!("read error: {e}"),
                }],
                is_error: true,
            };
        }
    };

    let hash = blake3::hash(&source).to_hex().to_string();
    let path_buf = path.to_path_buf();

    // Check in-memory cache
    let cached = INDEX_CACHE.with(|cache| {
        let cache = cache.borrow();
        cache.get(&path_buf).and_then(|(h, skeleton)| {
            if *h == hash {
                Some(skeleton.clone())
            } else {
                None
            }
        })
    });

    if let Some(skeleton) = cached {
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: skeleton,
            }],
            is_error: false,
        };
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
                // Populate in-memory cache too
                INDEX_CACHE.with(|cache| {
                    cache.borrow_mut().insert(path_buf.clone(), (hash.clone(), entry.skeleton.clone()));
                });
                return ToolResult {
                    content: vec![ToolContent {
                        r#type: "text".to_string(),
                        text: entry.skeleton.clone(),
                    }],
                    is_error: false,
                };
            }
        }
    }

    // If entire file is a test file by naming convention, collapse it
    if is_test_filename(path) {
        let total_lines = source.iter().filter(|&&b| b == b'\n').count() + 1;
        let base_skeleton = format!("tests: [1-{}]\n", total_lines);
        INDEX_CACHE.with(|cache| {
            cache
                .borrow_mut()
                .insert(path_buf.clone(), (hash.clone(), base_skeleton.clone()));
        });
        // Save to disk cache
        if let (Some(root), Some(ref rel)) = (&repo_root, &rel_path) {
            upsert_xray_cache(root, rel.clone(), XrayDiskEntry {
                hash: hash.clone(),
                skeleton: base_skeleton.clone(),
            });
        }
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: base_skeleton,
            }],
            is_error: false,
        };
    }

    // Cache miss — parse and store
    match index::index_source(&source, lang) {
        Ok(raw_skeleton) => {
            INDEX_CACHE.with(|cache| {
                cache
                    .borrow_mut()
                    .insert(path_buf, (hash.clone(), raw_skeleton.clone()));
            });
            // Save to disk cache
            if let (Some(root), Some(ref rel)) = (&repo_root, &rel_path) {
                upsert_xray_cache(root, rel.clone(), XrayDiskEntry {
                    hash: hash.clone(),
                    skeleton: raw_skeleton.clone(),
                });
            }
            ToolResult {
                content: vec![ToolContent {
                    r#type: "text".to_string(),
                    text: raw_skeleton,
                }],
                is_error: false,
            }
        }
        Err(e) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: e.to_string(),
            }],
            is_error: true,
        },
    }
}

pub fn is_test_filename(path: &std::path::Path) -> bool {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    // Go: *_test.go
    name.ends_with("_test.go")
    // Python: test_*.py, *_test.py
        || (matches!(ext, "py" | "pyi") && (stem.starts_with("test_") || stem.ends_with("_test")))
    // TS/JS: *.test.ts, *.spec.ts, *.test.js, *.spec.js
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
    // Java: *Test.java, *Tests.java
        || (ext == "java" && (stem.ends_with("Test") || stem.ends_with("Tests")))
    // Test data directories (universal conventions)
        || is_test_data_path(path)
}

/// Checks if a file lives inside a well-known test data directory.
///
/// These directories contain fixture/input data for tests, not application code.
/// Uses forward-slash normalized paths to work cross-platform.
fn is_test_data_path(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    const PATTERNS: &[&str] = &[
        "/testdata/",      // Go convention
        "/tests/data/",    // Python, Rust
        "/tests/fixtures/", // general
        "/test/fixtures/", // general
        "/test/data/",     // general
        "/__fixtures__/",  // Jest
        "/src/test/resources/", // Java/Maven
    ];
    PATTERNS.iter().any(|p| s.contains(p))
}

fn call_radar(args: &Value) -> ToolResult {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: "missing required parameter: path".to_string(),
            }],
            is_error: true,
        };
    }

    let globs: Vec<String> = args
        .get("globs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    match codemap::build_code_map(std::path::Path::new(path), &globs) {
        Ok(map) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: map,
            }],
            is_error: false,
        },
        Err(e) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: e.to_string(),
            }],
            is_error: true,
        },
    }
}

fn call_ripple(args: &Value) -> ToolResult {
    let file_str = args.get("file").and_then(|v| v.as_str()).unwrap_or("");
    let root_str = args.get("repo_root").and_then(|v| v.as_str()).unwrap_or("");
    let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(1).clamp(1, 3) as u32;

    if file_str.is_empty() || root_str.is_empty() {
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: "missing required parameters: file, repo_root".to_string(),
            }],
            is_error: true,
        };
    }

    let root = std::path::Path::new(root_str);
    let file_path = std::path::Path::new(file_str);

    let rel = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .to_string();

    // Always walk files — needed for incremental invalidation
    let files = match crate::codemap::walk_files_public(root) {
        Ok(f) => f,
        Err(e) => {
            return ToolResult {
                content: vec![ToolContent {
                    r#type: "text".to_string(),
                    text: format!("failed to walk files: {e}"),
                }],
                is_error: true,
            };
        }
    };

    // Load old cache (if any) and do incremental build
    let old_cache = crate::deps::load_deps_cache(root);
    let graph = crate::deps::build_deps_graph(root, &files, old_cache.as_ref());
    crate::deps::save_deps_cache(root, &graph);

    let output = crate::deps::query_deps(&graph, &rel, depth);

    ToolResult {
        content: vec![ToolContent {
            r#type: "text".to_string(),
            text: output,
        }],
        is_error: false,
    }
}

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

        let args = serde_json::json!({
            "file": src.join("main.rs").to_str().unwrap(),
            "repo_root": dir.path().to_str().unwrap(),
        });
        let result = call_ripple(&args);
        assert!(!result.is_error, "should not error: {:?}", result.content);
        let text = &result.content[0].text;
        assert!(text.contains("depends_on:") || text.contains("external:"),
            "should show dependencies:\n{text}");
    }

    #[test]
    fn test_file_by_name_collapses_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test_auth.py");
        fs::write(&test_file, "def test_login():\n    assert True\n\ndef test_logout():\n    pass\n").unwrap();

        let args = serde_json::json!({ "path": test_file.to_str().unwrap() });
        let result = call_xray(&args);
        assert!(!result.is_error);
        let text = &result.content[0].text;
        assert!(text.contains("tests:"), "should collapse entire file as tests:\n{text}");
        assert!(!text.contains("test_login"), "individual test names should not appear:\n{text}");
    }

    #[test]
    fn test_data_path_detected() {
        assert!(is_test_filename(std::path::Path::new("project/tests/data/cases/pep_654.py")));
        assert!(is_test_filename(std::path::Path::new("project/testdata/input.go")));
        assert!(is_test_filename(std::path::Path::new("project/test/fixtures/sample.ts")));
        assert!(is_test_filename(std::path::Path::new("project/__fixtures__/mock.js")));
        assert!(is_test_filename(std::path::Path::new("project/src/test/resources/Config.java")));
    }

    #[test]
    fn tool_definitions_use_new_names() {
        let defs = tool_definitions();
        let tools = defs["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["xray", "radar", "ripple"]);
    }

    #[test]
    fn xray_disk_cache_persists() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();

        let args = serde_json::json!({ "path": file.to_str().unwrap() });

        // First call — parses and caches
        let r1 = call_xray(&args);
        assert!(!r1.is_error);

        // Clear in-memory cache to force disk read
        INDEX_CACHE.with(|c| c.borrow_mut().clear());

        // Second call — should hit disk cache
        let r2 = call_xray(&args);
        assert!(!r2.is_error);
        assert_eq!(r1.content[0].text, r2.content[0].text);

        // Verify cache file exists
        let cache_path = dir.path().join(".cache/taoki/xray.json");
        assert!(cache_path.exists(), "disk cache should exist");
    }

    #[test]
    fn xray_disk_cache_invalidated_on_change() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();

        let args = serde_json::json!({ "path": file.to_str().unwrap() });
        let r1 = call_xray(&args);

        // Modify the file
        fs::write(&file, "pub fn hello() {}\npub fn world() {}\n").unwrap();
        INDEX_CACHE.with(|c| c.borrow_mut().clear());

        let r2 = call_xray(&args);
        assert_ne!(r1.content[0].text, r2.content[0].text, "should re-parse changed file");
    }

    #[test]
    fn xray_works_outside_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();

        let args = serde_json::json!({ "path": file.to_str().unwrap() });
        let result = call_xray(&args);
        assert!(!result.is_error, "should work without git repo");
        assert!(result.content[0].text.contains("hello"));
    }

    #[test]
    fn xray_corrupt_cache_falls_back_to_parse() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();

        // Write corrupt cache
        let cache_dir = dir.path().join(".cache/taoki");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("xray.json"), "{{not valid json}}").unwrap();

        let args = serde_json::json!({ "path": file.to_str().unwrap() });
        let result = call_xray(&args);
        assert!(!result.is_error, "should gracefully fall back to parsing");
        assert!(result.content[0].text.contains("hello"));
    }

    #[test]
    fn non_test_data_path_not_detected() {
        assert!(!is_test_filename(std::path::Path::new("src/data/models.py")));
        assert!(!is_test_filename(std::path::Path::new("src/fixtures.rs")));
        assert!(!is_test_filename(std::path::Path::new("lib/data/parser.ts")));
        assert!(!is_test_filename(std::path::Path::new("src/fixtures/models.py")));
        assert!(!is_test_filename(std::path::Path::new("app/fixtures/seed.ts")));
    }

    #[test]
    fn ripple_detects_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("main.rs"), "use crate::helper;\nfn main() {}\n").unwrap();
        fs::write(src.join("helper.rs"), "pub fn run() {}\n").unwrap();

        // First call — builds and caches the graph
        let args1 = serde_json::json!({
            "file": src.join("helper.rs").to_str().unwrap(),
            "repo_root": dir.path().to_str().unwrap(),
        });
        let r1 = call_ripple(&args1);
        assert!(!r1.is_error);
        assert!(r1.content[0].text.contains("src/main.rs"), "helper used by main: {}", r1.content[0].text);

        // Add a new file that imports helper
        fs::write(src.join("utils.rs"), "use crate::helper;\npub fn util() {}\n").unwrap();

        // Second call — should detect new file without manual cache deletion
        let r2 = call_ripple(&args1);
        assert!(!r2.is_error);
        let text = &r2.content[0].text;
        assert!(text.contains("src/main.rs"), "helper still used by main: {text}");
        assert!(text.contains("src/utils.rs"), "helper now also used by utils: {text}");
    }
}
