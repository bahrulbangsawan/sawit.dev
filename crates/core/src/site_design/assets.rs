//! Asset naming, data-URL decoding and capped downloads for the design export.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::StreamExt;

use super::scan::{AssetKind, AssetOutcome, AssetRef, FontFace};
use crate::{CoreError, cmd};

pub const SAFARI_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
pub const MAX_TOTAL: u64 = 300 * 1024 * 1024;

const BLOB_MSG: &str = "Can't save blob: media (made by the page's scripts).";
/// A video left out because videos weren't asked for: skipped, not failed.
pub const VIDEO_SKIPPED: &str = "Videos weren't included.";
const KNOWN_EXTS: [&str; 18] = [
    "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "ico", "mp4", "webm", "mov", "json",
    "lottie", "woff2", "woff", "ttf", "otf", "eot",
];

pub fn sub_dir(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "images",
        AssetKind::Svg => "svg",
        AssetKind::Icon => "icons",
        AssetKind::Video => "video",
        AssetKind::Lottie => "lottie",
        AssetKind::Font => "fonts",
    }
}

fn kind_name(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "image",
        AssetKind::Svg => "svg",
        AssetKind::Icon => "icon",
        AssetKind::Video => "video",
        AssetKind::Lottie => "lottie",
        AssetKind::Font => "font",
    }
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(60).collect();
    out.trim_matches('-').to_string()
}

fn is_ext(e: &str) -> bool {
    KNOWN_EXTS.contains(&e)
}

pub fn file_name(asset: &AssetRef, taken: &mut HashSet<String>) -> String {
    let url = asset.url.as_deref().filter(|u| !u.starts_with("data:"));
    let segment = url.and_then(|u| {
        let path = u.split(['?', '#']).next().unwrap_or("");
        let path = path.split("://").last().unwrap_or(path);
        let seg = path.rsplit('/').next()?;
        // A bare host has no file segment.
        (path.contains('/') && !seg.is_empty()).then(|| seg.to_string())
    });
    let (seg_stem, seg_ext) = match &segment {
        Some(s) => match s.rsplit_once('.') {
            Some((stem, e)) => (stem.to_string(), Some(e.to_ascii_lowercase())),
            None => (s.clone(), None),
        },
        None => (String::new(), None),
    };
    let mut stem = slug(&asset.label);
    if stem.is_empty() {
        stem = slug(&seg_stem);
    }
    if stem.is_empty() {
        stem = kind_name(asset.kind).to_string();
    }
    let data_ext = asset
        .url
        .as_deref()
        .filter(|u| u.starts_with("data:"))
        .and_then(decode_data_url)
        .and_then(|(m, _)| match m.as_str() {
            "image/png" => Some("png"),
            "image/jpeg" => Some("jpg"),
            "image/gif" => Some("gif"),
            "image/webp" => Some("webp"),
            "image/avif" => Some("avif"),
            "image/svg+xml" => Some("svg"),
            "image/x-icon" | "image/vnd.microsoft.icon" => Some("ico"),
            _ => None,
        });
    let ext = seg_ext
        .filter(|e| is_ext(e))
        .or(data_ext.map(String::from))
        .or(asset.svg.as_ref().map(|_| "svg".to_string()))
        .unwrap_or_else(|| {
            match asset.kind {
                AssetKind::Svg => "svg",
                AssetKind::Lottie => "json",
                AssetKind::Font => "woff2",
                AssetKind::Image | AssetKind::Icon => "png",
                AssetKind::Video => "mp4",
            }
            .to_string()
        });
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        if taken.insert(name.clone()) {
            return name;
        }
        n += 1;
    }
}

fn percent_decode(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

pub fn decode_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mut parts = meta.split(';');
    let mime = parts.next().unwrap_or("").trim();
    let mime = if mime.is_empty() { "text/plain" } else { mime };
    let is_b64 = parts.any(|p| p.eq_ignore_ascii_case("base64"));
    let raw = percent_decode(data.as_bytes());
    let bytes = if is_b64 {
        let text: String = raw
            .iter()
            .filter(|b| !b.is_ascii_whitespace())
            .map(|&b| b as char)
            .collect();
        crate::browser::base64_decode(&text)?
    } else {
        raw
    };
    Some((mime.to_ascii_lowercase(), bytes))
}

