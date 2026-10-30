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
const KNOWN_EXTS: [&str; 16] = [
    "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "ico", "mp4", "webm", "mov", "json",
    "lottie", "woff2", "woff", "ttf",
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
    KNOWN_EXTS.contains(&e) || e == "otf"
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

fn b64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

fn base64_decode(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for &c in s {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | b64_val(c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
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
    let bytes = if is_b64 { base64_decode(&raw)? } else { raw };
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
    a.extend(["--max-filesize", max_size, "-A", SAFARI_UA, "-e", referer].map(OsString::from));
    if let Some(out) = out {
        a.push("-o".into());
        a.push(out.as_os_str().to_os_string());
    }
    a.push(url.into());
    a
}

pub fn curl_args(url: &str, out: &Path, referer: &str) -> Vec<OsString> {
    curl_args_with(url, Some(out), referer, "30", "52428800")
}

/// Fetches a small text resource (cross-origin CSS, manifests).
pub async fn fetch_text(url: &str, referer: &str) -> crate::Result<String> {
    let args = curl_args_with(url, None, referer, "10", "5242880");
    let out = cmd::run("/usr/bin/curl", args, Duration::from_secs(15)).await?;
    if out.code != Some(0) {
        return Err(CoreError::Download(out.stderr.trim().to_string()));
    }
    Ok(out.stdout)
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

pub async fn download_all(
    items: Vec<AssetRef>,
    assets_dir: &Path,
    referer: &str,
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
            Some(outcome(a, None, 0, Some("Videos weren't included.".into())))
        } else if local {
            if total.load(Ordering::SeqCst) >= budget {
                Some(outcome(a, None, 0, Some(budget_msg(budget))))
            } else {
                Some(write_local(a, assets_dir, &mut taken, &total))
            }
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
        let out = download_all(items, dir.path(), "https://x.com/", false, 500, |_, _| {}).await;
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].file.as_deref(), Some("svg/mark.svg"));
        assert_eq!(out[0].bytes, 6);
        assert_eq!(out[1].file.as_deref(), Some("images/one.png"));
        assert_eq!(out[1].bytes, 600);
        assert!(dir.path().join("images/one.png").exists());
        assert!(out[2].error.as_deref().unwrap().starts_with("Skipped: the"));
        assert!(out[2].file.is_none());
        assert_eq!(out[3].error.as_deref(), Some("Videos weren't included."));
        assert_eq!(out[4].error.as_deref(), Some(BLOB_MSG));
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
