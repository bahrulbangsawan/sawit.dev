//! A dev server's pages found by following its links: from its first pages, each page fetched
//! (over loopback only) and the links on it to pages of the same server followed, breadth first,
//! within [`Limits`]. It finds the routes of an app whose framework's files Sawit doesn't read,
//! and samples for the dynamic routes of one it does (`/blog/hello` for `/blog/[slug]`).

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use super::pattern::not_a_page;

/// How far a crawl goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Most pages fetched.
    pub pages: usize,
    /// Most pages found, fetched or only linked to.
    pub links: usize,
    /// Longest a page is waited for.
    pub request: Duration,
    /// Longest the whole crawl takes.
    pub total: Duration,
}

/// The crawl Add Routes runs: 30 pages fetched, 200 found, in 15 s at most.
pub const CRAWL: Limits = Limits {
    pages: 30,
    links: 200,
    request: Duration::from_secs(4),
    total: Duration::from_secs(15),
};

/// What a crawl found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Crawled {
    /// The pages fetched that answered with HTML, in the order they were.
    pub pages: Vec<String>,
    /// Every page of the server linked to, fetched or not, in the order found.
    pub links: Vec<String>,
    /// How many pages were fetched.
    pub fetched: usize,
    /// It stopped at a limit with pages still to fetch.
    pub capped: bool,
}

/// Crawls from the pages `start`, fetching each with `fetch` (a page's HTML, `None` for one that
/// isn't an HTML page), until there is nothing left or a limit is reached. The links of a page are
/// read as being on the server at `origin` (`localhost:3000`).
pub async fn crawl_with<F, Fut>(
    start: &[String],
    origin: &str,
    limits: &Limits,
    mut fetch: F,
) -> Crawled
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Option<String>>,
{
    let started = Instant::now();
    let mut crawled = Crawled::default();
    let mut seen: HashSet<String> = start.iter().cloned().collect();
    let mut queue: VecDeque<String> = start.iter().cloned().collect();
    while let Some(path) = queue.pop_front() {
        if crawled.fetched >= limits.pages || started.elapsed() >= limits.total {
            queue.push_front(path);
            break;
        }
        crawled.fetched += 1;
        let Some(html) = fetch(path.clone()).await else {
            continue;
        };
        crawled.pages.push(path.clone());
        for link in links_in(&html, &path, origin) {
            if seen.len() >= limits.links {
                crawled.capped = true;
                break;
            }
            if seen.insert(link.clone()) {
                crawled.links.push(link.clone());
                queue.push_back(link);
            }
        }
    }
    crawled.capped |= !queue.is_empty();
    crawled
}

/// The pages of the server at `origin` that `html`, the page at `page`, links to: each `<a
/// href>`'s path, without its query and fragment, and none twice. Links elsewhere, to files
/// (`/logo.png`), to the server's own (`/_next/…`, `/@vite/…`) and to its API are left out.
pub fn links_in(html: &str, page: &str, origin: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<a") {
        let start = from + at + 2;
        from = start;
        if !lower[start..].starts_with(|c: char| c.is_ascii_whitespace()) {
            continue;
        }
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        if let Some(href) = attribute(&html[start..end], &lower[start..end], "href")
            && let Some(path) = same_origin_path(&href, page, origin)
            && !found.contains(&path)
        {
            found.push(path);
        }
        from = end;
    }
    found
}

