//! Lane 1 (mesurer-capture): what Sawit does with mesurer's downloads, screenshots, recordings and
//! colours, as pure helpers the app calls: what a file's bytes really are ([`sniff`]) and the name
//! it is saved under ([`fixed_name`]), the captures folder ([`captures_folder`]), a sampled colour
//! as `#rrggbb` ([`srgb_hex`]), the size of a recording's frames ([`frame_size`]), and the H.264
//! levels a frame fits ([`avc_codec_for`], mirrored by `page/mesurer/capture.js`); and mesurer's
//! MP4 export made whole for AVFoundation ([`repair_mp4`]).

mod mp4;

use std::borrow::Cow;
use std::path::{Path, PathBuf};

pub use mp4::repair_mp4;

use super::store::no_links;
use crate::nyawit;

/// What a saved file's bytes are, read from their first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Png,
    Gif,
    Jpeg,
    Webp,
    /// ISO base media (`ftyp`), as MP4.
    Mp4,
    /// ISO base media with QuickTime's brand.
    Mov,
    /// EBML with the `webm` doc type.
    Webm,
    /// EBML of another doc type.
    Mkv,
}

impl Container {
    /// The extension a file of this kind is saved with.
    pub fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Webm => "webm",
            Self::Mkv => "mkv",
        }
    }

    /// Whether `ext` (lower case) names this kind already.
    fn named_by(self, ext: &str) -> bool {
        ext == self.ext()
            || matches!(
                (self, ext),
                (Self::Jpeg, "jpeg") | (Self::Mp4, "m4v" | "m4a")
            )
    }
}

/// What `bytes` are, when they start as a picture or a video Sawit knows does.
pub fn sniff(bytes: &[u8]) -> Option<Container> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(Container::Png);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(Container::Gif);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(Container::Jpeg);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(Container::Webp);
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some(if &bytes[8..12] == b"qt  " {
            Container::Mov
        } else {
            Container::Mp4
        });
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        // The EBML header names its doc type within its first few dozen bytes.
        let head = &bytes[..bytes.len().min(64)];
        let webm = head.windows(4).any(|w| w == b"webm");
        return Some(if webm {
            Container::Webm
        } else {
            Container::Mkv
        });
    }
    None
}

/// The extension for a MIME type mesurer gives its downloads, when there is one.
fn mime_ext(mime: &str) -> Option<&'static str> {
    let base = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    Some(match base.as_str() {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "text/plain" => "txt",
        "application/json" => "json",
        _ => return None,
    })
}

/// The longest name a capture is saved under, in characters.
const NAME_MAX: usize = 120;
/// The name of a capture that came with none.
const NAME_FALLBACK: &str = "mesurer-capture";

/// `name` and its extension, when it has one: 1 to 5 letters and digits after the last dot.
fn split_ext(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && (1..=5).contains(&ext.len())
                && ext.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            (stem, Some(ext))
        }
        _ => (name, None),
    }
}

/// A name the page chose, made safe to save under: its last path part, with no control
/// characters, colons or leading dots, at most [`NAME_MAX`] characters, never empty.
fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last
        .chars()
        .filter(|c| !c.is_control() && *c != ':')
        .collect();
    let clean = clean.trim().trim_start_matches('.').trim();
    if clean.is_empty() {
        return NAME_FALLBACK.into();
    }
    if clean.chars().count() <= NAME_MAX {
        return clean.into();
    }
    // Too long: the start of the stem, and the extension kept.
    let (stem, ext) = split_ext(clean);
    let keep = NAME_MAX - ext.map_or(0, |e| e.len() + 1);
    let stem: String = stem.chars().take(keep).collect();
    match ext {
        Some(ext) => format!("{stem}.{ext}"),
        None => stem,
    }
}

