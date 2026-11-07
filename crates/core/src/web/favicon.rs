//! A page's favicon for its browser tab: the icons its `<link>` tags name (`icon`,
//! `apple-touch-icon`), then `/favicon.ico`, fetched in Rust and decoded to pixels (PNG, ICO,
//! JPEG, GIF, WebP, BMP with the `image` crate, SVG with `resvg`), cached per origin. The page's
//! script only reports its `<link>` tags ([`ICON_LINKS_JS`]); without them the page's HTML is
//! fetched and read ([`super::head`]).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::head::{self, Link};
use super::{Request, Url, fetch_with, origin};

/// The side, in pixels, icons are decoded to: a 16 pt tab icon at 2×, with room for 3×.
pub const ICON_PX: u32 = 64;

/// How long an origin with no icon is left alone before it is tried again.
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);

/// [`RETRY_AFTER`] when the page reported its own links.
const RETRY_WITH_LINKS: Duration = Duration::from_secs(30);

/// The most bytes of an icon read.
const MAX_ICON: usize = 2 * 1024 * 1024;
/// The most bytes of a bigger picture (a share image) read.
const MAX_PICTURE: usize = 12 * 1024 * 1024;

/// A script for the page that answers its icon `<link>` tags as JSON, collecting only.
pub const ICON_LINKS_JS: &str = r#"JSON.stringify(Array.from(document.querySelectorAll('link[rel~="icon" i], link[rel~="apple-touch-icon" i], link[rel~="apple-touch-icon-precomposed" i]')).map(l => ({rel: l.getAttribute("rel") || "", href: l.href, sizes: l.getAttribute("sizes"), type: l.getAttribute("type")})))"#;

/// A decoded icon: `width` × `height` RGBA pixels, not premultiplied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Favicon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// The address it came from.
    pub url: String,
}

/// One `<link>` as [`ICON_LINKS_JS`] reports it.
#[derive(Clone, Debug, Deserialize)]
struct ReportedLink {
    rel: String,
    href: String,
    sizes: Option<String>,
    #[serde(rename = "type")]
    mime: Option<String>,
}

/// The links [`ICON_LINKS_JS`] answered, read as [`Link`]s; `None` when it answered something
/// else.
pub fn parse_reported(json: &str) -> Option<Vec<Link>> {
    // WebKit hands back the script's string as JSON, so it may come quoted twice.
    let text = serde_json::from_str::<String>(json).unwrap_or_else(|_| json.to_string());
    let links: Vec<ReportedLink> = serde_json::from_str(&text).ok()?;
    Some(
        links
            .into_iter()
            .map(|l| Link {
                rel: l
                    .rel
                    .split_whitespace()
                    .map(|r| r.to_ascii_lowercase())
                    .collect(),
                href: l.href,
                sizes: l.sizes,
                mime: l.mime.map(|m| m.to_ascii_lowercase()),
                ..Default::default()
            })
            .collect(),
    )
}

/// The largest side `sizes` names (`"16x16 32x32"` → 32; `any` → `u32::MAX`).
pub fn largest_size(sizes: &str) -> Option<u32> {
    sizes
        .split_whitespace()
        .filter_map(|s| {
            if s.eq_ignore_ascii_case("any") {
                return Some(u32::MAX);
            }
            let (w, h) = s
                .to_ascii_lowercase()
                .split_once('x')
                .map(|(w, h)| (w.parse::<u32>().ok(), h.parse::<u32>().ok()))?;
            Some(w?.max(h?))
        })
        .max()
}

/// Whether a link is one of the icon kinds a tab can show.
pub fn is_icon(link: &Link) -> bool {
    link.has_rel("icon")
        || link.has_rel("apple-touch-icon")
        || link.has_rel("apple-touch-icon-precomposed")
}

/// The icons to try for a tab, best first: the one nearest 32 px (SVG scales to any), the
/// touch icons after the plain ones of no size, and `/favicon.ico` last.
pub fn candidates(links: &[Link], page_url: &str) -> Vec<String> {
    let mut ranked: Vec<(u32, usize, &str)> = links
        .iter()
        .enumerate()
        .filter(|(_, l)| is_icon(l) && !l.href.is_empty())
        .map(|(i, l)| {
            let svg = l.mime.as_deref() == Some("image/svg+xml")
                || l.href
                    .split(['?', '#'])
                    .next()
                    .unwrap_or("")
                    .ends_with(".svg")
                || l.href.starts_with("data:image/svg");
            let touch = !l.has_rel("icon");
            let cost = if svg {
                0
            } else {
                match l.sizes.as_deref().and_then(largest_size) {
                    Some(u32::MAX) => 0,
                    Some(s) if s >= 32 => (s - 32).min(500),
                    Some(s) => (32 - s) * 4,
                    None if touch => 148,
                    None => 20,
                }
            };
            (cost, i, l.href.as_str())
        })
        .collect();
    ranked.sort_by_key(|(cost, i, _)| (*cost, *i));
    let mut out: Vec<String> = ranked.into_iter().map(|(_, _, h)| h.to_string()).collect();
    if let Some(origin) = origin(page_url) {
        let ico = format!("{origin}/favicon.ico");
        if !out.contains(&ico) {
            out.push(ico);
        }
    }
    out
}