/// The value of attribute `name` in a tag's text (`tag`, and `lower`, the same in lower case).
fn attribute(tag: &str, lower: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = lower[from..].find(name) {
        let at = from + at;
        from = at + name.len();
        let before = lower[..at].chars().next_back();
        if !before.is_some_and(|c| c.is_ascii_whitespace()) {
            continue;
        }
        let rest = lower[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let value_at = tag.len() - rest.trim_start().len();
        let value = &tag[value_at..];
        let text = match value.chars().next() {
            Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or_default(),
            _ => value
                .split(|c: char| c.is_ascii_whitespace())
                .next()
                .unwrap_or_default(),
        };
        return Some(text.replace("&amp;", "&"));
    }
    None
}

/// `href`, a link on the page at `page`, as the path of a page of the server at `origin`: `None`
/// for a link elsewhere, to a file, or to the server's own or API paths.
pub fn same_origin_path(href: &str, page: &str, origin: &str) -> Option<String> {
    let href = href.trim();
    let href = href.split(['#', '?']).next().unwrap_or_default();
    let lower = href.to_ascii_lowercase();
    let schemes = ["mailto:", "tel:", "javascript:", "data:", "blob:", "ftp:"];
    if href.is_empty() || schemes.iter().any(|s| lower.starts_with(s)) {
        return None;
    }
    let path = if let Some(rest) = (lower.strip_prefix("http://"))
        .or_else(|| lower.strip_prefix("https://"))
        .or_else(|| lower.strip_prefix("//"))
    {
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        if !same_server(host, origin) {
            return None;
        }
        // The path as written, in its own case.
        let at = href.len() - path.len();
        format!("/{}", &href[at..])
    } else if href.starts_with('/') {
        href.to_string()
    } else {
        let dir = page.rsplit_once('/').map_or("", |(dir, _)| dir);
        format!("{dir}/{href}")
    };
    let path = normalized(&path)?;
    let own = path.starts_with("/_") || path.starts_with("/@") || path.len() > 200;
    (!own && !not_a_page(&path)).then_some(path)
}

/// Whether `host` (`localhost:3000`) is the server at `origin`: the same port on a name of this
/// Mac.
fn same_server(host: &str, origin: &str) -> bool {
    let port = |h: &str| h.rsplit_once(':').map(|(_, p)| p.to_string());
    let local = |h: &str| {
        let name = h.rsplit_once(':').map_or(h, |(n, _)| n);
        matches!(name, "localhost" | "127.0.0.1" | "[::1]")
    };
    host.eq_ignore_ascii_case(origin)
        || (local(host) && local(origin) && port(host) == port(origin))
}

/// `path` with `.` and `..` worked out, no empty segments and no trailing slash (but `/`).
fn normalized(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(format!("/{}", parts.join("/")))
}

/// The host (as the URL writes it) and port of an `http://` or `https://` URL.
pub fn host_port(url: &str) -> Option<(String, u16)> {
    let (rest, default) = match url.strip_prefix("http://") {
        Some(rest) => (rest, 80),
        None => (url.strip_prefix("https://")?, 443),
    };
    let host = rest.split(['/', '?', '#']).next()?;
    let (name, port) = match host.rsplit_once(':') {
        Some((name, port)) if !name.is_empty() && !port.contains(']') => (name, port.parse().ok()?),
        _ => (host, default),
    };
    Some((name.to_string(), port))
}

/// The loopback addresses of `url`'s server: 127.0.0.1 before ::1 for `localhost`.
pub fn loopback_addrs(url: &str) -> Vec<SocketAddr> {
    let Some((host, port)) = host_port(url) else {
        return Vec::new();
    };
    let name = host.trim_start_matches('[').trim_end_matches(']');
    let mut addrs: Vec<SocketAddr> = (name, port)
        .to_socket_addrs()
        .map(|found| found.filter(|a| a.ip().is_loopback()).collect())
        .unwrap_or_default();
    addrs.sort_by_key(SocketAddr::is_ipv6);
    addrs.dedup();
    addrs
}

/// Whether something answers on `url`'s server: a connection to one of its loopback addresses
/// within `wait`. Blocks, so it runs off the UI thread.
pub fn answers(url: &str, wait: Duration) -> bool {
    (loopback_addrs(url).iter()).any(|addr| TcpStream::connect_timeout(addr, wait).is_ok())
}

/// Crawls the dev server at `base` (`http://localhost:3000`) from `start` within `limits`, over
/// loopback only. A server that isn't on this Mac, or speaks HTTPS, is crawled no further than
/// nothing: the answer says so.
pub async fn crawl(base: &str, start: &[String], limits: &Limits) -> Result<Crawled, String> {
    let Some((host, port)) = host_port(base).filter(|_| base.starts_with("http://")) else {
        return Err(format!("{base} isn't an http:// address Sawit can crawl"));
    };
    let addrs = loopback_addrs(base);
    let first = (addrs.iter())
        .find(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(500)).is_ok())
        .copied()
        .ok_or_else(|| format!("nothing answers on {host}:{port}"))?;
    let origin = format!("{host}:{port}");
    let request = limits.request;
    let crawled = crawl_with(start, &origin, limits, |path| async move {
        let page = crate::cloudflare::explorer::loopback_get(first, &path, request).await;
        page.ok()
            .filter(|p| (200..300).contains(&p.status) && is_html(p))
            .map(|p| String::from_utf8_lossy(&p.body).into_owned())
    })
    .await;
    Ok(crawled)
}

fn is_html(page: &crate::cloudflare::explorer::Response) -> bool {
    match &page.content_type {
        Some(kind) => kind.to_ascii_lowercase().contains("html"),
        None => page.body.trim_ascii_start().starts_with(b"<"),
    }
}
