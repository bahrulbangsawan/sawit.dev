//! Video tools: HyperFrames, its CLI and its desktop app, HyperFrames Studio. Read from their docs, repositories and release files on 2026-11-06.

use super::super::{Category, Latest, Tool};
use super::tool;

/// The HyperFrames CLI (github.com/heygen-com/hyperframes, Apache-2.0, by HeyGen): an agent
/// writes a video as HTML, and `hyperframes` previews and renders it in headless Chrome.
///
/// From npm (`hyperframes`, `engines.node` ">=22"), with the package manager picked (bun and
/// pnpm installs work as well). Its docs mention `brew upgrade hyperframes`, but no formula is
/// in Homebrew's core or a published tap (`brew info hyperframes` finds none), so npm it is.
/// `hyperframes --version` prints "0.9.19" on a fast path, with no update check and no
/// telemetry. `hyperframes update --yes` updates the CLI with the manager that installed it,
/// then installs or refreshes HyperFrames' Core Skills in every agent's folder (checked on
/// this Mac: it wrote them into ~/.agents/skills and Claude Code's), so the upgrade sets
/// `HYPERFRAMES_SKIP_SKILLS=1`, which leaves skills to the Skills page; without `--yes` it asks
/// on a terminal and exits 2 when nobody can answer. Rendering needs FFmpeg and FFprobe, and Chrome: the one
/// `HYPERFRAMES_BROWSER_PATH` names, its own download in ~/.cache/hyperframes/chrome
/// (`hyperframes browser ensure`), or Google Chrome. `hyperframes doctor` checks them all.
/// Uninstalling removes the package only: ~/.cache/hyperframes and the user's projects and
/// renders stay.
pub(super) const HYPERFRAMES: Tool = Tool {
    latest: Latest::Npm("hyperframes"),
    upgrade: Some("HYPERFRAMES_SKIP_SKILLS=1 hyperframes update --yes"),
    uninstall: Some("npm uninstall -g hyperframes"),
    needs: &["node", "ffmpeg"],
    ..tool(
        "hyperframes",
        "HyperFrames CLI",
        "HeyGen's video maker for agents: write a video as HTML, render it in headless Chrome.",
        Category::Other,
        "hyperframes",
        "npm install -g hyperframes",
    )
};

/// Downloads the disk image hyperframes.studio/studio/download?os=mac redirects to (one for
/// Apple silicon and Intel) into a new temporary folder and opens it: the user finishes in
/// HeyGen's installer.
pub(crate) const STUDIO_INSTALL: &str = r#"tmp="$(mktemp -d)" && curl -fL --progress-bar -o "$tmp/Install-HyperFrames.dmg" "https://static.heygen.ai/hyperframes-oss/desktop/Install-HyperFrames.dmg" && open "$tmp/Install-HyperFrames.dmg""#;

/// HyperFrames Studio, the desktop app (Electron, bundle id `dev.hyperframes.desktop`, a
/// universal build for macOS 13 and later; 0.1.0 in the image read). The disk image holds
/// "Install HyperFrames", which checks the download is HeyGen's and copies `HyperFrames.app`
/// into /Applications, or ~/Applications when that is locked; the app updates itself
/// (Squirrel). There is no Homebrew cask. So installing downloads the image and opens it, and
/// the user finishes in that installer; then they sign in with a free HeyGen account in the
/// app, which Sawit never handles. Its version is the bundle's `CFBundleShortVersionString`.
/// Uninstalling quits it and removes the app only: projects, and renders in Downloads, stay.
pub(super) const HYPERFRAMES_STUDIO: Tool = Tool {
    // The installer's own places: a copy there is the one its uninstall removes.
    owns: &[
        "/Applications/HyperFrames.app",
        "~/Applications/HyperFrames.app",
    ],
    also: &[
        "/Applications/HyperFrames.app",
        "~/Applications/HyperFrames.app",
    ],
    uninstall: Some(
        r#"osascript -e 'if application id "dev.hyperframes.desktop" is running then tell application id "dev.hyperframes.desktop" to quit' >/dev/null 2>&1 || true
rm -rf "/Applications/HyperFrames.app" "$HOME/Applications/HyperFrames.app""#,
    ),
    ..tool(
        "hyperframes-studio",
        "HyperFrames Studio",
        "HeyGen's desktop app for making videos with an agent. Sign in with a free HeyGen account in the app.",
        Category::Other,
        "",
        STUDIO_INSTALL,
    )
};
