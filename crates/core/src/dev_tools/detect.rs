//! Which tools are installed: each one's command, found by the login shell or at its known
//! places, and its version.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use super::programs::{PRELUDE, expand_in, is_cap_cli};
use super::removal::installed_outside;
use super::script::shell;
use super::{TOOLS, Tool, find};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub version: Option<String>,
    /// For an app whose copy isn't the one its own commands install,
    /// upgrade and remove (dragged in from a disk image, kept up to date by
    /// its own updater): what it was installed outside of, said instead of
    /// offering Upgrade, Reinstall or Uninstall. `None` for every other copy
    /// ([`installed_outside`]).
    pub outside: Option<Outside>,
}

/// What an app's copy was installed outside of ([`Found::outside`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outside {
    /// A cask's app that Homebrew's Caskroom doesn't have: dragged in from
    /// its disk image.
    Homebrew,
    /// Another app with no command of its install script leading into it.
    InstallScript,
}

impl Outside {
    /// What it was installed outside of: "Homebrew", "its install script".
    pub fn of(self) -> &'static str {
        match self {
            Outside::Homebrew => "Homebrew",
            Outside::InstallScript => "its install script",
        }
    }

    /// "Installed outside Homebrew", for the page.
    pub fn text(self) -> String {
        format!("Installed outside {}", self.of())
    }
}

pub(super) const MARK: &str = "__SAWIT_TOOL__";

/// How long detection waits for the login shell.
const DETECT_TIMEOUT: Duration = Duration::from_secs(20);

/// What [`detect`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detection {
    pub found: HashMap<&'static str, Found>,
    /// Why the login shell couldn't say which tools it finds, when it
    /// couldn't. `found` then has only the tools at their usual places, so
    /// a tool missing from it may still be installed.
    pub problem: Option<String>,
}

/// Which tools are installed: `command -v` in one login shell, then the
/// known install locations, then each one's `--version` (an app's
/// `Info.plist`).
pub async fn detect() -> Detection {
    detect_as(crate::probe::login_shell().as_deref()).await
}

/// The script detection runs in the login shell: a line with the path of
/// each tool's command, or an empty one. An app without a command (PureMac)
/// has no line: it's found at its `also` paths only.
pub(super) fn detect_script() -> String {
    let mut script = String::from(PRELUDE);
    for t in TOOLS.iter().filter(|t| !t.bin.is_empty()) {
        // nvm is a shell function; its path is the script that defines it.
        let probe = if t.id == "nvm" {
            r#"[ -s "$NVM_DIR/nvm.sh" ] && printf '%s' "$NVM_DIR/nvm.sh""#.to_string()
        } else if t.id == "python" {
            // macOS's /usr/bin/python3 is the old system Python (or a stub
            // that asks to install the Xcode tools); it doesn't count.
            "/usr/bin/which -a python3 2>/dev/null | grep -v '^/usr/bin/' | head -n 1".to_string()
        } else {
            format!("command -v {} 2>/dev/null", t.bin)
        };
        script.push_str(&format!("printf '{MARK}{}\\t%s\\n' \"$({probe})\"\n", t.id));
    }
    script
}

/// The first of `tool`'s `also` paths that is there, under `home`.
pub(super) fn at_known_place(tool: &Tool, home: &Path) -> Option<PathBuf> {
    tool.also
        .iter()
        .map(|p| expand_in(p, home))
        .find(|p| p.exists())
}

/// The version of `tool`'s copy at `path`: an app bundle's from its
/// `Info.plist`, else the number its `--version` prints.
pub(super) async fn version_of(tool: &Tool, path: &Path) -> Option<String> {
    if !tool.version {
        return None;
    }
    if is_app(path) {
        return app_version(path).await;
    }
    let out = crate::cmd::run(path, ["--version"], Duration::from_secs(3))
        .await
        .ok()?;
    let text = if out.stdout.trim().is_empty() {
        out.stderr
    } else {
        out.stdout
    };
    short_version(&text)
}

/// [`detect`] with `login` as `$SHELL`.
pub(super) async fn detect_as(login: Option<&str>) -> Detection {
    let mut cmd = shell(login);
    cmd.arg("-c")
        .arg(detect_script())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let shell_name = crate::probe::script_shell(login).display().to_string();
    let (out, problem) = match tokio::time::timeout(DETECT_TIMEOUT, cmd.output()).await {
        Ok(Ok(out)) => {
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            // It printed a line per tool, unless it failed before the first.
            let problem = (!text.contains(MARK)).then(|| {
                let why = String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map(|l| format!(": {}", l.trim()))
                    .unwrap_or_default();
                format!(
                    "{shell_name} -l failed (exit {}){why}",
                    out.status
                        .code()
                        .map_or_else(|| "signal".to_string(), |c| c.to_string())
                )
            });
            (text, problem)
        }
        Ok(Err(e)) => (
            String::new(),
            Some(format!("{shell_name} couldn't start: {e}")),
        ),
        Err(_) => (
            String::new(),
            Some(format!(
                "{shell_name} -l didn't answer within {} seconds",
                DETECT_TIMEOUT.as_secs()
            )),
        ),
    };
    let mut paths: HashMap<&'static str, PathBuf> = HashMap::new();
    for line in out.lines() {
        let Some((id, path)) = line
            .split_once(MARK)
            .and_then(|(_, rest)| rest.split_once('\t'))
        else {
            continue;
        };
        // `command -v` names functions and aliases without a slash.
        if let Some(t) = find(id)
            && path.starts_with('/')
            // Another program by the same name (Capistrano's `cap`).
            && (t.id != "cap" || is_cap_cli(Path::new(path)))
        {
            paths.insert(t.id, PathBuf::from(path));
        }
    }
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    for t in TOOLS {
        if paths.contains_key(t.id) {
            continue;
        }
        if let Some(p) = at_known_place(t, &home) {
            paths.insert(t.id, p);
        }
    }
    let versions = futures::future::join_all(
        paths
            .iter()
            .map(|(id, path)| async move { Some((*id, version_of(find(id)?, path).await?)) }),
    )
    .await;
    let mut versions: HashMap<&str, String> = versions.into_iter().flatten().collect();
    let found = paths
        .into_iter()
        .map(|(id, path)| {
            let version = versions.remove(id);
            let outside = find(id).and_then(|t| installed_outside(t, &path));
            (
                id,
                Found {
                    path,
                    version,
                    outside,
                },
            )
        })
        .collect();
    Detection { found, problem }
}

/// The version number in a `--version` line ("Python 3.13.2" → "3.13.2").
pub(super) fn short_version(text: &str) -> Option<String> {
    let first = text.lines().find(|l| !l.trim().is_empty())?;
    let word = first
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .map(|w| w.trim_start_matches('v'))
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.contains('.'))?;
    Some(word.to_string())
}

pub(super) fn is_app(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "app")
}

/// The version an app bundle gives in its `Info.plist`
/// (`CFBundleShortVersionString`, "3.2.0"), read with `plutil`, which reads
/// the XML and binary forms alike; `None` without one.
pub(super) async fn app_version(app: &Path) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    let out = crate::cmd::run(
        "/usr/bin/plutil",
        [
            std::ffi::OsStr::new("-extract"),
            "CFBundleShortVersionString".as_ref(),
            "raw".as_ref(),
            "-o".as_ref(),
            "-".as_ref(),
            plist.as_os_str(),
        ],
        Duration::from_secs(3),
    )
    .await
    .ok()?;
    if out.code != Some(0) {
        return None;
    }
    let version = out.stdout.trim();
    version
        .starts_with(|c: char| c.is_ascii_digit())
        .then(|| version.to_string())
}
