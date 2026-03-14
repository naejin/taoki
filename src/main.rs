mod codemap;
mod index;
mod mcp;

use std::io::{self, Read, Write};

fn read_message<R: Read>(reader: &mut R) -> io::Result<Option<String>> {
    let mut headers = String::new();
    let mut buf = [0u8; 1];
    while reader.read(&mut buf)? == 1 {
        headers.push(buf[0] as char);
        if headers.ends_with("\r\n\r\n") {
            break;
        }
    }
    if headers.is_empty() {
        return Ok(None);
    }
    let mut content_len = None;
    for line in headers.split("\r\n") {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_len = rest.trim().parse::<usize>().ok();
            break;
        }
    }
    let Some(len) = content_len else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"));
    };
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).to_string()))
}

fn write_message<W: Write>(writer: &mut W, msg: &str) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n{}", msg.len(), msg)?;
    writer.flush()
}

fn main() {
    eprintln!("taoki: MCP server starting");

    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    loop {
        let raw = match read_message(&mut stdin) {
            Ok(Some(m)) => m,
            Ok(None) => break,
            Err(e) => {
                eprintln!("taoki: read error: {e}");
                break;
            }
        };

        let req: mcp::JsonRpcRequest = match serde_json::from_str(&raw) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("taoki: parse error: {e}");
                let resp = mcp::JsonRpcResponse::error(None, -32700, format!("parse error: {e}"));
                let json = serde_json::to_string(&resp).unwrap();
                let _ = write_message(&mut stdout, &json);
                continue;
            }
        };

        eprintln!("taoki: received {}", req.method);

        if let Some(resp) = mcp::handle_request(&req) {
            let json = serde_json::to_string(&resp).unwrap();
            let _ = write_message(&mut stdout, &json);
        }
    }

    eprintln!("taoki: MCP server shutting down");
}
