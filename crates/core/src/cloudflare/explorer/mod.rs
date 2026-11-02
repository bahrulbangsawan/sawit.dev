//! Cloudflare's Local Explorer: the page a Workers dev server (`wrangler dev`, `vite` with the
//! Cloudflare plugin, `cf dev`) serves on its own origin at `/cdn-cgi/local/explorer`, to browse
//! and edit the data of its local bindings, and the API beside it at
//! `/cdn-cgi/local/explorer/api`, whose `GET` answers with its OpenAPI document.
//!
//! Nothing announces a running one, so Sawit asks: each dev server the Ports panel lists is sent
//! `GET /cdn-cgi/local/explorer/api` on loopback, and one that answers with an OpenAPI document
//! serves an explorer. Answers are kept per port and process ([`Cache`]), so a server is asked
//! again only after a while, or once another process holds its port. Only loopback addresses are
//! ever asked or called: a dev server bound to the LAN alone is left alone, and so is every other
//! host. The docs say nothing of authentication, and the API needs none on loopback.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::ports::{BindAddr, PortJob};

/// Where a dev server serves Local Explorer's page.
pub const PAGE_PATH: &str = "/cdn-cgi/local/explorer";
/// Where it serves Local Explorer's API.
pub const API_PATH: &str = "/cdn-cgi/local/explorer/api";

/// How long a dev server gets to answer the probe. It is on this Mac, so a short wait.
const PROBE_TIMEOUT: Duration = Duration::from_millis(800);
/// How long one API call gets: a D1 query or an R2 upload can take a moment.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// The OpenAPI document is about 60 KB; an answer much bigger than this isn't one.
const PROBE_LIMIT: usize = 4 << 20;
/// At most this much of an API answer is read; the rest is cut and said to be.
pub const BODY_LIMIT: usize = 8 << 20;
/// At most this many of a dev server's ports are asked (Wrangler also listens on its inspector
/// and on internal ports).
const PORTS_PER_SERVER: usize = 8;
/// How long an answer is kept: a found explorer a minute; a miss a few seconds at first, as a
/// server that has just started may answer once its Worker is up, then twice as long after each
/// miss up to a few minutes, so a server that has none isn't asked every few seconds.
const FOUND_FOR: Duration = Duration::from_secs(60);
const MISSED_FOR: Duration = Duration::from_secs(8);
const MISSED_AT_MOST: Duration = Duration::from_secs(240);

/// What in a dev server's command lines says it may be a Workers one: Wrangler and the runtime it
/// starts, Vite (the Cloudflare plugin runs inside it) and the frameworks that run Vite, and `cf`.
/// Other servers (Django, Rails, Next.js) aren't asked, so their logs don't fill with 404s.
const WORKERS_HINTS: &[&str] = &[
    "wrangler",
    "workerd",
    "miniflare",
    "vite",
    "react-router",
    "astro",
    "vinext",
    "cf dev",
    "/cf ",
];

/// What a Local Explorer's OpenAPI document says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// `info.title`: `Local Explorer API`.
    pub title: Option<String>,
    /// `info.version`.
    pub version: Option<String>,
    /// Its operations, `GET /storage/kv/namespaces`, in the document's order.
    pub operations: Vec<String>,
}

impl Spec {
    /// Whether the API has `operation` (`GET /local/workers`).
    pub fn has(&self, operation: &str) -> bool {
        self.operations.iter().any(|o| o == operation)
    }
}

/// `body` as an OpenAPI 3 document (`openapi` and `paths`), which is what a dev server with Local
/// Explorer answers its API's root with; `None` for anything else (a 404 page, the app's own
/// HTML, other JSON).
pub fn parse_spec(body: &[u8]) -> Option<Spec> {
    let doc: Value = serde_json::from_slice(body).ok()?;
    let openapi = doc.get("openapi")?.as_str()?;
    if !openapi.starts_with("3.") {
        return None;
    }
    let paths = doc.get("paths")?.as_object()?;
    let info = |key: &str| {
        doc.pointer(&format!("/info/{key}"))?
            .as_str()
            .map(str::to_string)
    };
    let mut operations = Vec::new();
    for (path, item) in paths {
        for method in ["get", "post", "put", "patch", "delete"] {
            if item.get(method).is_some() {
                operations.push(format!("{} {path}", method.to_uppercase()));
            }
        }
    }
    Some(Spec {
        title: info("title"),
        version: info("version"),
        operations,
    })
}

