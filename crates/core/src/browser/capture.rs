//! Native screenshots and recordings of the browser, without GPUI: what a capture is asked for
//! (device or size, scale, output resolution, format, full page), the pixel sizes that follow
//! from it, how a page too tall for one snapshot is cut into tiles, what the file is called, and
//! the limits the encoders set. The app takes the pictures (WebKit) and encodes them (Apple's
//! frameworks); this module decides the numbers.

use serde::{Deserialize, Serialize};

use super::viewport::device_by_name;

/// The largest side WebKit and Core Graphics are asked to draw in one snapshot; a taller page is
/// taken in tiles of at most this height.
pub const MAX_SNAPSHOT_SIDE: u32 = 16_000;
/// The tallest full page captured, in CSS px (infinite scroll stops here).
pub const MAX_PAGE_HEIGHT: u32 = 30_000;
/// H.264's widest frame on the Mac's encoder; wider video goes to HEVC or is capped.
pub const H264_MAX_WIDTH: u32 = 4096;
/// The widest video frame Sawit records.
pub const VIDEO_MAX_WIDTH: u32 = 3840;
/// The widest image Sawit hands an agent inline; the full file is on disk.
pub const INLINE_MAX_WIDTH: u32 = 1568;

/// A screenshot's file format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
    /// A vector PDF of the whole page.
    Pdf,
}

impl ImageFormat {
    pub fn parse(text: &str) -> Result<ImageFormat, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "png" | "" => Ok(ImageFormat::Png),
            "jpeg" | "jpg" => Ok(ImageFormat::Jpeg),
            "pdf" => Ok(ImageFormat::Pdf),
            other => Err(format!("Unknown format “{other}”. Use png, jpeg or pdf.")),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Pdf => "pdf",
        }
    }
}

/// A recording's file format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoFormat {
    /// H.264 in MP4.
    #[default]
    Mp4,
    /// HEVC in QuickTime: smaller, and wider than 4096 px.
    Mov,
    /// An animated GIF, for short clips.
    Gif,
}

impl VideoFormat {
    pub fn parse(text: &str) -> Result<VideoFormat, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "mp4" | "" | "h264" => Ok(VideoFormat::Mp4),
            "mov" | "hevc" => Ok(VideoFormat::Mov),
            "gif" => Ok(VideoFormat::Gif),
            other => Err(format!("Unknown format “{other}”. Use mp4, mov or gif.")),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            VideoFormat::Mp4 => "mp4",
            VideoFormat::Mov => "mov",
            VideoFormat::Gif => "gif",
        }
    }

    pub fn codec(self) -> &'static str {
        match self {
            VideoFormat::Mp4 => "H.264",
            VideoFormat::Mov => "HEVC",
            VideoFormat::Gif => "GIF",
        }
    }
}

/// An output resolution for video: its height in pixels (the width follows the viewport's
/// shape), as the presets name them.
pub const RESOLUTIONS: &[(&str, u32)] = &[
    ("360p", 360),
    ("480p", 480),
    ("720p", 720),
    ("1080p", 1080),
    ("1440p", 1440),
    ("2160p", 2160),
];

/// A resolution by name (`720p`, `HD`, `Full HD`, `4K`…), as its short side in pixels.
pub fn resolution(name: &str) -> Option<u32> {
    let key = name.trim().to_ascii_lowercase().replace([' ', '-'], "");
    let alias = match key.as_str() {
        "hd" => "720p",
        "fullhd" | "fhd" => "1080p",
        "qhd" | "2k" => "1440p",
        "4k" | "uhd" => "2160p",
        other => other,
    };
    RESOLUTIONS
        .iter()
        .find(|(n, _)| *n == alias)
        .map(|(_, h)| *h)
}

/// The scale (device pixel ratio) a capture is taken at: what's asked (1 to 3), else the
/// device's own, else 2 for a Mac-sized page on a Retina default.
pub fn capture_scale(asked: Option<f64>, device: Option<&str>) -> f64 {
    if let Some(s) = asked.filter(|s| s.is_finite()) {
        return s.clamp(1., 3.);
    }
    device
        .and_then(device_by_name)
        .map(|d| d.default_scale())
        .unwrap_or(2.)
}

