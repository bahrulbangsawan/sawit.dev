//! Finding running Local Explorers: each Workers dev server is asked for the API's OpenAPI
//! document on loopback, and the answers are kept per port and process.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::http::{http, refuse_remote};
use super::{API_PATH, Explorer, loopback};
use crate::ports::PortJob;

/// How long a dev server gets to answer the probe. It is on this Mac, so a short wait.
const PROBE_TIMEOUT: Duration = Duration::from_millis(800);

/// The OpenAPI document is about 60 KB; an answer much bigger than this isn't one.
const PROBE_LIMIT: usize = 4 << 20;

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
            crate::cloudflare::CONFIG_FILES
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