fn curl_args_with(
    url: &str,
    out: Option<&Path>,
    referer: &str,
    max_time: &str,
    max_size: &str,
) -> Vec<OsString> {
    let mut a: Vec<OsString> = ["-fsSL", "--compressed", "--max-time", max_time]
        .into_iter()
        .map(OsString::from)
        .collect();
    a.extend(
        [
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--max-filesize",
            max_size,
            "-A",
            SAFARI_UA,
            "-e",
            referer,
        ]
        .map(OsString::from),
    );
    if let Some(out) = out {
        a.push("-o".into());
        a.push(out.as_os_str().to_os_string());
    }
    a.push("--".into());
    a.push(url.into());
    a
}

pub fn curl_args(url: &str, out: &Path, referer: &str) -> Vec<OsString> {
    curl_args_with(url, Some(out), referer, "30", "52428800")
}

/// A fetched text resource, and what curl says about where it came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fetched {
    pub text: String,
    /// The response's `Content-Type`, as sent.
    pub content_type: String,
    /// Where redirects ended.
    pub final_url: String,
    /// The address curl connected to last.
    pub remote_ip: String,
}

/// What curl writes after the body: one value per line (none can hold a newline).
const WRITE_OUT: &str = "\n%{content_type}\n%{url_effective}\n%{remote_ip}";

/// Fetches a small text resource (cross-origin CSS, manifests).
pub async fn fetch_text(url: &str, referer: &str) -> crate::Result<Fetched> {
    let mut args = curl_args_with(url, None, referer, "10", "5242880");
    // Before `--`: the URL stays last.
    let at = args.len() - 2;
    args.splice(at..at, ["-w".into(), OsString::from(WRITE_OUT)]);
    let out = cmd::run("/usr/bin/curl", args, Duration::from_secs(15)).await?;
    if out.code != Some(0) {
        return Err(CoreError::Download(out.stderr.trim().to_string()));
    }
    parse_fetched(&out.stdout)
        .ok_or_else(|| CoreError::Download("curl's answer couldn't be read.".into()))
}

/// The body and [`WRITE_OUT`]'s lines after it.
fn parse_fetched(stdout: &str) -> Option<Fetched> {
    let mut parts = stdout.rsplitn(4, '\n');
    let remote_ip = parts.next()?.trim().to_string();
    let final_url = parts.next()?.trim().to_string();
    let content_type = parts.next()?.trim().to_string();
    let text = parts.next()?.to_string();
    Some(Fetched {
        text,
        content_type,
        final_url,
        remote_ip,
    })
}

/// Whether `content_type` is a style sheet's (`text/css`, any parameters).
pub fn is_css(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("text/css")
}

/// The URL's host: lowercased, decoded, without userinfo, port, brackets or trailing dot.
pub fn url_host(url: &str) -> Option<String> {
    let (_, rest) = url.trim().split_once("://")?;
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    let host = String::from_utf8_lossy(&percent_decode(host.as_bytes())).to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_string())
}

/// An IPv4 address as `inet_aton` (and so curl) reads it: `127.1`, `0x7f000001`, `0177.0.0.1`.
fn loose_v4(host: &str) -> Option<std::net::Ipv4Addr> {
    let num = |p: &str| -> Option<u64> {
        if let Some(hex) = p.strip_prefix("0x") {
            u64::from_str_radix(hex, 16).ok()
        } else if p.len() > 1 && p.starts_with('0') {
            u64::from_str_radix(&p[1..], 8).ok()
        } else {
            p.parse().ok()
        }
    };
    let parts: Vec<u64> = host.split('.').map(num).collect::<Option<_>>()?;
    let (last, head) = parts.split_last()?;
    if head.len() > 3 || head.iter().any(|&n| n > 255) {
        return None;
    }
    let rest = 4 - head.len() as u32;
    if *last >= 1u64 << (8 * rest) {
        return None;
    }
    let v = head.iter().fold(0u64, |v, &n| v << 8 | n) << (8 * rest) | last;
    Some(std::net::Ipv4Addr::from(v as u32))
}