/// A running Local Explorer: a dev server that answered the probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explorer {
    pub port: u16,
    /// The loopback address it answered on.
    pub addr: SocketAddr,
    /// The dev server's process: the command it was started with, as the Ports panel groups it.
    pub pid: u32,
    /// That command (`pnpm dev`).
    pub label: String,
    /// The folder it runs in.
    pub cwd: Option<String>,
    pub spec: Spec,
}

impl Explorer {
    /// The page, as Sawit's browser opens it.
    pub fn url(&self) -> String {
        page_url(self.port)
    }

    /// The API's root.
    pub fn api_url(&self) -> String {
        format!("http://localhost:{}{API_PATH}", self.port)
    }

    /// Whether the dev server runs from inside `folder` (a project, or one of its packages).
    /// `lsof` gives the folder with its links resolved (`/private/tmp/…` for `/tmp/…`), so
    /// `folder` is taken both ways.
    pub fn runs_in(&self, folder: &Path) -> bool {
        let Some(cwd) = self.cwd.as_deref().map(Path::new) else {
            return false;
        };
        cwd.starts_with(folder) || cwd.starts_with(crate::files::resolve(folder))
    }
}

/// Local Explorer's page on the dev server at `port`.
pub fn page_url(port: u16) -> String {
    format!("http://localhost:{port}{PAGE_PATH}")
}