/// The even pixel size `w` × `h` CSS px make at `scale`, capped at `max_width` pixels wide with
/// the shape kept; `even` rounds both down to even numbers (H.264 needs them).
pub fn pixel_size(w: u32, h: u32, scale: f64, max_width: Option<u32>, even: bool) -> (u32, u32) {
    let (mut pw, mut ph) = (
        (f64::from(w) * scale).round().max(1.),
        (f64::from(h) * scale).round().max(1.),
    );
    if let Some(max) = max_width
        && pw > f64::from(max)
    {
        ph = (ph * f64::from(max) / pw).round().max(1.);
        pw = f64::from(max);
    }
    let (mut pw, mut ph) = (pw as u32, ph as u32);
    if even {
        pw = (pw & !1).max(2);
        ph = (ph & !1).max(2);
    }
    (pw, ph)
}

/// A video's frame size for a viewport `w` × `h` CSS px: at `resolution`'s short side (the
/// shape kept), or at `scale` capped by `max_width`, always even, and at most
/// [`VIDEO_MAX_WIDTH`] (H.264 above 4096 px is moved to HEVC by [`video_format_for`]).
pub fn video_size(
    w: u32,
    h: u32,
    scale: f64,
    resolution: Option<u32>,
    max_width: Option<u32>,
) -> (u32, u32) {
    let (w, h) = (w.max(1), h.max(1));
    match resolution {
        Some(short) => {
            let s = f64::from(short) / f64::from(w.min(h));
            pixel_size(w, h, s, Some(VIDEO_MAX_WIDTH), true)
        }
        None => pixel_size(
            w,
            h,
            scale,
            Some(max_width.unwrap_or(VIDEO_MAX_WIDTH).min(VIDEO_MAX_WIDTH)),
            true,
        ),
    }
}

/// The format a recording really uses: MP4 wider than H.264 allows becomes MOV (HEVC), and a
/// note says so.
pub fn video_format_for(asked: VideoFormat, width: u32) -> (VideoFormat, Option<String>) {
    if asked == VideoFormat::Mp4 && width > H264_MAX_WIDTH {
        return (
            VideoFormat::Mov,
            Some(format!(
                "{width} px is wider than H.264 allows ({H264_MAX_WIDTH}): recorded as HEVC in MOV."
            )),
        );
    }
    (asked, None)
}

/// One tile of a tall capture: CSS px from the top, and its height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    pub top: u32,
    pub height: u32,
}

/// How a page `height` CSS px tall is cut so no snapshot passes `max_px` pixels at `scale`.
pub fn tiles(height: u32, scale: f64, max_px: u32) -> Vec<Tile> {
    let height = height.max(1);
    let per = ((f64::from(max_px) / scale.max(0.1)).floor() as u32).max(1);
    let mut out = Vec::new();
    let mut top = 0;
    while top < height {
        let h = per.min(height - top);
        out.push(Tile { top, height: h });
        top += h;
    }
    out
}

/// What a capture is called: `<host>[-<port>]-<device>-<scale>x[-full].<ext>`, e.g.
/// `example.com-iphone-15-3x-full.png`; `n` > 1 adds `-<n>` before the extension so a second
/// shot doesn't replace the first.
pub fn file_name(url: &str, device: &str, scale: f64, full: bool, ext: &str, n: u32) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = slug(&authority.replace(':', "-"));
    let host = if host.is_empty() { "page".into() } else { host };
    let device = slug(device);
    let scale = if scale.fract() == 0. {
        format!("{scale:.0}x")
    } else {
        format!("{scale:.1}x").replace('.', "_")
    };
    let mut name = format!("{host}-{device}-{scale}");
    if full {
        name.push_str("-full");
    }
    if n > 1 {
        name.push_str(&format!("-{n}"));
    }
    format!("{name}.{ext}")
}

/// Lower case, letters, digits, dots and hyphens; runs of anything else one hyphen.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}
