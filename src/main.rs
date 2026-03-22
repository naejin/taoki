use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--version" {
        println!("taoki {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    eprintln!("taoki: MCP server starting");

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("taoki: failed to start async runtime: {e}");
            process::exit(1);
        }
    };
    rt.block_on(async {
        if let Err(e) = taoki::mcp::run_mcp_server().await {
            eprintln!("taoki: MCP server error: {e}");
            process::exit(1);
        }
    });
}
