//! One HTTP/1.1 exchange with a dev server on loopback, its answer read (sized or chunked) up to
//! a limit.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::Response;

pub(super) fn refuse_remote(addr: SocketAddr) -> Result<(), String> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(format!(
            "Refused: {} isn't a loopback address. Sawit only talks to Local Explorer on this Mac (127.0.0.1 or ::1).",
            addr.ip()
        ))
    }
}

/// One HTTP/1.1 exchange with the server at `addr` (loopback, checked by the callers), reading at
/// most `limit` bytes of the answer's body.
pub(super) async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<(&[u8], &str)>,
    wait: Duration,
    limit: usize,
) -> Result<Response, String> {
    refuse_remote(addr)?;
    let exchange = async {
        let mut stream = TcpStream::connect(addr)
            .await
            .map_err(|e| format!("Nothing answers on {addr}: {e}"))?;
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost:{}\r\nUser-Agent: Sawit\r\nAccept: application/json, */*\r\nConnection: close\r\n",
            addr.port()
        );
        match body {
            Some((bytes, kind)) => head.push_str(&format!(
                "Content-Type: {kind}\r\nContent-Length: {}\r\n",
                bytes.len()
            )),
            None if method != "GET" => head.push_str("Content-Length: 0\r\n"),
            None => {}
        }
        head.push_str("\r\n");
        let send = async {
            stream.write_all(head.as_bytes()).await?;
            if let Some((bytes, _)) = body {
                stream.write_all(bytes).await?;
            }
            stream.flush().await
        };
        send.await
            .map_err(|e| format!("Sending to {addr} failed: {e}"))?;
        read_response(&mut stream, limit).await
    };
    tokio::time::timeout(wait, exchange)
        .await
        .map_err(|_| format!("{addr} didn't answer within {} s.", wait.as_secs_f32()))?
}

/// The head of an answer: its status, content type and how its body is framed.
struct Head {
    status: u16,
    content_type: Option<String>,
    length: Option<usize>,
    chunked: bool,
}

fn parse_head(head: &[u8]) -> Result<Head, String> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.strip_prefix("HTTP/1."))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or("The server didn't answer with HTTP.")?;
    let mut head = Head {
        status,
        content_type: None,
        length: None,
        chunked: false,
    };
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-type" => head.content_type = Some(value.to_string()),
            "content-length" => head.length = value.parse().ok(),
            "transfer-encoding" => head.chunked = value.to_ascii_lowercase().contains("chunked"),
            _ => {}
        }
    }
    Ok(head)
}

async fn read_response(stream: &mut TcpStream, limit: usize) -> Result<Response, String> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 16 * 1024];
    let mut head: Option<(usize, Head)> = None;
    let mut cut = false;
    loop {
        let n = stream
            .read(&mut chunk)
            .await
            .map_err(|e| format!("Reading the answer failed: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if head.is_none()
            && let Some(end) = position(&buf, b"\r\n\r\n")
        {
            head = Some((end + 4, parse_head(&buf[..end])?));
        }
        match &head {
            None if buf.len() > 64 * 1024 => return Err("The answer's head is too long.".into()),
            None => {}
            Some((start, h)) => {
                let body = &buf[*start..];
                // A chunked body is decoded only once it ends as its last chunk does, not after
                // every read of a long one.
                if h.length.is_some_and(|len| body.len() >= len)
                    || (h.chunked && body.ends_with(b"0\r\n\r\n") && dechunk(body).1)
                    || h.status == 204
                    || h.status == 304
                {
                    break;
                }
                if body.len() > limit + 64 * 1024 {
                    cut = true;
                    break;
                }
            }
        }
    }
    let (start, h) = head.ok_or("The server closed the connection without an answer.")?;
    let raw = &buf[start..];
    let mut body = if h.chunked {
        dechunk(raw).0
    } else {
        match h.length {
            Some(len) if raw.len() >= len => raw[..len].to_vec(),
            Some(_) => {
                cut = true;
                raw.to_vec()
            }
            None => raw.to_vec(),
        }
    };
    if body.len() > limit {
        body.truncate(limit);
        cut = true;
    }
    Ok(Response {
        status: h.status,
        content_type: h.content_type,
        body,
        truncated: cut,
    })
}

pub(super) fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A chunked body decoded as far as it has come, and whether its last chunk has.
fn dechunk(mut raw: &[u8]) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    loop {
        let Some(line_end) = position(raw, b"\r\n") else {
            return (out, false);
        };
        let size_text = String::from_utf8_lossy(&raw[..line_end]);
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let Ok(size) = usize::from_str_radix(size_text, 16) else {
            return (out, false);
        };
        if size == 0 {
            return (out, true);
        }
        let data = &raw[line_end + 2..];
        if data.len() < size {
            out.extend_from_slice(data);
            return (out, false);
        }
        out.extend_from_slice(&data[..size]);
        raw = data.get(size + 2..).unwrap_or_default();
    }
}