/// The bytes of a `data:` address, with its media type.
pub fn data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let base64 = meta.ends_with(";base64");
    let mime = meta.split(';').next().unwrap_or("").to_ascii_lowercase();
    let bytes = if base64 {
        crate::browser::base64_decode(data)?
    } else {
        percent_decode(data)
    };
    Some((mime, bytes))
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Whether `bytes` look like SVG.
fn looks_svg(mime: &str, bytes: &[u8]) -> bool {
    if mime.contains("svg") {
        return true;
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    let head = head.trim_start_matches('\u{feff}').trim_start();
    head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg"))
}

/// Decodes an icon to at most `side` × `side` pixels, its aspect kept. `None` for bytes that
/// aren't a picture.
pub fn decode(bytes: &[u8], mime: &str, side: u32) -> Option<(u32, u32, Vec<u8>)> {
    if looks_svg(mime, bytes) {
        return decode_svg(bytes, side);
    }
    let picture = image::load_from_memory(bytes).ok()?;
    let picture = if picture.width() > side || picture.height() > side {
        picture.resize(side, side, image::imageops::FilterType::Lanczos3)
    } else {
        picture
    };
    let rgba = picture.to_rgba8();
    let (w, h) = rgba.dimensions();
    (w > 0 && h > 0).then(|| (w, h, rgba.into_raw()))
}

fn decode_svg(bytes: &[u8], side: u32) -> Option<(u32, u32, Vec<u8>)> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = side as f32 / size.width().max(size.height()).max(1.0);
    let (w, h) = (
        ((size.width() * scale).round() as u32).clamp(1, side),
        ((size.height() * scale).round() as u32).clamp(1, side),
    );
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some((w, h, rgba))
}

/// Fetches and decodes the icon at `url` (a `data:` address too).
pub async fn load_icon(url: &str) -> Option<Favicon> {
    load_picture(url, ICON_PX).await
}

/// Fetches and decodes the picture at `url` to at most `side` pixels a side.
pub async fn load_picture(url: &str, side: u32) -> Option<Favicon> {
    let (mime, bytes) = if url.starts_with("data:") {
        data_url(url)?
    } else {
        let got = fetch_with(
            url,
            Request::default()
                .header(
                    "Accept",
                    "image/avif,image/webp,image/png,image/svg+xml,image/*;q=0.8,*/*;q=0.5",
                )
                .timeout(Duration::from_secs(8))
                .max_body(if side > ICON_PX {
                    MAX_PICTURE
                } else {
                    MAX_ICON
                }),
        )
        .await;
        if !got.ok() || got.body.is_empty() || got.mime().starts_with("text/html") {
            return None;
        }
        (got.mime(), got.body)
    };
    let (width, height, rgba) = tokio::task::spawn_blocking(move || decode(&bytes, &mime, side))
        .await
        .ok()??;
    Some(Favicon {
        width,
        height,
        rgba,
        url: url.to_string(),
    })
}

struct Entry {
    icon: Option<Arc<Favicon>>,
    at: Instant,
}

static CACHE: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Default::default);

/// The icon already found for `page_url`'s origin, if any, without fetching.
pub fn cached(page_url: &str) -> Option<Arc<Favicon>> {
    let key = origin(page_url)?;
    let cache = CACHE.lock().ok()?;
    cache.get(&key).and_then(|e| e.icon.clone())
}

/// The favicon of the page at `page_url`, from the cache or fetched: from `links` (the page's
/// own, as it reported them) when given, else from the page's HTML. `None` for pages with no
/// icon that loads, and for addresses that aren't on the web (`about:`, `file:`).
pub async fn favicon(page_url: &str, links: Option<Vec<Link>>) -> Option<Arc<Favicon>> {
    let parsed = Url::parse(page_url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let key = origin(page_url)?;
    // The page's own links may name an icon the HTML didn't: they're tried again sooner.
    let retry = if links.is_some() {
        RETRY_WITH_LINKS
    } else {
        RETRY_AFTER
    };
    if let Ok(cache) = CACHE.lock()
        && let Some(e) = cache.get(&key)
        && (e.icon.is_some() || e.at.elapsed() < retry)
    {
        return e.icon.clone();
    }
    let links = match links {
        Some(links) => links,
        None => {
            let page = fetch_with(
                page_url,
                Request::default()
                    .header("Accept", "text/html")
                    .timeout(Duration::from_secs(8))
                    .max_body(1024 * 1024),
            )
            .await;
            if page.ok() {
                head::parse(&page.text(), &page.url).links
            } else {
                Vec::new()
            }
        }
    };
    let mut found = None;
    for url in candidates(&links, page_url) {
        if let Some(icon) = load_icon(&url).await {
            found = Some(Arc::new(icon));
            break;
        }
    }
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(
            key,
            Entry {
                icon: found.clone(),
                at: Instant::now(),
            },
        );
    }
    found
}
