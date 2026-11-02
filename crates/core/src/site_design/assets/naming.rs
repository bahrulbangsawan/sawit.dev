//! Where an asset's file goes: a folder by kind, and a name no other file there has.

use std::collections::HashSet;
use std::path::Path;

use super::data_url::decode_data_url;
use crate::site_design::scan::{AssetKind, AssetRef};

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
    let data_mime = asset
        .url
        .as_deref()
        .filter(|u| u.starts_with("data:"))
        .and_then(decode_data_url)
        .map(|(mime, _)| mime);
    name_for(asset, data_mime.as_deref(), taken)
}

/// [`file_name`], with the MIME type of the asset's `data:` URL as its caller decoded it.
pub(super) fn name_for(
    asset: &AssetRef,
    data_mime: Option<&str>,
    taken: &mut HashSet<String>,
) -> String {
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
    let data_ext = data_mime.and_then(|m| match m {
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

/// The names of the files already in `assets_dir`'s folders.
pub(super) fn names_in(assets_dir: &Path) -> HashSet<String> {
    let kinds = [
        AssetKind::Image,
        AssetKind::Svg,
        AssetKind::Icon,
        AssetKind::Video,
        AssetKind::Lottie,
        AssetKind::Font,
    ];
    kinds
        .into_iter()
        .filter_map(|k| std::fs::read_dir(assets_dir.join(sub_dir(k))).ok())
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}