fn local_v4(ip: std::net::Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.octets()[0] == 0
}

/// Whether `host` (a name or an address) is this Mac or its network: loopback, private,
/// link-local and unique-local addresses, `localhost` and `.local` names.
pub fn is_local_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Some(ip) = loose_v4(host) {
        return local_v4(ip);
    }
    // A scoped address (`fe80::1%en0`) is link-local whatever its zone.
    if let Ok(ip) = host
        .split('%')
        .next()
        .unwrap_or("")
        .parse::<std::net::Ipv6Addr>()
    {
        let first = ip.segments()[0];
        return ip.is_loopback()
            || ip.is_unspecified()
            || first & 0xfe00 == 0xfc00
            || first & 0xffc0 == 0xfe80
            || ip.to_ipv4_mapped().is_some_and(local_v4);
    }
    host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local")
}

/// Whether Sawit may fetch `url` for the page at `page_url`: a host on this Mac or its
/// network only when it is the page's own.
pub fn may_fetch_for(url: &str, page_url: &str) -> bool {
    let Some(host) = url_host(url) else {
        return false;
    };
    !is_local_host(&host) || url_host(page_url).is_some_and(|p| p == host)
}

/// The text of a sheet fetched from `href`, when it may go back to the page at `page_url`
/// (Sawit's own URL for the tab, not the page's word): served as CSS, and from a host the
/// page may reach ([`may_fetch_for`]) — where it asked, where redirects ended, and the
/// address connected to. Otherwise why not.
pub fn sheet_for_page(href: &str, fetched: Fetched, page_url: &str) -> Result<String, String> {
    if !is_css(&fetched.content_type) {
        return Err(format!(
            "it was served as “{}”, not text/css",
            fetched.content_type
        ));
    }
    if !may_fetch_for(href, page_url) || !may_fetch_for(&fetched.final_url, page_url) {
        return Err("it is on this Mac or its network".into());
    }
    let own_host = url_host(&fetched.final_url).is_some_and(|h| url_host(page_url) == Some(h));
    if !own_host && is_local_host(&fetched.remote_ip) {
        return Err(format!("its host is at {}", fetched.remote_ip));
    }
    Ok(fetched.text)
}

pub fn font_assets(faces: &[FontFace]) -> Vec<AssetRef> {
    faces
        .iter()
        .flat_map(|f| {
            f.src.iter().map(move |u| AssetRef {
                kind: AssetKind::Font,
                url: Some(u.clone()),
                svg: None,
                label: format!("{}-{}-{}", f.family, f.weight, f.style),
                width: None,
                height: None,
                used_on: vec![],
                logo: false,
            })
        })
        .collect()
}

fn outcome(a: &AssetRef, file: Option<String>, bytes: u64, error: Option<String>) -> AssetOutcome {
    AssetOutcome {
        file,
        url: a.url.clone(),
        kind: a.kind,
        bytes,
        width: a.width,
        height: a.height,
        label: a.label.clone(),
        used_on: a.used_on.clone(),
        error,
    }
}

fn budget_msg(budget: u64) -> String {
    format!(
        "Skipped: the {} MB export limit was reached.",
        budget / (1024 * 1024)
    )
}

fn write_local(
    a: &AssetRef,
    dir: &Path,
    taken: &mut HashSet<String>,
    total: &AtomicU64,
) -> AssetOutcome {
    let bytes = if let Some(svg) = &a.svg {
        svg.as_bytes().to_vec()
    } else {
        match a.url.as_deref().and_then(decode_data_url).map(|(_, b)| b) {
            Some(b) => b,
            None => return outcome(a, None, 0, Some("Couldn't read the inline data.".into())),
        }
    };
    let sub = sub_dir(a.kind);
    let name = file_name(a, taken);
    let target = dir.join(sub);
    let res =
        std::fs::create_dir_all(&target).and_then(|_| std::fs::write(target.join(&name), &bytes));
    match res {
        Ok(()) => {
            total.fetch_add(bytes.len() as u64, Ordering::SeqCst);
            outcome(a, Some(format!("{sub}/{name}")), bytes.len() as u64, None)
        }
        Err(e) => outcome(a, None, 0, Some(e.to_string())),
    }
}

