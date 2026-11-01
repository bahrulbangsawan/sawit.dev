//! The login shell: the shell Sawit's scripts run in, and the `PATH` a login shell has.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cmd;

/// The login shell, from `$SHELL`.
pub fn login_shell() -> Option<String> {
    std::env::var("SHELL").ok().filter(|s| !s.is_empty())
}

/// Shells that run POSIX `sh` scripts, which Sawit's are.
const POSIX_SHELLS: [&str; 6] = ["sh", "bash", "zsh", "dash", "ksh", "mksh"];

/// Whether `shell` (a path or a name) runs POSIX scripts and takes `-l -c`.
pub fn is_posix_shell(shell: &str) -> bool {
    Path::new(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| POSIX_SHELLS.contains(&n))
}

/// The shell Sawit's scripts run in: the login shell when it runs POSIX
/// scripts, so its profile's `PATH` and variables apply; else zsh, macOS's
/// default. fish, nu, PowerShell and tcsh can't run them (fish refuses
/// `${VAR:-default}` before running a line; tcsh refuses `-l -c`).
pub fn script_shell(login: Option<&str>) -> PathBuf {
    match login {
        Some(shell) if is_posix_shell(shell) => PathBuf::from(shell),
        _ => PathBuf::from("/bin/zsh"),
    }
}

/// The `PATH` a login shell would have. GUI apps launched from Finder get a
/// minimal `PATH`, so the app adopts this at startup (as Zed does).
pub async fn login_shell_path() -> Option<String> {
    login_shell_path_of(login_shell().as_deref()).await
}

/// [`login_shell_path`] for `login`. fish answers this one (it joins `$PATH`
/// with colons), so it's asked first; a login shell that can't, zsh then.
pub(super) async fn login_shell_path_of(login: Option<&str>) -> Option<String> {
    let shell = login.unwrap_or("/bin/zsh");
    match shell_path(Path::new(shell)).await {
        Some(path) => Some(path),
        None if !is_posix_shell(shell) => shell_path(&script_shell(None)).await,
        None => None,
    }
}

async fn shell_path(shell: &Path) -> Option<String> {
    const MARK: &str = "__SAWIT_PATH__";
    let script = format!("printf '{MARK}%s' \"$PATH\"");
    let out = cmd::run(shell, ["-l", "-c", script.as_str()], Duration::from_secs(5))
        .await
        .ok()?;
    let path = out.stdout.rsplit_once(MARK)?.1.trim().to_string();
    (!path.is_empty()).then_some(path)
}
