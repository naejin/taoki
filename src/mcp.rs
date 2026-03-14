use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codemap;
use crate::index;

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
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
    match req.method.as_str() {
        "initialize" => Some(handle_initialize(req)),
        "notifications/initialized" => None,
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

    match index::index_file(std::path::Path::new(path)) {
        Ok(skeleton) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: skeleton,
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
