use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codemap;
use crate::index;

thread_local! {
    static INDEX_CACHE: RefCell<HashMap<PathBuf, (String, String)>> = RefCell::new(HashMap::new());
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
                "name": "index",
                "description": "Return a compact structural skeleton of a source file: imports, type definitions, function signatures, and their line numbers. ~70-90% fewer tokens than reading the full file. Use this to understand a file's architecture before reading specific sections with the Read tool. Supports: Rust, Python, TypeScript, JavaScript, Go, Java.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the source file to index"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "code_map",
                "description": "Build an incremental structural map of a codebase. Returns one line per file with public types and public function signatures. Use this FIRST when you need to understand a repository's structure or find which files are relevant to a task. Results are cached (blake3 hash) so repeated calls are near-instant. Supports glob patterns to narrow scope.",
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
                "name": "dependencies",
                "description": "Show what a file imports and what imports it. Returns dependency and dependent files with the specific symbols used. Automatically builds the dependency graph if not cached. Use this to understand impact before modifying a file.",
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
        "index" => call_index(&arguments),
        "code_map" => call_code_map(&arguments),
        "dependencies" => call_dependencies(&arguments),
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

fn find_repo_root(path: &std::path::Path) -> Option<PathBuf> {
    let mut current = if path.is_file() {
        path.parent()?.to_path_buf()
    } else {
        path.to_path_buf()
    };
    let start = current.clone();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    // Fallback: .cache/taoki/ directory (supports non-git workspaces)
    current = start;
    loop {
        if current.join(".cache/taoki").is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn prepend_enrichment(skeleton: &str, enrichment: &str) -> String {
    let mut out = String::from("summary:\n");
    for line in enrichment.lines() {
        out.push_str(&format!("  {line}\n"));
    }
    out.push('\n');
    out.push_str(skeleton);
    out
}

fn call_index(args: &Value) -> ToolResult {
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

    // Look up enrichment data
    let debug = std::env::var("TAOKI_DEBUG").is_ok();
    let enrichment_text = find_repo_root(path).and_then(|root| {
        let enrichments = codemap::load_enrichment_cache(&root);
        let rel_path = path.strip_prefix(&root).ok()?;
        let rel_str = rel_path.to_string_lossy().replace('\\', "/");
        let entry = match enrichments.get(&*rel_str) {
            Some(e) => e,
            None => {
                if debug {
                    eprintln!("[taoki] no enrichment for {rel_str}");
                }
                return None;
            }
        };
        if entry.hash == hash {
            Some(entry.enrichment.clone())
        } else {
            None
        }
    });

    // Check in-memory cache (stores un-enriched skeletons)
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
        let final_skeleton = match &enrichment_text {
            Some(e) => prepend_enrichment(&skeleton, e),
            None => skeleton,
        };
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: final_skeleton,
            }],
            is_error: false,
        };
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
        let skeleton = match &enrichment_text {
            Some(e) => prepend_enrichment(&base_skeleton, e),
            None => base_skeleton,
        };
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: skeleton,
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
                    .insert(path_buf, (hash, raw_skeleton.clone()));
            });
            let skeleton = match &enrichment_text {
                Some(e) => prepend_enrichment(&raw_skeleton, e),
                None => raw_skeleton,
            };
            ToolResult {
                content: vec![ToolContent {
                    r#type: "text".to_string(),
                    text: skeleton,
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
}

fn call_code_map(args: &Value) -> ToolResult {
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

    match codemap::build_code_map(std::path::Path::new(path), &globs, &[]) {
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

fn call_dependencies(args: &Value) -> ToolResult {
    let file_str = args.get("file").and_then(|v| v.as_str()).unwrap_or("");
    let root_str = args.get("repo_root").and_then(|v| v.as_str()).unwrap_or("");

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

    // Try loading cached graph, build if missing
    let graph = match crate::deps::load_deps_cache(root) {
        Some(g) => g,
        None => {
            // Build graph by walking all files
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
            let g = crate::deps::build_deps_graph(root, &files);
            crate::deps::save_deps_cache(root, &g);
            g
        }
    };

    let output = crate::deps::query_deps(&graph, &rel);

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
        let result = call_dependencies(&args);
        assert!(!result.is_error, "should not error: {:?}", result.content);
        let text = &result.content[0].text;
        assert!(text.contains("depends_on:") || text.contains("external:"),
            "should show dependencies:\n{text}");
    }

    #[test]
    fn index_includes_enrichment_summary() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();

        // Initialize git repo so find_repo_root works
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Get the file hash
        let source = fs::read(&file).unwrap();
        let hash = blake3::hash(&source).to_hex().to_string();

        // Write enrichment cache
        let root_hash = blake3::hash(
            dir.path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .as_bytes(),
        )
        .to_hex()
        .to_string();

        let cache_dir = dir.path().join(".cache/taoki");
        fs::create_dir_all(&cache_dir).unwrap();
        let enrichment = serde_json::json!({
            "version": 1,
            "model": "haiku",
            "repo_root_hash": root_hash,
            "files": {
                "lib.rs": {
                    "hash": hash,
                    "enrichment": "Library root.\nconventions: exports a single function."
                }
            }
        });
        fs::write(
            cache_dir.join("enriched.json"),
            serde_json::to_string(&enrichment).unwrap(),
        )
        .unwrap();

        let result = call_index(&serde_json::json!({
            "path": file.to_string_lossy()
        }));
        let text = &result.content[0].text;
        assert!(
            text.starts_with("summary:\n"),
            "should start with summary:\n{text}"
        );
        assert!(text.contains("Library root."));
        assert!(text.contains("conventions: exports a single function."));
        assert!(text.contains("fns:"));
    }

    #[test]
    fn test_file_by_name_collapses_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test_auth.py");
        fs::write(&test_file, "def test_login():\n    assert True\n\ndef test_logout():\n    pass\n").unwrap();

        let args = serde_json::json!({ "path": test_file.to_str().unwrap() });
        let result = call_index(&args);
        assert!(!result.is_error);
        let text = &result.content[0].text;
        assert!(text.contains("tests:"), "should collapse entire file as tests:\n{text}");
        assert!(!text.contains("test_login"), "individual test names should not appear:\n{text}");
    }
}