/// The name a download is saved under: `name` made safe, with the extension its bytes really
/// have (WebKit records MP4 where mesurer names a WebM), else the one it came with, else its
/// MIME type's.
pub fn fixed_name(name: &str, mime: &str, bytes: &[u8]) -> String {
    let name = safe_name(name);
    let (stem, ext) = split_ext(&name);
    let lower = ext.map(str::to_ascii_lowercase);
    match (sniff(bytes), lower.as_deref()) {
        (Some(kind), Some(ext)) if kind.named_by(ext) => name,
        (Some(kind), _) => format!("{stem}.{}", kind.ext()),
        (None, Some(_)) => name,
        (None, None) => match mime_ext(mime) {
            Some(ext) => format!("{name}.{ext}"),
            None => name,
        },
    }
}

/// Where mesurer's captures go: `<project>/.nyawit/browser/captures`, or, with no project,
/// `~/Downloads/sawit-captures` (as the Design export's `~/Downloads/sawit-design`).
pub fn captures_dir(project: Option<&Path>, home: &Path) -> PathBuf {
    match project {
        Some(project) => nyawit::dir(project).join("browser").join("captures"),
        None => home.join("Downloads").join("sawit-captures"),
    }
}

/// The folder a capture is reached through: the project, or `~/Downloads`.
fn captures_base(project: Option<&Path>, home: &Path) -> PathBuf {
    project.map_or_else(|| home.join("Downloads"), Path::to_path_buf)
}

/// The captures folder ([`captures_dir`]), made (with `.nyawit/`'s ignore file in a project).
/// Refused when it, or a folder between the project (or `~/Downloads`) and it, is a link: a
/// cloned project's `.nyawit/` can hold one, pointing anywhere.
pub fn captures_folder(project: Option<&Path>, home: &Path) -> Result<PathBuf, String> {
    let folder = captures_dir(project, home);
    let base = captures_base(project, home);
    no_links(&base, &folder)?;
    let made = match project {
        Some(project) => nyawit::ensure(project, &folder),
        None => std::fs::create_dir_all(&folder),
    };
    made.map_err(|e| format!("{} can't be made: {e}", folder.display()))?;
    no_links(&base, &folder)?;
    Ok(folder)
}

/// Saves a download as a new file in the captures folder, under its [`fixed_name`] (or the next
/// free one: `-2`, `-3`, …); never through a link, nor over another file. mesurer's MP4 is saved
/// [repaired](repair_mp4). Its path.
pub fn save_capture(
    project: Option<&Path>,
    home: &Path,
    name: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    let folder = captures_folder(project, home)?;
    let name = fixed_name(name, mime, bytes);
    let bytes = match sniff(bytes) {
        Some(Container::Mp4) => repair_mp4(bytes).map_or(Cow::Borrowed(bytes), Cow::Owned),
        _ => Cow::Borrowed(bytes),
    };
    nyawit::write_unique(&folder, &name, &bytes)
        .map_err(|e| format!("{name} can't be saved in {}: {e}", folder.display()))
}

/// The captures folder as the user is told of it: `.nyawit/browser/captures` in a project,
/// `~/Downloads/sawit-captures` without one.
pub fn shown_folder(project: Option<&Path>, home: &Path, folder: &Path) -> String {
    if let Some(inside) = project.and_then(|p| folder.strip_prefix(p).ok()) {
        return inside.display().to_string();
    }
    match folder.strip_prefix(home) {
        Ok(inside) => format!("~/{}", inside.display()),
        Err(_) => folder.display().to_string(),
    }
}

/// A colour's sRGB components (0 to 1) as `#rrggbb`, what `EyeDropper` answers.
pub fn srgb_hex(red: f64, green: f64, blue: f64) -> String {
    let byte = |v: f64| {
        let v = if v.is_finite() { v.clamp(0., 1.) } else { 0. };
        (v * 255.).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", byte(red), byte(green), byte(blue))
}

/// The widest frame a recording asks for, in pixels.
pub const FRAME_MAX_WIDTH: u32 = 3840;
/// The narrowest.
pub const FRAME_MIN_WIDTH: u32 = 16;

/// A recording's frame: the snapshot's width in the view's points (WebKit's `snapshotWidth`),
/// and the frame's size in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSize {
    pub points: f64,
    pub width: u32,
    pub height: u32,
}