/// The loopback address to reach a server bound to `bind` on: 127.0.0.1 for IPv4 binds, ::1 for
/// IPv6 ones (Vite listens on `localhost`, often ::1 alone). `None` for a server bound to another
/// address only, which Sawit never asks.
pub fn loopback(bind: BindAddr) -> Option<IpAddr> {
    match bind {
        BindAddr::AnyV4 | BindAddr::LoopbackV4 => Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        BindAddr::AnyV6 | BindAddr::LoopbackV6 => Some(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        BindAddr::Specific(ip) if ip.is_loopback() => Some(ip),
        BindAddr::Specific(_) => None,
    }
}

/// Asks the dev server at `addr` for Local Explorer's OpenAPI document: `Some` when it serves
/// one, `None` when it doesn't or doesn't answer in time. Refused before anything is sent when
/// `addr` isn't a loopback address.
pub async fn probe(addr: SocketAddr) -> Result<Option<Spec>, String> {
    refuse_remote(addr)?;
    Ok(
        match http(addr, "GET", API_PATH, None, PROBE_TIMEOUT, PROBE_LIMIT).await {
            Ok(r) if r.status == 200 && !r.truncated => parse_spec(&r.body),
            _ => None,
        },
    )
}

/// What the probe found per port, with the process that held the port when it was asked.
#[derive(Debug, Default)]
pub struct Cache {
    seen: HashMap<u16, Seen>,
}

#[derive(Debug)]
struct Seen {
    pid: u32,
    spec: Option<Spec>,
    at: Instant,
    /// Misses in a row while this process held the port.
    misses: u32,
}

impl Cache {
    /// What is known of `port` while `pid` holds it, at `now`: `Some(found)` while the answer is
    /// fresh (a found explorer for a minute, a miss for a while that grows with each miss);
    /// `None` when it has to be asked again, or another process holds the port now.
    pub fn known(&self, port: u16, pid: u32, now: Instant) -> Option<Option<&Spec>> {
        let seen = self.seen.get(&port).filter(|s| s.pid == pid)?;
        let keep = if seen.spec.is_some() {
            FOUND_FOR
        } else {
            let doubled = MISSED_FOR.saturating_mul(1 << seen.misses.saturating_sub(1).min(8));
            doubled.min(MISSED_AT_MOST)
        };
        (now.saturating_duration_since(seen.at) < keep).then_some(seen.spec.as_ref())
    }

    /// Keeps the answer of `port`, held by `pid`, asked at `now`.
    pub fn record(&mut self, port: u16, pid: u32, spec: Option<Spec>, now: Instant) {
        let before = self
            .seen
            .get(&port)
            .filter(|s| s.pid == pid && s.spec.is_none());
        let misses = match spec {
            Some(_) => 0,
            None => before.map_or(0, |s| s.misses) + 1,
        };
        self.seen.insert(
            port,
            Seen {
                pid,
                spec,
                at: now,
                misses,
            },
        );
    }

    /// Forgets the ports nothing listens on any more.
    pub fn retain(&mut self, listening: impl Fn(u16) -> bool) {
        self.seen.retain(|port, _| listening(*port));
    }
}

/// The answers of every probe of this process: the app's Ports panels and the MCP server share
/// them.
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

/// Whether `job`, a listening process, may be a Workers dev server worth asking: a dev server
/// (not Docker or a database) whose command lines name a Workers tool ([`WORKERS_HINTS`]), or
/// that runs in a folder with a Worker config.
pub fn may_serve(job: &PortJob) -> bool {
    if !job.dev || job.is_docker() || job.is_daemon() {
        return false;
    }
    let named = std::iter::once(job.cmdline.as_str())
        .chain(job.listeners.iter().map(|(_, c)| c.as_str()))
        .any(|line| WORKERS_HINTS.iter().any(|hint| line.contains(hint)));
    named
        || job.cwd.as_deref().is_some_and(|cwd| {
            super::CONFIG_FILES
                .iter()
                .any(|f| Path::new(cwd).join(f).is_file())
        })
}

/// The Local Explorers among `jobs`, the dev servers [`crate::probe::listening_jobs`] lists, by
/// port; one per server, on its lowest port that serves one (`wrangler dev` serves it on an
/// internal port too). Only servers that [`may_serve`] one are asked.
pub async fn find(jobs: &[PortJob]) -> Vec<Explorer> {
    find_with(&CACHE, jobs).await
}

/// [`find`] with the answers kept in `cache`.
pub async fn find_with(cache: &Mutex<Cache>, jobs: &[PortJob]) -> Vec<Explorer> {
    let lock = || cache.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let mut answers: Vec<(usize, u16, SocketAddr, Option<Spec>)> = Vec::new();
    let mut asks: Vec<(usize, u16, SocketAddr)> = Vec::new();
    {
        let mut cache = lock();
        cache.retain(|port| jobs.iter().any(|j| j.ports.iter().any(|(p, _)| *p == port)));
        for (ix, job) in jobs.iter().enumerate() {
            if !may_serve(job) {
                continue;
            }
            for &(port, bind) in job.ports.iter().take(PORTS_PER_SERVER) {
                let Some(ip) = loopback(bind) else { continue };
                let addr = SocketAddr::new(ip, port);
                match cache.known(port, job.root, now) {
                    Some(spec) => answers.push((ix, port, addr, spec.cloned())),
                    None => asks.push((ix, port, addr)),
                }
            }
        }
    }
    let asked = futures::future::join_all(asks.iter().map(|(_, _, addr)| probe(*addr))).await;
    {
        let mut cache = lock();
        let now = Instant::now();
        for ((ix, port, addr), answer) in asks.into_iter().zip(asked) {
            let spec = answer.ok().flatten();
            cache.record(port, jobs[ix].root, spec.clone(), now);
            answers.push((ix, port, addr, spec));
        }
    }
    answers.sort_by_key(|(ix, port, ..)| (*ix, *port));
    let mut found: Vec<Explorer> = Vec::new();
    for (ix, port, addr, spec) in answers {
        let (Some(spec), job) = (spec, &jobs[ix]) else {
            continue;
        };
        if found.iter().any(|e| e.pid == job.root) {
            continue;
        }
        found.push(Explorer {
            port,
            addr,
            pid: job.root,
            label: job.label.clone(),
            cwd: job.cwd.clone(),
            spec,
        });
    }
    found.sort_by_key(|e| e.port);
    found
}

/// The explorers of `explorers` whose dev server runs inside `folder`.
pub fn running_in<'a>(explorers: &'a [Explorer], folder: &Path) -> Vec<&'a Explorer> {
    explorers.iter().filter(|e| e.runs_in(folder)).collect()
}

