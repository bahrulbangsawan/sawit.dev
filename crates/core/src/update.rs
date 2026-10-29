//! Self-update from GitHub Releases: comparing versions, reading the latest
//! release, downloading its app bundle and the script that swaps it in once
//! the app has quit.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::shell_quote;
use crate::{CoreError, Result, cmd};

/// Where releases are published.
pub const REPO: &str = "bahrulbangsawan/sawit.dev";

/// `major.minor.patch`; a leading `v` (as in tags) is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = s.trim().trim_start_matches('v').splitn(3, '.');
        let mut next = || parts.next()?.parse().ok();
        Some(Version(next()?, next()?, next()?))
    }

    /// The version this binary was built as.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// Release notes (Markdown), the version's CHANGELOG section.
    pub notes: String,
    /// The release page on GitHub.
    pub page: String,
    /// The zipped `Sawit.app`, once CI has attached it.
    pub asset: Option<String>,
}

/// Reads GitHub's `releases/latest` response.
pub fn parse_release(json: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let asset = v["assets"].as_array().and_then(|assets| {
        assets.iter().find_map(|a| {
            let name = a["name"].as_str()?;
            name.ends_with(".zip")
                .then(|| a["browser_download_url"].as_str().map(String::from))?
        })
    });
    Some(Release {
        version: Version::parse(v["tag_name"].as_str()?)?,
        notes: v["body"].as_str().unwrap_or_default().to_string(),
        page: v["html_url"].as_str().unwrap_or_default().to_string(),
        asset,
    })
}

pub async fn latest_release() -> Result<Release> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let out = cmd::run(
        "/usr/bin/curl",
        [
            "-fsSL",
            "--max-time",
            "20",
            "-H",
            "Accept: application/vnd.github+json",
            &url,
        ],
        Duration::from_secs(30),
    )
    .await?;
    if out.code != Some(0) {
        return Err(CoreError::Download(out.stderr.trim().to_string()));
    }
    parse_release(&out.stdout)
        .ok_or_else(|| CoreError::Parse("Unable to read the latest release from GitHub.".into()))
}

/// Downloads and unpacks the release's app into `dir`, returning the new
/// `Sawit.app`.
pub async fn download(release: &Release, dir: &Path) -> Result<PathBuf> {
    let url = release.asset.as_deref().ok_or_else(|| {
        CoreError::Download("This release has no app download yet. Try again later.".into())
    })?;
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    let zip = dir.join("app.zip");
    let fetch = cmd::run(
        "/usr/bin/curl",
        [
            "-fsSL".as_ref(),
            "-o".as_ref(),
            zip.as_os_str(),
            url.as_ref(),
        ],
        Duration::from_secs(600),
    )
    .await?;
    if fetch.code != Some(0) {
        return Err(CoreError::Download(fetch.stderr.trim().to_string()));
    }
    let unzip = cmd::run(
        "/usr/bin/ditto",
        [
            "-x".as_ref(),
            "-k".as_ref(),
            zip.as_os_str(),
            dir.as_os_str(),
        ],
        Duration::from_secs(120),
    )
    .await?;
    let app = dir.join("Sawit.app");
    if unzip.code != Some(0) || !app.join("Contents/MacOS/sawit").is_file() {
        return Err(CoreError::Download(format!(
            "The download has unexpected contents: {}",
            unzip.combined().trim()
        )));
    }
    let _ = std::fs::remove_file(&zip);
    Ok(app)
}

/// Shell script that waits for process `pid` to exit, replaces `current`
/// with `new` (putting the old app back if that fails) and opens it.
pub fn swap_script(pid: u32, current: &Path, new: &Path) -> String {
    let old = shell_quote(current.as_os_str());
    let new = shell_quote(new.as_os_str());
    format!(
        "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done\n\
         old={old}; new={new}\n\
         rm -rf \"$old.previous\"\n\
         if mv \"$old\" \"$old.previous\" && mv \"$new\" \"$old\"; then\n\
         \x20 rm -rf \"$old.previous\"\n\
         elif [ ! -e \"$old\" ]; then\n\
         \x20 mv \"$old.previous\" \"$old\"\n\
         fi\n\
         open \"$old\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(Version::parse("v0.0.12"), Some(Version(0, 0, 12)));
        assert_eq!(Version::parse("1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("x"), None);
        assert!(Version(0, 0, 10) > Version(0, 0, 9));
        assert!(Version(0, 1, 0) > Version(0, 0, 99));
        assert_eq!(Version(0, 0, 1).to_string(), "0.0.1");
    }

    #[test]
    fn release_json() {
        let json = r#"{
            "tag_name": "v0.0.3",
            "html_url": "https://github.com/o/sawit/releases/tag/v0.0.3",
            "body": "- feat: x",
            "assets": [
                {"name": "Sawit.dmg", "browser_download_url": "https://x/dmg"},
                {"name": "Sawit-0.0.3-macos-arm64.zip", "browser_download_url": "https://x/zip"}
            ]
        }"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.version, Version(0, 0, 3));
        assert_eq!(r.notes, "- feat: x");
        assert_eq!(r.asset.as_deref(), Some("https://x/zip"));
        let no_assets = parse_release(r#"{"tag_name":"v1.0.0","assets":[]}"#).unwrap();
        assert_eq!(no_assets.asset, None);
        assert_eq!(parse_release(r#"{"message":"Not Found"}"#), None);
    }

    #[test]
    fn swap_script_restores_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("Sawit.app");
        let new = dir.path().join("new/Sawit.app");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("v"), "old").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("v"), "new").unwrap();
        // A pid that isn't running, so it doesn't wait; `open` is stubbed out.
        let script = swap_script(999_999, &current, &new).replace("open ", "true ");
        let ok = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&script)
            .status()
            .unwrap();
        assert!(ok.success());
        assert_eq!(std::fs::read_to_string(current.join("v")).unwrap(), "new");
        assert!(!dir.path().join("Sawit.app.previous").exists());
    }
}
