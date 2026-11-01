//! Tests of asset saving: the shared fixtures here, the tests by topic in the submodules.

mod curl;
mod downloads;
mod hosts;
mod naming;
mod policy;

use std::path::Path;

use super::*;

fn asset(kind: AssetKind, url: Option<&str>, svg: Option<&str>, label: &str) -> AssetRef {
    AssetRef {
        kind,
        url: url.map(String::from),
        svg: svg.map(String::from),
        label: label.into(),
        width: None,
        height: None,
        used_on: vec![],
        logo: false,
    }
}

fn enc(b: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// A server on this Mac answering `/gz/<n>` with `n` zero bytes gzip-encoded (whatever
/// the request asked for) and `/plain/<n>` with `n` zero bytes as they are, both with no
/// `Content-Length`; its origin.
fn zeros_server() -> String {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let (kind, n) = path
                    .trim_start_matches('/')
                    .split_once('/')
                    .unwrap_or(("", "0"));
                let n: usize = n
                    .split(['/', '.'])
                    .next()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0);
                let zeros = vec![0u8; n];
                let (encoding, body) = if kind == "gz" {
                    let mut gz =
                        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
                    gz.write_all(&zeros).unwrap();
                    ("Content-Encoding: gzip\r\n", gz.finish().unwrap())
                } else {
                    ("", zeros)
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/css\r\n{encoding}Connection: close\r\n\r\n"
                );
                let _ = stream.write_all(reply.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    origin
}

/// The bytes in `dir`'s files, all the way down.
fn bytes_in(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => bytes_in(&e.path()),
                _ => e.metadata().map_or(0, |m| m.len()),
            })
            .sum()
    })
}