const MAX_FILE: u64 = 52_428_800;

/// Copies a `file://` asset of a `file://` page, without curl.
fn copy_file_url(
    a: &AssetRef,
    url: &str,
    dir: &Path,
    taken: &mut HashSet<String>,
    total: &AtomicU64,
    budget: u64,
) -> AssetOutcome {
    if total.load(Ordering::SeqCst) >= budget {
        return outcome(a, None, 0, Some(budget_msg(budget)));
    }
    let rest = url.strip_prefix("file://").unwrap_or("");
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let src = std::path::PathBuf::from(
        String::from_utf8_lossy(&percent_decode(rest.as_bytes())).into_owned(),
    );
    let len = match std::fs::metadata(&src) {
        Ok(m) if m.is_file() => m.len(),
        Ok(_) => return outcome(a, None, 0, Some("Not a file.".into())),
        Err(e) => return outcome(a, None, 0, Some(e.to_string())),
    };
    if len > MAX_FILE {
        return outcome(a, None, 0, Some("The file is larger than 50 MB.".into()));
    }
    let sub = sub_dir(a.kind);
    let name = file_name(a, taken);
    let target = dir.join(sub);
    let res =
        std::fs::create_dir_all(&target).and_then(|_| std::fs::copy(&src, target.join(&name)));
    match res {
        Ok(n) => {
            total.fetch_add(n, Ordering::SeqCst);
            outcome(a, Some(format!("{sub}/{name}")), n, None)
        }
        Err(e) => outcome(a, None, 0, Some(e.to_string())),
    }
}

async fn fetch_one(
    a: &AssetRef,
    url: &str,
    rel: String,
    dir: &Path,
    referer: &str,
    total: &AtomicU64,
    budget: u64,
) -> AssetOutcome {
    if total.load(Ordering::SeqCst) >= budget {
        return outcome(a, None, 0, Some(budget_msg(budget)));
    }
    let path = dir.join(&rel);
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return outcome(a, None, 0, Some(e.to_string()));
    }
    let res = cmd::run(
        "/usr/bin/curl",
        curl_args(url, &path, referer),
        Duration::from_secs(40),
    )
    .await;
    let err = match res {
        Ok(o) if o.code == Some(0) => None,
        Ok(o) => Some(o.stderr.trim().to_string()),
        Err(e) => Some(e.to_string()),
    };
    if let Some(msg) = err {
        let _ = std::fs::remove_file(&path);
        let msg = if msg.is_empty() {
            "Download failed.".into()
        } else {
            msg
        };
        return outcome(a, None, 0, Some(msg));
    }
    let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    total.fetch_add(len, Ordering::SeqCst);
    outcome(a, Some(rel), len, None)
}

/// Whether `o` is a video left out because videos weren't asked for.
pub fn is_skipped_video(o: &AssetOutcome) -> bool {
    o.kind == AssetKind::Video && o.error.as_deref() == Some(VIDEO_SKIPPED)
}