/// One of Local Explorer's API calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    /// `GET`, `post`, ….
    pub fn parse(text: &str) -> Result<Method, String> {
        Ok(match text.trim().to_ascii_uppercase().as_str() {
            "" | "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "PATCH" => Method::Patch,
            "DELETE" => Method::Delete,
            other => {
                return Err(format!(
                    "Local Explorer's API takes GET, POST, PUT, PATCH or DELETE, not {other}."
                ));
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }

    /// Whether the call can change the dev server's data: anything but `GET`. Even a `POST` that
    /// only reads (a D1 `SELECT`, the observability query) is counted, as nothing tells them apart
    /// before it runs.
    pub fn writes(self) -> bool {
        self != Method::Get
    }
}

/// Refuses a call that can change data, unless the caller confirmed it: it writes to the dev
/// server's local KV, R2, D1, Durable Objects or Workflows (or, through a binding marked remote,
/// to the Cloudflare account's), and nothing undoes that.
pub fn check_confirmed(method: Method, confirm: bool) -> Result<(), String> {
    if method.writes() && !confirm {
        return Err(format!(
            "Refused: {} can change the dev server's data (KV keys, R2 objects, D1 rows, Durable Objects, Workflow instances; a binding marked remote reaches your Cloudflare account), which can't be undone. Read first with GET, then pass confirm: true.",
            method.as_str()
        ));
    }
    Ok(())
}

/// The request path for `path`, an API path given from the API's root (`storage/kv/namespaces`,
/// `local/scheduled?worker=api`) or whole (`/cdn-cgi/local/explorer/api/d1/database`): always
/// under the API. Refused: an absolute URL, a `.` or `..` segment written plainly or
/// percent-encoded (once or more), a backslash, whitespace, control characters or a fragment.
pub fn api_path(path: &str) -> Result<String, String> {
    let given = path.trim();
    let refuse = |why: &str| Err(format!("Refused path “{given}”: {why}."));
    if given.contains("://") || given.starts_with("//") {
        return refuse("give a path under the API, not a URL");
    }
    if given
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\' || c == '#')
    {
        return refuse("no spaces, backslashes, control characters or fragments");
    }
    let rest = match given.strip_prefix(API_PATH) {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '?']) => rest,
        _ => given,
    };
    let rest = rest.trim_start_matches('/');
    let route = rest.split('?').next().unwrap_or_default();
    // Decoded again and again, so `%252e%252e` can't come out as `..` at a server that decodes
    // twice.
    let mut decoded = route.to_string();
    for _ in 0..4 {
        let segments_bad = decoded
            .split('/')
            .any(|segment| segment == ".." || segment == ".");
        if segments_bad || decoded.contains('\\') {
            return refuse("it can't leave the Local Explorer API (no . or .. segments)");
        }
        let next = percent_decode(&decoded);
        if next == decoded {
            break;
        }
        decoded = next;
    }
    Ok(if rest.is_empty() {
        API_PATH.to_string()
    } else if rest.starts_with('?') {
        format!("{API_PATH}{rest}")
    } else {
        format!("{API_PATH}/{rest}")
    })
}

/// `%xx` escapes decoded, as bytes, then read as UTF-8 with anything else replaced.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        if bytes[ix] == b'%'
            && let Some(hex) = text.get(ix + 1..ix + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            ix += 3;
            continue;
        }
        out.push(bytes[ix]);
        ix += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What an API call answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    /// The answer was longer than [`BODY_LIMIT`] and `body` holds its start.
    pub truncated: bool,
}

/// Calls Local Explorer's API on the dev server at `addr`: `method` on `path` ([`api_path`]),
/// with `body` and its content type. Only loopback addresses are called. It doesn't ask for
/// confirmation: callers check writes with [`check_confirmed`] first.
pub async fn request(
    addr: SocketAddr,
    method: Method,
    path: &str,
    body: Option<(&[u8], &str)>,
) -> Result<Response, String> {
    refuse_remote(addr)?;
    let path = api_path(path)?;
    http(
        addr,
        method.as_str(),
        &path,
        body,
        REQUEST_TIMEOUT,
        BODY_LIMIT,
    )
    .await
}

