//! Fetching from the web in Rust, for the browser's favicons ([`favicon`]) and its SEO and
//! agent-readiness scan ([`seo`]): one HTTP client over `rustls` (ring, the Mac's trusted roots),
//! shared by everything that asks, and the page's `<head>` read with `html5ever` ([`head`]).
//!
//! The site's files are fetched here, not from the page, so the page's Content Security Policy
//! can't block them, and local dev servers (`localhost`, `*.local`, LAN addresses) answer the same
//! way production sites do.

pub mod favicon;
pub mod head;

#[cfg(test)]
mod tests;

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

/// How Sawit introduces itself: a browser's agent string with Sawit's name at the end, so sites
/// answer it as they answer a browser.
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15 Sawit";

/// The most a body is read: a bigger one is cut off there ([`Fetched::truncated`]).
pub const MAX_BODY: usize = 8 * 1024 * 1024;

/// The longest one request is waited for, by default.
pub const TIMEOUT: Duration = Duration::from_secs(12);

/// What one request answered.
#[derive(Clone, Debug, Default)]
pub struct Fetched {
    /// The address asked for.
    pub requested: String,
    /// The address that answered, after redirects.
    pub url: String,
    /// The HTTP status; 0 when nothing answered ([`Self::error`]).
    pub status: u16,
    /// The response headers, names in lower case, in the order sent.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The body was longer than the limit and was cut off.
    pub truncated: bool,
    /// The redirects followed, each `(status, location)`.
    pub redirects: Vec<(u16, String)>,
    pub elapsed: Duration,
    /// Why nothing answered: the connection failed, timed out, or TLS was refused.
    pub error: Option<String>,
}

impl Fetched {
    /// A 2xx answer.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The first header named `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Every header named `name` (any case).
    pub fn headers_named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a str> + 'a {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .filter(move |(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The media type without its parameters, lower case (`text/html`).
    pub fn mime(&self) -> String {
        self.header("content-type")
            .and_then(|t| t.split(';').next())
            .map(|t| t.trim().to_ascii_lowercase())
            .unwrap_or_default()
    }

    /// The body as text (UTF-8, bad bytes replaced).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// How a request is made.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: reqwest::Method,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
    /// The most of the body read.
    pub max_body: usize,
    /// Follow redirects (up to 10), recording each; otherwise the 3xx is the answer.
    pub follow: bool,
}

impl Default for Request {
    fn default() -> Self {
        Request {
            method: reqwest::Method::GET,
            headers: Vec::new(),
            body: None,
            timeout: TIMEOUT,
            max_body: MAX_BODY,
            follow: true,
        }
    }
}

impl Request {
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn method(mut self, method: reqwest::Method) -> Self {
        self.method = method;
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn max_body(mut self, max: usize) -> Self {
        self.max_body = max;
        self
    }
}

/// The shared client. Redirects are followed by [`fetch_with`] itself, so each hop is recorded.
pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map(|b| b.with_root_certificates(roots).with_no_client_auth());
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(8))
            .http1_only();
        if let Ok(tls) = tls {
            builder = builder.tls_backend_preconfigured(tls);
        }
        builder.build().unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// GETs `url`, following redirects.
pub async fn fetch(url: &str) -> Fetched {
    fetch_with(url, Request::default()).await
}

/// Makes `request` to `url`. Never fails: what went wrong is in [`Fetched::error`].
pub async fn fetch_with(url: &str, request: Request) -> Fetched {
    let started = Instant::now();
    let mut out = Fetched {
        requested: url.to_string(),
        url: url.to_string(),
        ..Default::default()
    };
    let mut current = match Url::parse(url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => u,
        Ok(u) => {
            out.error = Some(format!("{} addresses can't be fetched.", u.scheme()));
            return out;
        }
        Err(e) => {
            out.error = Some(format!("“{url}” isn't an address: {e}."));
            return out;
        }
    };
    let mut headers = HeaderMap::new();
    for (name, value) in &request.headers {
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.append(n, v);
        }
    }
    let deadline = started + request.timeout;
    for _ in 0..=10 {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            out.error = Some(format!("No answer in {} s.", request.timeout.as_secs()));
            break;
        }
        let mut req = client()
            .request(request.method.clone(), current.clone())
            .headers(headers.clone())
            .timeout(left);
        if let Some(body) = &request.body {
            req = req.body(body.clone());
        }
        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                out.error = Some(describe_error(&e));
                break;
            }
        };
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|l| l.to_str().ok())
            .and_then(|l| current.join(l).ok());
        if request.follow
            && (300..400).contains(&status)
            && status != 304
            && let Some(next) = location
        {
            out.redirects.push((status, next.to_string()));
            current = next;
            continue;
        }
        out.url = current.to_string();
        out.status = status;
        out.headers = response
            .headers()
            .iter()
            .map(|(n, v)| {
                (
                    n.as_str().to_ascii_lowercase(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        if request.method != reqwest::Method::HEAD {
            let (body, truncated, error) = read_body(response, request.max_body, deadline).await;
            out.body = body;
            out.truncated = truncated;
            out.error = error;
        }
        out.elapsed = started.elapsed();
        return out;
    }
    if out.error.is_none() {
        out.error = Some("Too many redirects.".into());
    }
    out.elapsed = started.elapsed();
    out
}

async fn read_body(
    mut response: reqwest::Response,
    max: usize,
    deadline: Instant,
) -> (Vec<u8>, bool, Option<String>) {
    let mut body = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, response.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                let room = max.saturating_sub(body.len());
                if chunk.len() > room {
                    body.extend_from_slice(&chunk[..room]);
                    return (body, true, None);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Ok(None)) => return (body, false, None),
            Ok(Err(e)) => return (body, false, Some(describe_error(&e))),
            Err(_) => return (body, true, Some("The body took too long to arrive.".into())),
        }
    }
}

/// What went wrong, in a sentence: the deepest cause reqwest knows ("connection refused").
fn describe_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        return "No answer in time.".into();
    }
    let mut cause: &dyn std::error::Error = e;
    while let Some(next) = cause.source() {
        cause = next;
    }
    let text = cause.to_string();
    if e.is_connect() {
        format!("Couldn't connect: {text}.")
    } else {
        format!("The request failed: {text}.")
    }
}

/// Whether `url` is a local or private address: `localhost`, `127.0.0.1`, `::1`, `*.local`,
/// `*.localhost`, `*.test`, and the private and link-local ranges. These are never sent to an
/// outside service.
pub fn is_local(url: &str) -> bool {
    let Ok(u) = Url::parse(url) else {
        return false;
    };
    match u.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost"
                || d.ends_with(".localhost")
                || d.ends_with(".local")
                || d.ends_with(".test")
                || d.ends_with(".internal")
                || d.ends_with(".lan")
                || d.ends_with(".home.arpa")
                || !d.contains('.')
        }
        Some(url::Host::Ipv4(ip)) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || (ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]))
        }
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
        None => true,
    }
}

/// The origin of `url` (`https://example.com`, with the port when it isn't the scheme's own).
pub fn origin(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    match u.origin() {
        url::Origin::Tuple(..) => Some(u.origin().ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}
