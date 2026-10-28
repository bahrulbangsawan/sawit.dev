//! Image facts for the viewer, via macOS `sips`: pixel size of any format
//! the system reads, and PNG copies of formats GPUI can't decode (HEIC,
//! AVIF), kept in a cache folder.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cmd::run;
use crate::{CoreError, Result};

const SIPS: &str = "/usr/bin/sips";
const TIMEOUT: Duration = Duration::from_secs(30);

/// Width and height in pixels.
pub async fn pixel_size(path: &Path) -> Result<(u32, u32)> {
    let out = run(
        SIPS,
        [
            "-g".as_ref(),
            "pixelWidth".as_ref(),
            "-g".as_ref(),
            "pixelHeight".as_ref(),
            path.as_os_str(),
        ],
        TIMEOUT,
    )
    .await?;
    let field = |name: &str| {
        out.stdout.lines().find_map(|l| {
            l.trim()
                .strip_prefix(name)
                .and_then(|v| v.trim_start_matches(':').trim().parse::<u32>().ok())
        })
    };
    match (field("pixelWidth"), field("pixelHeight")) {
        (Some(w), Some(h)) if out.code == Some(0) => Ok((w, h)),
        _ => Err(CoreError::Parse(format!(
            "can't read image size: {}",
            out.combined().trim()
        ))),
    }
}

/// Formats the system reads but GPUI does not decode.
pub fn needs_conversion(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(ext.as_str(), "heic" | "heif" | "avif")
}

/// A path GPUI can draw: the file itself, or a PNG copy in `cache_dir`
/// (named after the source path and its modification time, so edits make
/// a new copy).
pub async fn displayable(path: &Path, cache_dir: &Path) -> Result<PathBuf> {
    if !needs_conversion(path) {
        return Ok(path.to_path_buf());
    }
    use std::hash::{Hash, Hasher};
    let modified = std::fs::metadata(path)?.modified()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    modified.hash(&mut hasher);
    let out = cache_dir.join(format!("{:016x}.png", hasher.finish()));
    if out.is_file() {
        return Ok(out);
    }
    std::fs::create_dir_all(cache_dir)?;
    let result = run(
        SIPS,
        [
            "-s".as_ref(),
            "format".as_ref(),
            "png".as_ref(),
            path.as_os_str(),
            "--out".as_ref(),
            out.as_os_str(),
        ],
        TIMEOUT,
    )
    .await?;
    if result.code != Some(0) || !out.is_file() {
        return Err(CoreError::Parse(format!(
            "can't convert image: {}",
            result.combined().trim()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 24-bit BMP of `w × h` pixels (simple enough to write by hand).
    fn sample_bmp(path: &std::path::Path, w: u32, h: u32) {
        let row = (w * 3).div_ceil(4) * 4;
        let size = 54 + row * h;
        let mut b = Vec::new();
        b.extend(b"BM");
        b.extend(size.to_le_bytes());
        b.extend([0u8; 4]);
        b.extend(54u32.to_le_bytes());
        b.extend(40u32.to_le_bytes());
        b.extend((w as i32).to_le_bytes());
        b.extend((h as i32).to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(24u16.to_le_bytes());
        b.extend([0u8; 24]);
        for _ in 0..h {
            for _ in 0..w {
                b.extend([0x20, 0x40, 0xC0]);
            }
            b.extend(vec![0u8; (row - w * 3) as usize]);
        }
        std::fs::write(path, b).unwrap();
    }

    #[tokio::test]
    async fn reads_pixel_size() {
        let d = tempfile::tempdir().unwrap();
        let bmp = d.path().join("a.bmp");
        sample_bmp(&bmp, 3, 2);
        assert_eq!(pixel_size(&bmp).await.unwrap(), (3, 2));
        assert!(pixel_size(&d.path().join("missing.png")).await.is_err());
    }

    #[test]
    fn decodable_formats_need_no_conversion() {
        assert!(!needs_conversion(Path::new("a.png")));
        assert!(!needs_conversion(Path::new("a.JPG")));
        assert!(!needs_conversion(Path::new("a.webp")));
        assert!(needs_conversion(Path::new("a.heic")));
        assert!(needs_conversion(Path::new("a.HEIF")));
        assert!(needs_conversion(Path::new("a.avif")));
    }

    #[tokio::test]
    async fn heic_is_converted_to_a_cached_png() {
        let d = tempfile::tempdir().unwrap();
        let bmp = d.path().join("a.bmp");
        sample_bmp(&bmp, 4, 4);
        let heic = d.path().join("a.heic");
        let made = std::process::Command::new("sips")
            .args(["-s", "format", "heic"])
            .arg(&bmp)
            .arg("--out")
            .arg(&heic)
            .output()
            .unwrap();
        if !made.status.success() || !heic.exists() {
            eprintln!("skip: this macOS can't write HEIC");
            return;
        }
        let cache = d.path().join("cache");
        let png = displayable(&heic, &cache).await.unwrap();
        assert_eq!(png.extension().unwrap(), "png");
        assert!(png.starts_with(&cache));
        assert_eq!(pixel_size(&png).await.unwrap(), (4, 4));
        // Second call reuses the cached copy.
        let modified = std::fs::metadata(&png).unwrap().modified().unwrap();
        assert_eq!(displayable(&heic, &cache).await.unwrap(), png);
        assert_eq!(
            std::fs::metadata(&png).unwrap().modified().unwrap(),
            modified
        );
        // PNGs are shown as they are.
        assert_eq!(displayable(&bmp, &cache).await.unwrap(), bmp);
    }
}
