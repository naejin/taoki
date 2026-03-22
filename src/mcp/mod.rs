pub mod tools;

// Re-export cache functions used by cache.rs for xray pruning
pub(crate) use tools::{load_xray_cache, save_xray_cache};

use rmcp::schemars::JsonSchema;
use rmcp::serde::{Deserialize, Serialize};
use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
    service::ServiceExt,
    tool, tool_handler, tool_router,
    transport::io::stdio,
    ErrorData as McpError, ServerHandler,
};

// ---------------------------------------------------------------------------
// Parameter types — schemas derived automatically via JsonSchema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct XrayParams {
    /// Absolute path to the source file to xray
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RadarParams {
    /// Absolute path to the repository root to scan
    pub path: String,
    /// Optional glob patterns to filter files (e.g. ["src/**/*.rs"])
    #[serde(default)]
    pub globs: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RippleParams {
    /// Absolute path to the source file to query
    pub file: String,
    /// Absolute path to the repository root
    pub repo_root: String,
    /// How many levels of used_by to expand (1-3, default 1)
    #[serde(default = "default_depth")]
    #[schemars(range(min = 1, max = 3))]
    pub depth: u32,
}

fn default_depth() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

#[derive(Clone)]
#[allow(dead_code)]
pub struct TaokiMcpServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
#[allow(clippy::new_without_default)]
impl TaokiMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "xray",
        description = "Return a compact structural skeleton of a source file: imports, type definitions, function signatures, and their line numbers. ~70-90% fewer tokens than reading the full file. Use this to understand a file's architecture before reading specific sections with the Read tool. Results are cached on disk (blake3) so repeated calls on unchanged files are instant. Supports: Rust, Python, TypeScript, JavaScript, Go, Java."
    )]
    async fn xray(
        &self,
        params: Parameters<XrayParams>,
    ) -> Result<CallToolResult, McpError> {
        let result = tools::call_xray(&params.0.path);
        match result {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(text) => Ok(CallToolResult::error(vec![Content::text(text)])),
        }
    }

    #[tool(
        name = "radar",
        description = "Sweep a repository and build a structural map — one line per file with public types, function signatures, and heuristic tags like [entry-point], [tests], [error-types]. Use this first to orient in an unfamiliar repo or find which files are relevant. Results are cached (blake3) so repeated calls are near-instant. Supports globs to narrow scope."
    )]
    async fn radar(
        &self,
        params: Parameters<RadarParams>,
    ) -> Result<CallToolResult, McpError> {
        let result = tools::call_radar(&params.0.path, &params.0.globs);
        match result {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(text) => Ok(CallToolResult::error(vec![Content::text(text)])),
        }
    }

    #[tool(
        name = "ripple",
        description = "Trace the ripple effect of a file: what it imports, what imports it, and external dependencies. Use depth to expand the blast radius — see not just direct dependents but what depends on those. Automatically builds the dependency graph if not cached."
    )]
    async fn ripple(
        &self,
        params: Parameters<RippleParams>,
    ) -> Result<CallToolResult, McpError> {
        let depth = params.0.depth.clamp(1, 3);
        let result = tools::call_ripple(&params.0.file, &params.0.repo_root, depth);
        match result {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(text) => Ok(CallToolResult::error(vec![Content::text(text)])),
        }
    }
}

#[tool_handler]
impl ServerHandler for TaokiMcpServer {
    fn get_info(&self) -> InitializeResult {
        let mut result =
            InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
                .with_instructions(
                    "Structural code intelligence — radar (repo overview), \
                     xray (file skeleton), ripple (dependency graph).",
                );
        result.server_info.name = "taoki".to_string();
        result.server_info.version = env!("CARGO_PKG_VERSION").to_string();
        result
    }
}

pub async fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    let server = TaokiMcpServer::new();
    let transport = stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
