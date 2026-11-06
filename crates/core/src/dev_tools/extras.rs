//! What some tools' rows say and do besides install, upgrade and uninstall: whether the GitHub
//! CLI is signed in, whether HyperFrames has a Chrome to render with, starting voxl safely,
//! and the macOS permissions voxl needs.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The GitHub CLI's sign-in, from `gh auth status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhAuth {
    /// Signed in to github.com, as this login when gh says.
    SignedIn(Option<String>),
    SignedOut,
}

/// Reads `gh auth status` for github.com with the `gh` at `gh`. It prints the token masked
/// (`gho_****`), and nothing of it is kept. `None` when gh couldn't be asked.
pub async fn gh_auth(gh: &Path) -> Option<GhAuth> {
    let out = crate::cmd::run(
        gh,
        ["auth", "status", "--hostname", "github.com"],
        Duration::from_secs(8),
    )
    .await
    .ok()?;
    Some(parse_gh_auth(
        out.code,
        &format!("{}\n{}", out.stdout, out.stderr),
    ))
}

/// What `gh auth status` said: exit 0 is signed in, and the login is in "Logged in to
/// github.com account octocat (keyring)" (gh 2.40 and later) or "Logged in to github.com as
/// octocat" (older).
pub fn parse_gh_auth(code: Option<i32>, text: &str) -> GhAuth {
    if code != Some(0) {
        return GhAuth::SignedOut;
    }
    let login = text.lines().find_map(|line| {
        let rest = line.split_once("Logged in to github.com")?.1.trim_start();
        let rest = rest
            .strip_prefix("account ")
            .or_else(|| rest.strip_prefix("as "))?;
        let login: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        (!login.is_empty()).then_some(login)
    });
    GhAuth::SignedIn(login)
}

/// The Chrome HyperFrames would render with, looked for where its CLI looks: the browser
/// `HYPERFRAMES_BROWSER_PATH` names, its own download (under `HYPERFRAMES_CACHE_DIR`, else
/// ~/.cache/hyperframes, in `chrome`), Puppeteer's download, then Google Chrome or Chromium.
/// `None` when there is none: rendering then needs one (`hyperframes browser ensure` downloads
/// it), and Sawit says so rather than installing one.
pub fn hyperframes_browser(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(path) = env("HYPERFRAMES_BROWSER_PATH").filter(|p| !p.is_empty()) {
        let path = PathBuf::from(path);
        return path.exists().then_some(path);
    }
    let cache = env("HYPERFRAMES_CACHE_DIR")
        .filter(|p| p.starts_with('/'))
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache/hyperframes"));
    let downloads = [
        cache.join("chrome"),
        home.join(".cache/puppeteer/chrome-headless-shell"),
        home.join(".cache/puppeteer/chrome"),
    ];
    let downloaded = downloads
        .into_iter()
        .find(|dir| std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some()));
    downloaded.or_else(|| {
        [
            PathBuf::from("/Applications/Google Chrome.app"),
            home.join("Applications/Google Chrome.app"),
            PathBuf::from("/Applications/Chromium.app"),
            home.join("Applications/Chromium.app"),
        ]
        .into_iter()
        .find(|app| app.exists())
    })
}

/// The port voxl listens on unless told otherwise.
pub const VOXL_PORT: u16 = 6901;

/// How Sawit starts voxl: the command typed into a new terminal, and the environment that
/// terminal's shell gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoxlLaunch {
    /// `voxl --addr=127.0.0.1 --port=6901`: on this Mac only. Never `0.0.0.0` or a network
    /// address, which would show the whole screen, and take input, from other machines.
    pub command: String,
    /// `VOXL_PASSWORD`, a new random one each time: in the shell's environment, so it is never
    /// in the command line, the shell's history or Sawit's logs. voxl prints a sign-in link
    /// with it, for the user, in that terminal only.
    pub env: Vec<(String, String)>,
}

/// How Sawit starts voxl on `port`, signed in with a new 32-character random password.
pub fn voxl_launch(port: u16) -> std::io::Result<VoxlLaunch> {
    Ok(VoxlLaunch {
        command: format!("voxl --addr=127.0.0.1 --port={port}"),
        env: vec![("VOXL_PASSWORD".into(), random_password()?)],
    })
}

/// 32 hex characters (128 bits) from the system's random source.
fn random_password() -> std::io::Result<String> {
    use std::io::Read as _;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// A macOS privacy permission a tool needs, as Sawit (the app whose terminal starts it, which
/// macOS asks about) has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    ScreenRecording,
    Accessibility,
}

impl Privacy {
    pub const ALL: [Privacy; 2] = [Privacy::ScreenRecording, Privacy::Accessibility];

    pub fn name(self) -> &'static str {
        match self {
            Privacy::ScreenRecording => "Screen Recording",
            Privacy::Accessibility => "Accessibility",
        }
    }

    /// Its pane in System Settings, Privacy & Security.
    pub fn settings_url(self) -> &'static str {
        match self {
            Privacy::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            Privacy::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
        }
    }

    /// Whether this process has it. Only asks; it never brings up macOS's prompt.
    pub fn granted(self) -> bool {
        #[cfg(target_os = "macos")]
        {
            #[link(name = "CoreGraphics", kind = "framework")]
            unsafe extern "C" {
                fn CGPreflightScreenCaptureAccess() -> bool;
            }
            #[link(name = "ApplicationServices", kind = "framework")]
            unsafe extern "C" {
                fn AXIsProcessTrusted() -> bool;
            }
            // SAFETY: both only read this process's permission, with no arguments.
            unsafe {
                match self {
                    Privacy::ScreenRecording => CGPreflightScreenCaptureAccess(),
                    Privacy::Accessibility => AXIsProcessTrusted(),
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        false
    }
}

/// The permissions `tool` needs on macOS: Screen Recording and Accessibility for voxl.
pub fn privacy_needs(tool: &str) -> &'static [Privacy] {
    match tool {
        "voxl" => &Privacy::ALL,
        _ => &[],
    }
}
