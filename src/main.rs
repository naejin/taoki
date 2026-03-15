use taoki::mcp;

use std::io::{self, BufRead, Write};

#[derive(Clone, Copy, PartialEq)]
enum Framing {
    ContentLength,
    Jsonl,
}

/// Auto-detect framing: Content-Length headers (LSP-style) or newline-delimited JSON.
/// Returns (message, detected_framing) or None on EOF.
fn read_message(reader: &mut impl BufRead) -> io::Result<Option<(String, Framing)>> {
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(None);
        }
        // Skip whitespace / blank lines between messages
        if buf[0] == b'\n' || buf[0] == b'\r' || buf[0] == b' ' || buf[0] == b'\t' {
            reader.consume(1);
            continue;
        }
        if buf[0] == b'{' {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            return Ok(Some((trimmed, Framing::Jsonl)));
        } else {
            return read_content_length_message(reader)
                .map(|opt| opt.map(|msg| (msg, Framing::ContentLength)));
        }
    }
}

fn read_content_length_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return if headers.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF in headers"))
            };
        }
        headers.push_str(&line);
        if headers.ends_with("\r\n\r\n") || headers.ends_with("\n\n") {
            break;
        }
    }
    let mut content_len = None;
    for line in headers.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if key.trim().eq_ignore_ascii_case("Content-Length") {
                content_len = value.trim().parse::<usize>().ok();
                break;
            }
        }
    }
    let Some(len) = content_len else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"));
    };
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).to_string()))
}

fn write_message(writer: &mut impl Write, msg: &str, framing: Framing) -> io::Result<()> {
    match framing {
        Framing::ContentLength => {
            write!(writer, "Content-Length: {}\r\n\r\n{}", msg.len(), msg)?;
        }
        Framing::Jsonl => {
            writer.write_all(msg.as_bytes())?;
            writer.write_all(b"\n")?;
        }
    }
    writer.flush()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--version" {
        println!("taoki {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    eprintln!("taoki: MCP server starting");

    let stdin = io::stdin().lock();
    let mut reader = io::BufReader::new(stdin);
    let mut stdout = io::stdout().lock();
    // Default to Content-Length; updated on first message received
    let mut framing = Framing::ContentLength;

    loop {
        let raw = match read_message(&mut reader) {
            Ok(Some((m, f))) => {
                framing = f;
                m
            }
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
                let _ = write_message(&mut stdout, &json, framing);
                continue;
            }
        };

        eprintln!("taoki: received {}", req.method);

        if let Some(resp) = mcp::handle_request(&req) {
            let json = serde_json::to_string(&resp).unwrap();
            let _ = write_message(&mut stdout, &json, framing);
        }
    }

    eprintln!("taoki: MCP server shutting down");
}
