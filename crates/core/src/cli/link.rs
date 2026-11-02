//! The `sawit` command in `PATH`: the symlink "Install CLI" puts in `/usr/local/bin`, and what
//! sits there now.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{CoreError, Result, cmd};

/// Where "Install CLI" puts the command.
pub const CLI_LINK: &str = "/usr/local/bin/sawit";

/// What sits at the CLI link right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    Missing,
    /// Points at this app.
    Installed,
    /// A symlink to something else (e.g. an older copy of the app).
    Other(PathBuf),
    /// A regular file we must not overwrite.
    Blocked,
}

pub fn link_state(link: &Path, exe: &Path) -> LinkState {
    match std::fs::read_link(link) {
        Ok(target) if same_file(&target, exe) => LinkState::Installed,
        Ok(target) => LinkState::Other(target),
        Err(_) if link.symlink_metadata().is_ok() => LinkState::Blocked,
        Err(_) => LinkState::Missing,
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Points `link` at `exe`, replacing an older symlink but never a regular
/// file. Fails with `PermissionDenied` when the folder isn't writable.
pub fn install_link(link: &Path, exe: &Path) -> std::io::Result<()> {
    if link_state(link, exe) == LinkState::Blocked {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists and isn't a symlink. Move or delete it, then try again.",
                link.display()
            ),
        ));
    }
    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match std::fs::remove_file(link) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    std::os::unix::fs::symlink(exe, link)
}

/// `install_link` through macOS's administrator password prompt, for the
/// root-owned `/usr/local/bin`.
pub async fn install_link_as_admin(link: &Path, exe: &Path) -> Result<()> {
    let dir = link.parent().unwrap_or(Path::new("/"));
    let script = format!(
        "mkdir -p {} && ln -sfn {} {}",
        shell_quote(dir.as_os_str()),
        shell_quote(exe.as_os_str()),
        shell_quote(link.as_os_str()),
    );
    let apple = format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(&script)
    );
    let out = cmd::run(
        "/usr/bin/osascript",
        ["-e", &apple],
        Duration::from_secs(120),
    )
    .await?;
    if out.code == Some(0) {
        return Ok(());
    }
    Err(CoreError::Command {
        program: "osascript".into(),
        code: out.code,
        stderr: out.stderr.trim().to_string(),
    })
}

pub(crate) fn shell_quote(s: &OsStr) -> String {
    format!("'{}'", s.to_string_lossy().replace('\'', r"'\''"))
}

pub(crate) fn applescript_escape(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', "\\\"")
}
