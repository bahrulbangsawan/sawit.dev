//! Runs the system tools (`lsof`, `ps`, `docker`, `brew`) to find out who is
//! listening on a port and explains the result.

mod jobs;
mod report;
mod shell;

pub use jobs::listening_jobs;
#[cfg(unix)]
pub use jobs::{is_alive, terminate};
pub use report::{OwnedListener, PortReport, check_port, check_port_with};
pub use shell::{is_posix_shell, login_shell, login_shell_path, script_shell};

use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::cmd;

const TIMEOUT: Duration = Duration::from_secs(3);

/// Where the tools live. `Default` uses bare names resolved via `PATH`.
#[derive(Debug, Clone)]
pub struct ProbeEnv {
    pub lsof: PathBuf,
    pub ps: PathBuf,
    pub docker: Option<PathBuf>,
    pub brew_prefix: Option<PathBuf>,
}

impl Default for ProbeEnv {
    fn default() -> Self {
        ProbeEnv {
            lsof: "lsof".into(),
            ps: "ps".into(),
            docker: None,
            brew_prefix: None,
        }
    }
}

impl ProbeEnv {
    /// Detects `docker` and the Homebrew prefix once per process.
    pub async fn detect() -> ProbeEnv {
        static ENV: OnceCell<ProbeEnv> = OnceCell::const_new();
        ENV.get_or_init(|| async {
            ProbeEnv {
                docker: find_in_path("docker"),
                brew_prefix: brew_prefix().await,
                ..ProbeEnv::default()
            }
        })
        .await
        .clone()
    }
}

/// Finds an executable named `name` in the current `PATH`.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    find_in(&std::env::var_os("PATH")?, name)
}

pub(crate) fn find_in(path: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// `brew --prefix`, if Homebrew is installed.
pub async fn brew_prefix() -> Option<PathBuf> {
    let brew = find_in_path("brew")?;
    let out = cmd::run(brew, ["--prefix"], TIMEOUT).await.ok()?;
    let prefix = out.stdout.trim();
    (out.code == Some(0) && !prefix.is_empty()).then(|| PathBuf::from(prefix))
}

#[cfg(test)]
mod tests;