/// A Worker the dev server runs, as `GET /local/workers` lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalWorker {
    pub name: String,
    /// The Worker the dev server was started for (others are ones it binds to).
    pub is_self: bool,
    /// Its bindings, as (kind, binding name): (`d1`, `DB`).
    pub bindings: Vec<(String, String)>,
}

/// The Workers the dev server at `addr` runs, when its API lists them (`GET /local/workers`,
/// which Wrangler 4.118 has); empty otherwise.
pub async fn local_workers(addr: SocketAddr) -> Vec<LocalWorker> {
    match request(addr, Method::Get, "local/workers", None).await {
        Ok(r) if r.status == 200 => parse_workers(&r.body),
        _ => Vec::new(),
    }
}

/// `GET /local/workers`'s answer: `{"result": [{"name", "isSelf", "bindings": {"d1": [{"bindingName"}]}}]}`.
pub fn parse_workers(body: &[u8]) -> Vec<LocalWorker> {
    let Ok(doc) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Some(list) = doc.get("result").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|w| {
            let mut bindings = Vec::new();
            for (kind, items) in w
                .get("bindings")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                for item in items.as_array().into_iter().flatten() {
                    if let Some(name) = item.get("bindingName").and_then(Value::as_str) {
                        bindings.push((kind.clone(), name.to_string()));
                    }
                }
            }
            Some(LocalWorker {
                name: w.get("name")?.as_str()?.to_string(),
                is_self: w.get("isSelf").and_then(Value::as_bool).unwrap_or(false),
                bindings,
            })
        })
        .collect()
}

