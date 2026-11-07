//! A small HTTPS GET in Rust (rustls, the system's root certificates, HTTP/1.1), for the public
//! JSON APIs Sawit reads without a sign-in, such as npm's registry: no `curl` to run.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

/// The most an answer may be; past it the request fails.
const MAX_BODY: usize = 8 << 20;

/// The TLS settings every request shares: the system's roots, loaded once (it is slow).
fn config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            for cert in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(cert);
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("ring supports the default TLS versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

/// `https://host/path` split into its host, port and path.
fn split_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| format!("{url} isn't an https:// address."))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().map_err(|_| format!("Bad port in {url}"))?),
        None => (authority, 443),
    };
    if host.is_empty() {
        return Err(format!("{url} has no host."));
    }
    Ok((host.to_string(), port, path.to_string()))
}

/// The body of a `GET url`, blocking, within `timeout` for each step. A status other than 200
/// is an error that names it.
pub fn get(url: &str, timeout: Duration) -> Result<String, String> {
    let (host, port, path) = split_url(url)?;
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("{host}: {e}"))?
        .next()
        .ok_or_else(|| format!("{host} has no address."))?;
    let tcp = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("{host}: {e}"))?;
    tcp.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let name = ServerName::try_from(host.clone()).map_err(|e| format!("{host}: {e}"))?;
    let conn = ClientConnection::new(config(), name).map_err(|e| e.to_string())?;
    let mut tls = StreamOwned::new(conn, tcp);
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: sawit (https://sawit.dev)\r\nAccept: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n"
    );
    tls.write_all(request.as_bytes())
        .map_err(|e| format!("{host}: {e}"))?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        match tls.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > MAX_BODY {
                    return Err(format!("{host} sent more than {} MB.", MAX_BODY >> 20));
                }
            }
            // Servers that close without TLS's close_notify: what came is the answer.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("{host}: {e}")),
        }
    }
    parse_response(&raw)
}

/// An HTTP/1.1 response's body, de-chunked, when its status is 200.
pub fn parse_response(raw: &[u8]) -> Result<String, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("The answer has no headers.")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = &raw[split + 4..];
    let mut lines = head.lines();
    let status = lines.next().unwrap_or_default();
    let code: u16 = status
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| format!("Not an HTTP answer: {status}"))?;
    if code != 200 {
        return Err(format!("The server answered {code}."));
    }
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    String::from_utf8(body).map_err(|_| "The answer isn't text.".to_string())
}

fn dechunk(mut body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let end = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("A chunk has no size.")?;
        let size_text = String::from_utf8_lossy(&body[..end]);
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| "A chunk size is bad.")?;
        body = &body[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if body.len() < size {
            return Err("The answer was cut short.".into());
        }
        out.extend_from_slice(&body[..size]);
        body = body.get(size + 2..).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_split() {
        assert_eq!(
            split_url("https://registry.npmjs.org/create-better-t-stack/latest").unwrap(),
            (
                "registry.npmjs.org".into(),
                443,
                "/create-better-t-stack/latest".into()
            )
        );
        assert_eq!(
            split_url("https://h:8443").unwrap(),
            ("h".into(), 8443, "/".into())
        );
        assert!(split_url("http://h/").is_err());
    }

    #[test]
    fn responses_parse() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        assert_eq!(parse_response(plain).unwrap(), "{\"a\":1}");
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked).unwrap(), "{\"a\":1}");
        let missing = b"HTTP/1.1 404 Not Found\r\n\r\nnope";
        assert!(parse_response(missing).unwrap_err().contains("404"));
    }
}
