//! Shell startup files on this machine (`~/.zshrc`, `/etc/profile`, …):
//! which ones exist, which shell reads them and when, so they can be opened
//! in the editor without hunting for the path.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::{applescript_escape, shell_quote};
use crate::{CoreError, Result, cmd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// In the home folder: only this user's shells.
    User,
    /// Under `/etc`: every user's shells; saving needs an admin password.
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellConfig {
    pub path: PathBuf,
    /// `zsh`, `bash`, `sh`, `fish` or `pwsh`.
    pub shell: &'static str,
    /// When the shell reads it.
    pub note: &'static str,
    pub scope: Scope,
    /// Saving it works without an admin password.
    pub writable: bool,
}

/// `(shell, path under $HOME, note)`. The fish and PowerShell ones live
/// under the config folder, so they're listed separately.
const USER: &[(&str, &str, &str)] = &[
    ("zsh", ".zshenv", "Every zsh, scripts too"),
    ("zsh", ".zprofile", "zsh login shells"),
    ("zsh", ".zshrc", "Interactive zsh"),
    ("bash", ".bash_profile", "bash login shells"),
    ("bash", ".bashrc", "Interactive bash"),
    ("sh", ".profile", "Login shells without their own file"),
];

/// `(shell, path under the config folder, note)`.
const USER_CONFIG: &[(&str, &str, &str)] = &[
    ("fish", "fish/config.fish", "Every fish shell"),
    (
        "pwsh",
        "powershell/Microsoft.PowerShell_profile.ps1",
        "PowerShell ($PROFILE)",
    ),
    (
        "pwsh",
        "powershell/profile.ps1",
        "PowerShell, all hosts ($PROFILE.CurrentUserAllHosts)",
    ),
];

/// `(shell, absolute path, note)`. Debian keeps zsh's under `/etc/zsh/`.
const SYSTEM: &[(&str, &str, &str)] = &[
    ("zsh", "/etc/zshenv", "Every zsh, all users"),
    ("zsh", "/etc/zsh/zshenv", "Every zsh, all users"),
    ("zsh", "/etc/zprofile", "zsh login shells, all users"),
    ("zsh", "/etc/zsh/zprofile", "zsh login shells, all users"),
    ("zsh", "/etc/zshrc", "Interactive zsh, all users"),
    ("zsh", "/etc/zsh/zshrc", "Interactive zsh, all users"),
    ("bash", "/etc/bash.bashrc", "Interactive bash, all users"),
    ("bash", "/etc/bashrc", "Interactive bash, all users"),
    ("sh", "/etc/profile", "Login shells, all users"),
    (
        "fish",
        "/etc/fish/config.fish",
        "Every fish shell, all users",
    ),
];

/// The startup files that exist, the user's own first.
pub fn detect() -> Vec<ShellConfig> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    detect_in(&home, &config, Path::new("/"))
}

/// [`detect`] with the folders given, for tests. `root` stands in for `/`.
fn detect_in(home: &Path, config: &Path, root: &Path) -> Vec<ShellConfig> {
    let user = USER
        .iter()
        .map(|&(shell, rel, note)| (shell, home.join(rel), note, Scope::User))
        .chain(
            USER_CONFIG
                .iter()
                .map(|&(shell, rel, note)| (shell, config.join(rel), note, Scope::User)),
        );
    let system = SYSTEM.iter().map(|&(shell, abs, note)| {
        let rel = abs.trim_start_matches('/');
        (shell, root.join(rel), note, Scope::System)
    });
    user.chain(system)
        .filter(|(_, path, _, _)| path.is_file())
        .map(|(shell, path, note, scope)| ShellConfig {
            writable: writable(&path),
            path,
            shell,
            note,
            scope,
        })
        .collect()
}

/// The login shell's name (`zsh`, `bash`, …), from `$SHELL`.
pub fn login_shell() -> Option<String> {
    let shell = std::env::var_os("SHELL")?;
    Path::new(&shell)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// Whether a save can replace the file: it and its folder are writable.
fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    let can_write = |p: &Path| {
        std::ffi::CString::new(p.as_os_str().as_bytes())
            .is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0)
    };
    can_write(path) && path.parent().is_some_and(can_write)
}

/// Writes `text` over `path` through macOS's administrator password prompt,
/// for root-owned files such as `/etc/zshrc`. The file keeps its owner and
/// permissions: only its contents are replaced.
pub async fn write_text_as_admin(path: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let tmp = std::env::temp_dir().join(format!(
        "sawit-admin-save-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    let script = format!(
        "/bin/cat {} > {}",
        shell_quote(tmp.as_os_str()),
        shell_quote(path.as_os_str()),
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
    .await;
    let _ = std::fs::remove_file(&tmp);
    let out = out?;
    if out.code == Some(0) {
        return Ok(());
    }
    // -128: the password prompt was dismissed.
    if out.stderr.contains("(-128)") {
        return Err(CoreError::Config("Saving was cancelled.".into()));
    }
    Err(CoreError::Command {
        program: "osascript".into(),
        code: out.code,
        stderr: out.stderr.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "# test\n").unwrap();
    }

    #[test]
    fn lists_only_files_that_exist_user_first() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let config = home.join(".config");
        let root = dir.path().join("root");
        touch(&home.join(".zshrc"));
        touch(&home.join(".profile"));
        touch(&config.join("fish/config.fish"));
        touch(&root.join("etc/zprofile"));
        // A folder with a config's name is not a config.
        std::fs::create_dir_all(home.join(".bashrc")).unwrap();

        let found = detect_in(&home, &config, &root);
        let names: Vec<_> = found
            .iter()
            .map(|c| (c.shell, c.scope, c.path.clone()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("zsh", Scope::User, home.join(".zshrc")),
                ("sh", Scope::User, home.join(".profile")),
                ("fish", Scope::User, config.join("fish/config.fish")),
                ("zsh", Scope::System, root.join("etc/zprofile")),
            ]
        );
        assert!(found.iter().all(|c| c.writable));
    }

    #[test]
    fn read_only_file_is_not_writable() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".zshrc");
        touch(&path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(!writable(&path));
    }
}