/// The frame of a view `view_width` × `view_height` points on a display of scale `backing`: the
/// view at the display's scale, brought down to `max_width` pixels wide (between
/// [`FRAME_MIN_WIDTH`] and [`FRAME_MAX_WIDTH`]) when that is narrower, its proportions kept.
pub fn frame_size(
    view_width: f64,
    view_height: f64,
    backing: f64,
    max_width: Option<u32>,
) -> FrameSize {
    let positive = |v: f64, or: f64| if v.is_finite() && v > 0. { v } else { or };
    let backing = positive(backing, 1.);
    let (view_width, view_height) = (positive(view_width, 1.), positive(view_height, 1.));
    let natural = (view_width * backing).round().max(1.);
    let cap = max_width
        .unwrap_or(FRAME_MAX_WIDTH)
        .clamp(FRAME_MIN_WIDTH, FRAME_MAX_WIDTH);
    let width = natural.min(f64::from(cap));
    let height = (view_height * width / view_width).round().max(1.);
    FrameSize {
        points: width / backing,
        width: width as u32,
        height: height as u32,
    }
}

/// A PNG's size in pixels, from its header; `None` for bytes that aren't a PNG.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if sniff(bytes) != Some(Container::Png) || bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let read =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    Some((read(16), read(20)))
}

/// H.264's levels (Rec. ITU-T H.264, Table A-1): `level_idc`, the most macroblocks in a frame,
/// and in a second.
pub const AVC_LEVELS: [(u8, u32, u32); 19] = [
    (10, 99, 1_485),
    (11, 396, 3_000),
    (12, 396, 6_000),
    (13, 396, 11_880),
    (20, 396, 11_880),
    (21, 792, 19_800),
    (22, 1_620, 20_250),
    (30, 1_620, 40_500),
    (31, 3_600, 108_000),
    (32, 5_120, 216_000),
    (40, 8_192, 245_760),
    (41, 8_192, 245_760),
    (42, 8_704, 522_240),
    (50, 22_080, 589_824),
    (51, 36_864, 983_040),
    (52, 36_864, 2_073_600),
    (60, 139_264, 4_177_920),
    (61, 139_264, 8_355_840),
    (62, 139_264, 16_711_680),
];

/// Whether a `width` × `height` frame at `fps` (30 when not given) fits AVC level `level`: its
/// macroblocks in a frame and in a second, and each side at most √(8 × the frame's).
pub fn avc_fits(level: u8, width: u32, height: u32, fps: f64) -> bool {
    let Some(&(_, frame, second)) = AVC_LEVELS.iter().find(|l| l.0 == level) else {
        return false;
    };
    let fps = if fps.is_finite() && fps > 0. {
        fps
    } else {
        30.
    };
    let (w, h) = (
        u64::from(width.div_ceil(16)),
        u64::from(height.div_ceil(16)),
    );
    let side = (f64::from(frame) * 8.).sqrt();
    w * h <= u64::from(frame)
        && w as f64 <= side
        && h as f64 <= side
        && (w * h) as f64 * fps <= f64::from(second)
}

/// The codec to ask WebKit's `VideoEncoder` for: `codec` itself when it isn't AVC (`avc1.PPCCLL`)
/// or its level fits the frame; else the same profile at the lowest level that does; `None` when
/// none does. WebKit says yes to a level too small for the frame, then fails to encode it.
pub fn avc_codec_for(codec: &str, width: u32, height: u32, fps: f64) -> Option<String> {
    let Some(hex) = codec.strip_prefix("avc1.") else {
        return Some(codec.into());
    };
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(codec.into());
    }
    let level = u8::from_str_radix(&hex[4..], 16).ok()?;
    if avc_fits(level, width, height, fps) {
        return Some(codec.into());
    }
    AVC_LEVELS
        .iter()
        .find(|l| l.0 > level && avc_fits(l.0, width, height, fps))
        .map(|l| format!("avc1.{}{:02x}", &hex[..4], l.0))
}