fn refuse_remote(addr: SocketAddr) -> Result<(), String> {
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
async fn http(
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

fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;

    /// A cut-down OpenAPI document, as Wrangler 4.118's Local Explorer serves at its API's root.
    const SPEC: &str = r#"{"openapi":"3.0.3","info":{"title":"Local Explorer API","version":"0.0.1"},"servers":[{"url":"/cdn-cgi/local/explorer/api"}],"paths":{"/storage/kv/namespaces":{"get":{}},"/d1/database/{database_id}/raw":{"post":{}},"/local/workers":{"get":{}}}}"#;
    const WORKERS: &str = r#"{"success":true,"result":[{"isSelf":true,"name":"shop-api","bindings":{"kv":[{"id":"a","bindingName":"CACHE"}],"d1":[{"id":"1","bindingName":"DB"}],"r2":[]}}]}"#;

    /// What the fake server got: each request's head and body.
    type Seen = Arc<Mutex<Vec<String>>>;

    /// A dev server on 127.0.0.1:0 that answers with `answer(request)`: the status, the content
    /// type and the body, sent chunked when `chunked`. It counts and keeps what it is sent.
    async fn serve(
        answer: fn(&str) -> (u16, &'static str, String),
        chunked: bool,
    ) -> (SocketAddr, Arc<AtomicUsize>, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let seen: Seen = Arc::default();
        let (h, s) = (hits.clone(), seen.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                h.fetch_add(1, Ordering::SeqCst);
                let s = s.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    // The head, then as much body as Content-Length says.
                    loop {
                        let n = socket.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(end) = position(&buf, b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                            let len = head
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .and_then(|v| v.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            if buf.len() >= end + 4 + len {
                                break;
                            }
                        }
                    }
                    let request = String::from_utf8_lossy(&buf).into_owned();
                    s.lock().unwrap().push(request.clone());
                    let (status, kind, body) = answer(&request);
                    let reply = if chunked {
                        let (a, b) = body.split_at(body.len() / 2);
                        format!(
                            "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
                            a.len(),
                            b.len()
                        )
                    } else {
                        format!(
                            "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        )
                    };
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        (addr, hits, seen)
    }

    fn explorer_server(request: &str) -> (u16, &'static str, String) {
        let line = request.lines().next().unwrap_or_default();
        match line.split_whitespace().nth(1).unwrap_or_default() {
            API_PATH => (200, "application/json", SPEC.into()),
            p if p == format!("{API_PATH}/local/workers") => {
                (200, "application/json", WORKERS.into())
            }
            p if p.starts_with(API_PATH) => (
                200,
                "application/json",
                format!(r#"{{"method":"{}"}}"#, line.split(' ').next().unwrap()),
            ),
            _ => (404, "text/html", "<h1>Not found</h1>".into()),
        }
    }

    fn app_server(_: &str) -> (u16, &'static str, String) {
        (
            200,
            "text/html",
            "<!doctype html><title>My app</title>".into(),
        )
    }

    fn job(root: u32, ports: &[u16]) -> PortJob {
        PortJob {
            root,
            pids: vec![root],
            label: "pnpm dev".into(),
            tool: "pnpm".into(),
            cmdline: "pnpm dev".into(),
            listeners: vec![(root + 1, "workerd serve --binary".into())],
            ports: ports.iter().map(|p| (*p, BindAddr::LoopbackV4)).collect(),
            cwd: Some("/Users/me/shop/apps/api".into()),
            dev: true,
        }
    }

    /// The probe finds a fake dev server's explorer from its OpenAPI document, chunked or not, and
    /// finds none on a server that answers with its own HTML or nothing at all.
    #[tokio::test]
    async fn the_probe_finds_an_explorer_by_its_openapi_document() {
        for chunked in [false, true] {
            let (addr, _, seen) = serve(explorer_server, chunked).await;
            let spec = probe(addr).await.unwrap().expect("an explorer");
            assert_eq!(spec.title.as_deref(), Some("Local Explorer API"));
            assert!(spec.has("GET /local/workers"));
            assert!(spec.has("POST /d1/database/{database_id}/raw"));
            let request = seen.lock().unwrap()[0].clone();
            assert!(request.starts_with("GET /cdn-cgi/local/explorer/api HTTP/1.1\r\n"));
            assert!(request.contains(&format!("Host: localhost:{}", addr.port())));
        }
        let (app, _, _) = serve(app_server, false).await;
        assert_eq!(probe(app).await.unwrap(), None);
        // A port nothing listens on any more.
        let closed = {
            let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap()
        };
        assert_eq!(probe(closed).await.unwrap(), None);
        // Other JSON isn't an explorer.
        assert_eq!(parse_spec(br#"{"openapi":"3.0.0"}"#), None);
        assert_eq!(parse_spec(br#"{"swagger":"2.0","paths":{}}"#), None);
    }

    /// Each dev server is asked once while the same process holds its port: a second look reads
    /// the cache, and a new process on the port is asked again. A server serving it on several
    /// ports is one explorer, on its lowest; a plain app and a database are no explorer.
    #[tokio::test]
    async fn explorers_are_found_once_per_port_and_process() {
        let (api, hits, _) = serve(explorer_server, false).await;
        let (internal, _, _) = serve(explorer_server, false).await;
        let (app, app_hits, _) = serve(app_server, false).await;
        let cache = Mutex::new(Cache::default());
        let mut db = job(30, &[5432]);
        db.dev = false;
        let mut jobs = vec![
            job(10, &[api.port(), internal.port()]),
            job(20, &[app.port()]),
            db,
        ];
        jobs[0].ports.sort_by_key(|(port, _)| *port);
        let lowest = jobs[0].ports[0].0;

        let found = find_with(&cache, &jobs).await;
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].port, found[0].pid), (lowest, 10));
        assert_eq!(
            found[0].url(),
            format!("http://localhost:{lowest}/cdn-cgi/local/explorer")
        );
        assert!(found[0].runs_in(Path::new("/Users/me/shop")));
        assert!(!found[0].runs_in(Path::new("/Users/me/blog")));
        let (first, app_first) = (hits.load(Ordering::SeqCst), app_hits.load(Ordering::SeqCst));
        assert_eq!(app_first, 1);

        let again = find_with(&cache, &jobs).await;
        assert_eq!(again, found);
        assert_eq!(hits.load(Ordering::SeqCst), first, "asked again");
        assert_eq!(
            app_hits.load(Ordering::SeqCst),
            1,
            "the miss was asked again at once"
        );

        jobs[0].root = 11;
        let restarted = find_with(&cache, &jobs).await;
        assert_eq!(restarted[0].pid, 11);
        assert!(
            hits.load(Ordering::SeqCst) > first,
            "a new process wasn't asked"
        );
    }

    /// A miss is asked again after a few seconds, a find after a minute.
    #[test]
    fn answers_are_kept_for_a_while() {
        let mut cache = Cache::default();
        let t = Instant::now();
        let spec = parse_spec(SPEC.as_bytes()).unwrap();
        cache.record(8787, 1, Some(spec.clone()), t);
        cache.record(5173, 2, None, t);
        assert_eq!(
            cache.known(8787, 1, t + Duration::from_secs(30)),
            Some(Some(&spec))
        );
        assert_eq!(cache.known(8787, 1, t + Duration::from_secs(61)), None);
        assert_eq!(cache.known(8787, 9, t), None);
        assert_eq!(cache.known(5173, 2, t + Duration::from_secs(1)), Some(None));
        assert_eq!(cache.known(5173, 2, t + Duration::from_secs(9)), None);
        // Each miss in a row keeps the next one longer, up to a few minutes.
        cache.record(5173, 2, None, t);
        assert_eq!(cache.known(5173, 2, t + Duration::from_secs(9)), Some(None));
        assert_eq!(cache.known(5173, 2, t + Duration::from_secs(17)), None);
        for _ in 0..10 {
            cache.record(5173, 2, None, t);
        }
        assert_eq!(
            cache.known(5173, 2, t + Duration::from_secs(200)),
            Some(None)
        );
        assert_eq!(cache.known(5173, 2, t + Duration::from_secs(241)), None);
        // A find starts the count again.
        cache.record(5173, 2, Some(spec.clone()), t);
        cache.record(5173, 2, None, t);
        assert_eq!(cache.known(5173, 2, t + Duration::from_secs(9)), None);
        cache.retain(|port| port == 8787);
        assert_eq!(cache.known(5173, 2, t), None);
    }

    /// Only dev servers that may be Workers ones are asked: a Django server's log doesn't fill
    /// with 404s every few seconds.
    #[tokio::test]
    async fn only_workers_dev_servers_are_asked() {
        let (django, hits, _) = serve(app_server, false).await;
        let mut other = job(50, &[django.port()]);
        other.cmdline = "python manage.py runserver".into();
        other.label = other.cmdline.clone();
        other.listeners = vec![(50, "python manage.py runserver".into())];
        other.cwd = None;
        assert!(!may_serve(&other));
        assert!(
            find_with(&Mutex::default(), &[other.clone()])
                .await
                .is_empty()
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        for line in [
            "pnpm exec wrangler dev",
            "node /app/node_modules/.bin/vite",
            "cf dev",
        ] {
            other.cmdline = line.into();
            assert!(may_serve(&other), "{line}");
        }
        // A folder with a Worker config, whatever started the server.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("wrangler.toml"), "name = \"w\"\n").unwrap();
        other.cmdline = "npm start".into();
        other.cwd = Some(dir.path().display().to_string());
        assert!(may_serve(&other));
    }

    /// Nothing but loopback is ever asked or called: a server bound to a LAN address alone isn't
    /// probed, and a call to another host is refused before anything is sent.
    #[tokio::test]
    async fn addresses_other_than_loopback_are_refused() {
        assert_eq!(
            loopback(BindAddr::AnyV4),
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
        );
        assert_eq!(
            loopback(BindAddr::LoopbackV6),
            Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
        assert_eq!(
            loopback(BindAddr::Specific("192.168.1.5".parse().unwrap())),
            None
        );
        assert_eq!(
            loopback(BindAddr::Specific("127.0.0.2".parse().unwrap())),
            Some("127.0.0.2".parse().unwrap())
        );
        for remote in [
            "192.168.1.5:8787",
            "10.0.0.1:80",
            "[2001:db8::1]:8787",
            "1.1.1.1:443",
        ] {
            let addr: SocketAddr = remote.parse().unwrap();
            let probed = probe(addr).await;
            assert!(probed.unwrap_err().contains("isn't a loopback address"));
            let called = request(addr, Method::Get, "d1/database", None).await;
            assert!(called.unwrap_err().contains("isn't a loopback address"));
        }
        // A LAN-only dev server is skipped without a probe.
        let mut lan = job(40, &[8787]);
        lan.ports = vec![(8787, BindAddr::Specific("192.168.1.5".parse().unwrap()))];
        assert!(find_with(&Mutex::default(), &[lan]).await.is_empty());
    }

    /// Paths stay under the API: absolute URLs, `..` and `.` segments, encoded or not, and
    /// backslashes are refused; the API's own prefix and a query are kept.
    #[test]
    fn paths_that_leave_the_api_are_refused() {
        let ok = |p: &str| api_path(p).unwrap();
        assert_eq!(
            ok("storage/kv/namespaces"),
            format!("{API_PATH}/storage/kv/namespaces")
        );
        assert_eq!(ok("/d1/database"), format!("{API_PATH}/d1/database"));
        assert_eq!(ok(""), API_PATH);
        assert_eq!(ok("/"), API_PATH);
        assert_eq!(
            ok(&format!("{API_PATH}/r2/buckets")),
            format!("{API_PATH}/r2/buckets")
        );
        assert_eq!(
            ok("local/scheduled?worker=shop-api&cron=*/5+*+*+*+*"),
            format!("{API_PATH}/local/scheduled?worker=shop-api&cron=*/5+*+*+*+*")
        );
        assert_eq!(
            ok("storage/kv/namespaces/a/values/user%3A1"),
            format!("{API_PATH}/storage/kv/namespaces/a/values/user%3A1")
        );
        // `/etc/passwd` is a path under the API too: it can't leave it.
        assert_eq!(ok("/etc/passwd"), format!("{API_PATH}/etc/passwd"));
        for bad in [
            "../../../",
            "local/../../secret",
            "storage/kv/..",
            "./d1",
            "%2e%2e/%2e%2e/",
            "%2E%2E%2Fadmin",
            "%252e%252e/x",
            "/cdn-cgi/local/explorer/../x",
            "..\\..\\x",
            "http://evil.example/x",
            "https://127.0.0.1:8787/cdn-cgi/local/explorer/api",
            "//evil.example/x",
            "d1/database\r\nX-Injected: 1",
            "d1 database",
            "d1#frag",
        ] {
            assert!(api_path(bad).is_err(), "{bad} was let through");
        }
    }

    /// Only GET goes without confirmation; every method that can write needs it.
    #[test]
    fn writes_need_confirmation() {
        assert!(check_confirmed(Method::Get, false).is_ok());
        for method in ["POST", "put", "Patch", "DELETE"] {
            let method = Method::parse(method).unwrap();
            let refused = check_confirmed(method, false).unwrap_err();
            assert!(refused.contains("confirm: true"), "{refused}");
            assert!(check_confirmed(method, true).is_ok());
        }
        assert_eq!(Method::parse("").unwrap(), Method::Get);
        assert!(Method::parse("TRACE").is_err());
    }

    /// A call goes to the API path with its method and body, and the Workers the dev server runs
    /// are read from `GET /local/workers`.
    #[tokio::test]
    async fn calls_reach_the_api_with_their_body() {
        let (addr, _, seen) = serve(explorer_server, true).await;
        let sql = br#"{"sql":"select 1"}"#;
        let r = request(
            addr,
            Method::Post,
            "d1/database/1/raw",
            Some((sql, "application/json")),
        )
        .await
        .unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, br#"{"method":"POST"}"#);
        assert!(!r.truncated);
        let sent = seen.lock().unwrap()[0].clone();
        assert!(sent.starts_with("POST /cdn-cgi/local/explorer/api/d1/database/1/raw HTTP/1.1"));
        assert!(sent.contains("Content-Type: application/json"));
        assert!(sent.ends_with(r#"{"sql":"select 1"}"#));

        let workers = local_workers(addr).await;
        assert_eq!(
            workers,
            [LocalWorker {
                name: "shop-api".into(),
                is_self: true,
                bindings: vec![("kv".into(), "CACHE".into()), ("d1".into(), "DB".into())],
            }]
        );
    }
}
