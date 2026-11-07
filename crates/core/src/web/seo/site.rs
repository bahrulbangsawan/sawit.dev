//! The site a scan reads: the page, its HTML read once, and every other address fetched at most
//! once however many checks ask for it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::web::head::{self, Head};
use crate::web::{Fetched, Request, Url, fetch_with};

/// How long one of the site's files is waited for.
const FILE_TIMEOUT: Duration = Duration::from_secs(10);
/// The most of one of the site's files read.
const FILE_MAX: usize = 4 * 1024 * 1024;

type Slot = Arc<OnceCell<Arc<Fetched>>>;

pub struct Site {
    /// The address scanned, as given.
    pub url: String,
    /// The page as the server sent it.
    pub page: Arc<Fetched>,
    /// The server's HTML, read.
    pub server: Head,
    /// The page as the browser rendered it, when it was handed over; else the server's.
    pub head: Head,
    pub rendered: bool,
    pub origin: String,
    pub host: String,
    pub local: bool,
    cache: Mutex<HashMap<String, Slot>>,
}

impl Site {
    /// Fetches the page at `url` and reads it.
    pub async fn load(url: &str, rendered_html: Option<String>) -> Site {
        let page = Arc::new(
            fetch_with(
                url,
                Request::default()
                    .header("Accept", "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8")
                    .header("Accept-Language", "en"),
            )
            .await,
        );
        let base = if page.url.is_empty() {
            url.to_string()
        } else {
            page.url.clone()
        };
        let server = head::parse(&page.text(), &base);
        let (head, rendered) = match rendered_html {
            Some(html) if !html.trim().is_empty() => (head::parse(&html, &base), true),
            _ => (server.clone(), false),
        };
        let parsed = Url::parse(&base).ok();
        let origin = crate::web::origin(&base).unwrap_or_default();
        let host = parsed
            .as_ref()
            .and_then(|u| u.host_str().map(String::from))
            .unwrap_or_default();
        let site = Site {
            url: url.to_string(),
            local: crate::web::is_local(&base),
            page: page.clone(),
            server,
            head,
            rendered,
            origin,
            host,
            cache: Mutex::new(HashMap::new()),
        };
        site.remember("GET", &base, &[], page.clone());
        site
    }

    /// The address of `path` on the site (`/robots.txt`), or `path` when it is a whole address.
    pub fn at(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        format!("{}{}", self.origin, path)
    }

    fn key(method: &str, url: &str, headers: &[(&str, &str)]) -> String {
        let mut key = format!("{method} {url}");
        for (n, v) in headers {
            key.push_str(&format!("\n{}: {v}", n.to_ascii_lowercase()));
        }
        key
    }

    fn slot(&self, key: String) -> Slot {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.entry(key).or_default().clone()
    }

    fn remember(&self, method: &str, url: &str, headers: &[(&str, &str)], got: Arc<Fetched>) {
        let _ = self.slot(Self::key(method, url, headers)).set(got);
    }

    /// GETs `path` (on the site, or a whole address) once.
    pub async fn get(&self, path: &str) -> Arc<Fetched> {
        self.get_with(path, &[]).await
    }

    /// GETs `path` with `headers`, once for each set of headers.
    pub async fn get_with(&self, path: &str, headers: &[(&str, &str)]) -> Arc<Fetched> {
        self.request("GET", path, headers, None).await
    }

    /// A request of any method, once per method, address, headers and body.
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Arc<Fetched> {
        let url = self.at(path);
        let mut key = Self::key(method, &url, headers);
        if let Some(body) = body {
            key.push_str("\n\n");
            key.push_str(body);
        }
        let slot = self.slot(key);
        slot.get_or_init(|| async {
            let mut req = Request::default()
                .timeout(FILE_TIMEOUT)
                .max_body(FILE_MAX)
                .method(reqwest::Method::from_bytes(method.as_bytes()).unwrap_or_default());
            for (n, v) in headers {
                req = req.header(n, v);
            }
            if let Some(body) = body {
                req = req.body(body.as_bytes().to_vec());
            }
            Arc::new(fetch_with(&url, req).await)
        })
        .await
        .clone()
    }

    /// GETs `path` and reads it as JSON when it answered 2xx with JSON.
    pub async fn json(&self, path: &str) -> (Arc<Fetched>, Option<serde_json::Value>) {
        let got = self.get_with(path, &[("Accept", "application/json")]).await;
        let value = got
            .ok()
            .then(|| serde_json::from_slice(&got.body).ok())
            .flatten();
        (got, value)
    }

    /// Whether `got` answered with a real file, not the site's HTML page for every address (a
    /// single-page app answers 200 with its shell for anything).
    pub fn real_file(&self, got: &Fetched) -> bool {
        got.ok()
            && !got.body.is_empty()
            && !(got.mime().contains("html") && got.text().trim_start().starts_with('<'))
    }
}

/// An image's pixel size and type, from the first bytes of it.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32, &'static str)> {
    if let Some((w, h)) = avif_size(bytes) {
        return Some((w, h, "image/avif"));
    }
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let format = reader.format()?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Gif => "image/gif",
        image::ImageFormat::WebP => "image/webp",
        image::ImageFormat::Bmp => "image/bmp",
        image::ImageFormat::Ico => "image/x-icon",
        _ => "image",
    };
    let (w, h) = reader.into_dimensions().ok()?;
    Some((w, h, mime))
}

/// An AVIF's size, from its `ispe` box (the image crate reads AVIF only with a C decoder).
pub fn avif_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 12
        || &bytes[4..8] != b"ftyp"
        || !bytes[8..bytes.len().min(64)]
            .windows(4)
            .any(|w| w == b"avif" || w == b"avis")
    {
        return None;
    }
    let at = bytes.windows(4).position(|w| w == b"ispe")?;
    let read = |i: usize| {
        bytes
            .get(i..i + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    // "ispe", then version and flags (4 bytes), width, height.
    Some((read(at + 8)?, read(at + 12)?))
}