/// Downloads `items` into `assets_dir`. `file_page` says whether the tab shows a
/// `file://` page by its own URL (not the page's word): only then are `file://` assets
/// copied.
pub async fn download_all(
    items: Vec<AssetRef>,
    assets_dir: &Path,
    referer: &str,
    file_page: bool,
    include_videos: bool,
    budget: u64,
    progress: impl Fn(usize, usize) + Send + Sync,
) -> Vec<AssetOutcome> {
    let n = items.len();
    let mut results: Vec<Option<AssetOutcome>> = vec![None; n];
    let mut taken: HashSet<String> = HashSet::new();
    let total = AtomicU64::new(0);
    let mut done = 0;
    let mut remote: Vec<(usize, String, String)> = Vec::new();

    for (i, a) in items.iter().enumerate() {
        let url = a.url.as_deref();
        let local = a.svg.is_some() || url.is_some_and(|u| u.starts_with("data:"));
        let res = if url.is_some_and(|u| u.starts_with("blob:")) {
            Some(outcome(a, None, 0, Some(BLOB_MSG.into())))
        } else if a.kind == AssetKind::Video && !include_videos {
            Some(outcome(a, None, 0, Some(VIDEO_SKIPPED.into())))
        } else if local {
            if total.load(Ordering::SeqCst) >= budget {
                Some(outcome(a, None, 0, Some(budget_msg(budget))))
            } else {
                Some(write_local(a, assets_dir, &mut taken, &total))
            }
        } else if let Some(u) = url.filter(|u| u.starts_with("file://")) {
            if file_page {
                Some(copy_file_url(a, u, assets_dir, &mut taken, &total, budget))
            } else {
                Some(outcome(
                    a,
                    None,
                    0,
                    Some("Local files can only be saved from a local page.".into()),
                ))
            }
        } else if let Some(u) = url.filter(|u| {
            let l = u.to_ascii_lowercase();
            !(l.starts_with("http://") || l.starts_with("https://"))
        }) {
            let scheme = u.split(':').next().unwrap_or("");
            Some(outcome(
                a,
                None,
                0,
                Some(format!("Can't download {scheme}: URLs.")),
            ))
        } else if let Some(u) = url {
            let rel = format!("{}/{}", sub_dir(a.kind), file_name(a, &mut taken));
            remote.push((i, u.to_string(), rel));
            None
        } else {
            Some(outcome(a, None, 0, Some("No source to download.".into())))
        };
        if let Some(r) = res {
            results[i] = Some(r);
            done += 1;
            progress(done, n);
        }
    }

    let items_ref = &items;
    let total_ref = &total;
    let mut stream = futures::stream::iter(remote.into_iter().map(|(i, url, rel)| async move {
        let o = fetch_one(
            &items_ref[i],
            &url,
            rel,
            assets_dir,
            referer,
            total_ref,
            budget,
        )
        .await;
        (i, o)
    }))
    .buffer_unordered(6);
    while let Some((i, o)) = stream.next().await {
        results[i] = Some(o);
        done += 1;
        progress(done, n);
    }
    results.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn file_names() {
        let mut t = HashSet::new();
        let hero = asset(
            AssetKind::Image,
            Some("https://x.com/a/hero_large.jpg?x=1"),
            None,
            "Hero photo",
        );
        assert_eq!(file_name(&hero, &mut t), "hero-photo.jpg");
        assert_eq!(file_name(&hero, &mut t), "hero-photo-2.jpg");
        let cart = asset(
            AssetKind::Icon,
            Some("https://x.com/icons/cart.svg"),
            None,
            "",
        );
        assert_eq!(file_name(&cart, &mut t), "cart.svg");
        let logo = asset(AssetKind::Svg, None, Some("<svg/>"), "logo");
        assert_eq!(file_name(&logo, &mut t), "logo.svg");
        let eot = asset(
            AssetKind::Font,
            Some("https://x.com/f/icons.eot"),
            None,
            "Icons",
        );
        assert_eq!(file_name(&eot, &mut t), "icons.eot");
        let small = asset(AssetKind::Icon, None, Some("<svg/>"), "logo");
        assert_eq!(file_name(&small, &mut t), "logo-2.svg");
        let data = asset(
            AssetKind::Image,
            Some("data:image/png;base64,AAAA"),
            None,
            "",
        );
        assert_eq!(file_name(&data, &mut t), "image.png");
    }

    #[test]
    fn data_urls() {
        assert_eq!(
            decode_data_url("data:image/svg+xml;base64,PHN2Zy8+"),
            Some(("image/svg+xml".into(), b"<svg/>".to_vec()))
        );
        assert_eq!(
            decode_data_url("data:text/plain,hello%20world").unwrap().1,
            b"hello world"
        );
    }

    #[test]
    fn curl_args_carry_the_referer() {
        let a = curl_args(
            "https://x.com/a.png",
            Path::new("/tmp/a.png"),
            "https://x.com/",
        );
        let s: Vec<String> = a.iter().map(|x| x.to_string_lossy().into_owned()).collect();
        let at = |f: &str| s.iter().position(|x| x == f).unwrap();
        assert_eq!(s[at("-e") + 1], "https://x.com/");
        assert_eq!(s[at("--max-filesize") + 1], "52428800");
        assert_eq!(s.last().unwrap(), "https://x.com/a.png");
        assert_eq!(s[s.len() - 2], "--");
        assert_eq!(s[at("--proto") + 1], "=http,https");
        assert_eq!(s[at("--proto-redir") + 1], "=http,https");
    }

    #[tokio::test]
    async fn file_urls_only_from_file_pages() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.png"), [1u8; 10]).unwrap();
        let url = format!("file://{}/a.png", src.path().display());
        let mk = || vec![asset(AssetKind::Image, Some(&url), None, "a")];

        let dir = tempfile::tempdir().unwrap();
        let out = download_all(
            mk(),
            dir.path(),
            "https://x.com/",
            false,
            false,
            1 << 20,
            |_, _| {},
        )
        .await;
        assert!(out[0].error.is_some() && out[0].file.is_none());
        assert!(!dir.path().join("images").exists());
        let hosts = vec![asset(
            AssetKind::Image,
            Some("file:///etc/hosts"),
            None,
            "h",
        )];
        let out = download_all(
            hosts,
            dir.path(),
            "https://x.com/",
            false,
            false,
            1 << 20,
            |_, _| {},
        )
        .await;
        assert!(out[0].error.is_some());
        assert!(!dir.path().join("images/h.png").exists());
        // A page saying it is a file doesn't make it one.
        let out = download_all(
            mk(),
            dir.path(),
            "file:///tmp/index.html",
            false,
            false,
            1 << 20,
            |_, _| {},
        )
        .await;
        assert!(out[0].error.is_some());
        assert!(!dir.path().join("images").exists());

        let out = download_all(
            mk(),
            dir.path(),
            "file:///tmp/index.html",
            true,
            false,
            1 << 20,
            |_, _| {},
        )
        .await;
        assert_eq!(out[0].file.as_deref(), Some("images/a.png"));
        assert_eq!(out[0].bytes, 10);
        assert!(dir.path().join("images/a.png").exists());

        let js = vec![asset(
            AssetKind::Image,
            Some("javascript:alert(1)"),
            None,
            "j",
        )];
        let out = download_all(
            js,
            dir.path(),
            "https://x.com/",
            false,
            false,
            1 << 20,
            |_, _| {},
        )
        .await;
        assert!(out[0].error.is_some());
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

    #[tokio::test]
    async fn failed_downloads_are_recorded() {
        let png = format!("data:image/png;base64,{}", enc(&[7u8; 600]));
        let items = vec![
            asset(AssetKind::Svg, None, Some("<svg/>"), "mark"),
            asset(AssetKind::Image, Some(&png), None, "one"),
            asset(AssetKind::Image, Some(&png), None, "two"),
            asset(AssetKind::Video, Some("https://x.com/v.mp4"), None, "clip"),
            asset(AssetKind::Image, Some("blob:https://x.com/abc"), None, "b"),
        ];
        let dir = tempfile::tempdir().unwrap();
        let out = download_all(
            items,
            dir.path(),
            "https://x.com/",
            false,
            false,
            500,
            |_, _| {},
        )
        .await;
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].file.as_deref(), Some("svg/mark.svg"));
        assert_eq!(out[0].bytes, 6);
        assert_eq!(out[1].file.as_deref(), Some("images/one.png"));
        assert_eq!(out[1].bytes, 600);
        assert!(dir.path().join("images/one.png").exists());
        assert!(out[2].error.as_deref().unwrap().starts_with("Skipped: the"));
        assert!(out[2].file.is_none());
        assert_eq!(out[3].error.as_deref(), Some(VIDEO_SKIPPED));
        assert!(is_skipped_video(&out[3]));
        assert!(!out.iter().take(3).any(is_skipped_video));
        assert_eq!(out[4].error.as_deref(), Some(BLOB_MSG));
    }

    fn css(text: &str, final_url: &str, remote_ip: &str) -> Fetched {
        Fetched {
            text: text.into(),
            content_type: "text/css; charset=utf-8".into(),
            final_url: final_url.into(),
            remote_ip: remote_ip.into(),
        }
    }

    #[test]
    fn curl_reports_follow_the_body() {
        let f = parse_fetched("a{}\nb{}\n\ntext/css\nhttps://c.dev/x.css\n1.2.3.4").unwrap();
        assert_eq!(f.text, "a{}\nb{}\n");
        assert_eq!(f.content_type, "text/css");
        assert_eq!(f.final_url, "https://c.dev/x.css");
        assert_eq!(f.remote_ip, "1.2.3.4");
        assert_eq!(parse_fetched("no report"), None);
    }

    #[test]
    fn only_css_goes_back_to_the_page() {
        let page = "https://a.dev/";
        let href = "https://cdn.b.dev/site.css";
        assert_eq!(
            sheet_for_page(href, css("a{}", href, "93.184.216.34"), page),
            Ok("a{}".into())
        );
        let mut js = css("export default 1", href, "93.184.216.34");
        js.content_type = "application/javascript".into();
        assert!(sheet_for_page(href, js, page).is_err());
        let mut none = css("a{}", href, "93.184.216.34");
        none.content_type.clear();
        assert!(sheet_for_page(href, none, page).is_err());
    }

    #[test]
    fn local_sheets_only_for_their_own_host() {
        let dev = "http://localhost:5173/src/App.css";
        // A public page can't read the dev server (or the LAN) through Sawit.
        assert!(sheet_for_page(dev, css("a{}", dev, "127.0.0.1"), "https://evil.dev/").is_err());
        let lan = "http://192.168.1.5/admin.css";
        assert!(sheet_for_page(lan, css("a{}", lan, "192.168.1.5"), "https://evil.dev/").is_err());
        // Nor by a redirect there, or a public name for a local address.
        let public = "https://cdn.b.dev/x.css";
        assert!(sheet_for_page(public, css("a{}", dev, "127.0.0.1"), "https://a.dev/").is_err());
        assert!(sheet_for_page(public, css("a{}", public, "10.0.0.2"), "https://a.dev/").is_err());
        // The dev server's own page keeps its sheets.
        assert_eq!(
            sheet_for_page(dev, css("a{}", dev, "::1"), "http://localhost:5173/"),
            Ok("a{}".into())
        );
        assert!(may_fetch_for(dev, "http://localhost:3000/"));
        assert!(!may_fetch_for(dev, "http://127.0.0.1:5173/"));
        assert!(!may_fetch_for("not a url", "https://a.dev/"));
    }

    #[test]
    fn local_hosts() {
        for h in [
            "127.0.0.1",
            "127.1",
            "0x7f000001",
            "2130706433",
            "0177.0.0.1",
            "0.0.0.0",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.0.10",
            "169.254.169.254",
            "::1",
            "[::1]",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1%en0",
            "::ffff:127.0.0.1",
            "localhost",
            "LocalHost.",
            "app.localhost",
            "printer.local",
        ] {
            assert!(is_local_host(h), "{h}");
        }
        for h in [
            "93.184.216.34",
            "172.32.0.1",
            "8.8.8.8",
            "2606:4700::1",
            "example.com",
            "localhost.example.com",
            "deadbeef",
            "",
        ] {
            assert!(!is_local_host(h), "{h}");
        }
        assert_eq!(url_host("http://user@[::1]:8080/a").as_deref(), Some("::1"));
        assert_eq!(
            url_host("http://%6cocalhost./x").as_deref(),
            Some("localhost")
        );
        assert_eq!(url_host("https://A.dev:443?q").as_deref(), Some("a.dev"));
    }

    #[test]
    fn fonts_become_assets() {
        let f = FontFace {
            family: "SF Pro Text".into(),
            weight: "400".into(),
            style: "normal".into(),
            src: vec![
                "https://x.com/a.woff2".into(),
                "https://x.com/a.woff".into(),
            ],
        };
        let a = font_assets(&[f]);
        assert_eq!(a.len(), 2);
        assert!(
            a.iter()
                .all(|x| x.kind == AssetKind::Font && x.label == "SF Pro Text-400-normal")
        );
    }
}
