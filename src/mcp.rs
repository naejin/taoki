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

    // If entire file is a test file by naming convention, collapse it
    if is_test_filename(path) {
        let total_lines = source.iter().filter(|&&b| b == b'\n').count() + 1;
        let skeleton = format!("tests: [1-{}]\n", total_lines);
        INDEX_CACHE.with(|cache| {
            cache.borrow_mut().insert(path_buf.clone(), (hash.clone(), skeleton.clone()));
        });
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
        Ok(skeleton) => {
            INDEX_CACHE.with(|cache| {
                cache.borrow_mut().insert(path_buf, (hash, skeleton.clone()));
            });
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

fn is_test_filename(path: &std::path::Path) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
