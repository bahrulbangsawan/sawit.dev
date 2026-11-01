//! Shell startup files on this machine (`~/.zshrc`, `/etc/profile`, …):
//! which ones exist, which shell reads them and when, so they can be opened
//! in the editor without hunting for the path. The user's own files can be
//! saved to a private GitHub repository.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::{applescript_escape, shell_quote};
use crate::{CoreError, Result, cmd, github};

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

/// Whether the login shell `login` ([`login_shell`]) reads the files meant
/// for `shell`: its own, and bash reads `sh`'s `.profile` too. When the
/// login shell isn't known, every file counts as read.
pub fn reads(login: Option<&str>, shell: &str) -> bool {
    match login {
        Some(login) => login == shell || (login == "bash" && shell == "sh"),
        None => true,
    }
}

/// Whether a save can replace the file: it and its folder are writable.
pub(crate) fn writable(path: &Path) -> bool {
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

// ---------------------------------------------------------------- GitHub

/// The private repository the user's files are saved to.
pub const BACKUP_REPO: &str = "sawit-shell-config";

/// The user's own startup files (not the system-wide ones) and the path
/// each gets in the repository: its path under the home folder.
pub fn backup_files() -> Vec<(String, PathBuf)> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    backup_files_in(&detect(), &home)
}

fn backup_files_in(configs: &[ShellConfig], home: &Path) -> Vec<(String, PathBuf)> {
    configs
        .iter()
        .filter(|c| c.scope == Scope::User)
        .map(|c| {
            let rel = c.path.strip_prefix(home).unwrap_or(&c.path);
            let rel = rel.to_string_lossy().trim_start_matches('/').to_string();
            (rel, c.path.clone())
        })
        .collect()
}

/// Lines that set what looks like a secret, as `line number, variable`: a
/// name like `OPENAI_API_KEY`, `GITHUB_PAT` or `DB_PASSWORD`, or a value
/// shaped like a known token (`sk-…`, `ghp_…`, `xoxb-…`, `AKIA…`), in the
/// ways shells set variables ([`assignment`]). Values that only point at
/// another variable or a command (`$(op read …)`) don't count.
pub fn find_secrets(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (ix, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = assignment(line) else {
            continue;
        };
        let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        let reference = value.is_empty() || value.starts_with('$') || value.starts_with('`');
        if valid && !reference && (secret_name(name) || secret_value(value)) {
            out.push((ix + 1, name.to_string()));
        }
    }
    out
}

/// The variable a line sets and its value, as shells write it: `NAME=value`
/// alone or after `export`, `typeset -x`, `declare -x`, `local` or
/// `readonly`; fish's `set -gx NAME value`; csh's `setenv NAME value`;
/// `launchctl setenv NAME value`; PowerShell's `$env:NAME = "value"` and
/// `[Environment]::SetEnvironmentVariable("NAME", "value", …)`.
fn assignment(line: &str) -> Option<(&str, String)> {
    fn words(rest: &str) -> Option<(&str, String)> {
        let mut words = rest.split_whitespace().skip_while(|w| w.starts_with('-'));
        let name = words.next()?;
        Some((name, words.collect::<Vec<_>>().join(" ")))
    }
    for prefix in ["set ", "setenv ", "launchctl setenv "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return words(rest);
        }
    }
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("$env:") {
        let (name, value) = line[5..].split_once('=')?;
        return Some((name.trim(), value.trim().to_string()));
    }
    for prefix in [
        "[environment]::setenvironmentvariable(",
        "[system.environment]::setenvironmentvariable(",
    ] {
        if lower.starts_with(prefix) {
            let mut parts = line[prefix.len()..].splitn(3, ',');
            let name = parts.next()?.trim().trim_matches(|c| c == '"' || c == '\'');
            let value = parts.next()?.trim().trim_end_matches(')').trim();
            return Some((name, value.to_string()));
        }
    }
    let mut rest = line;
    for keyword in ["export ", "typeset ", "declare ", "local ", "readonly "] {
        if let Some(after) = rest.strip_prefix(keyword) {
            rest = after.trim_start();
            while rest.starts_with('-') {
                rest = rest.split_once(char::is_whitespace)?.1.trim_start();
            }
            break;
        }
    }
    let (name, value) = rest.split_once('=')?;
    Some((name.trim(), value.trim().to_string()))
}

/// A variable name that holds a secret.
fn secret_name(name: &str) -> bool {
    const PARTS: [&str; 9] = [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "PASSPHRASE",
        "API_KEY",
        "PRIVATE_KEY",
        "ACCESS_KEY",
        "CREDENTIAL",
    ];
    let upper = name.to_ascii_uppercase();
    PARTS.iter().any(|p| upper.contains(p)) || upper.ends_with("_KEY") || upper.ends_with("_PAT")
}

/// A value shaped like a known kind of token or key.
fn secret_value(value: &str) -> bool {
    const PREFIXES: [&str; 17] = [
        "sk-",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
        "glpat-",
        "hf_",
        "npm_",
        "AIza",
        "AKIA",
        "ASIA",
    ];
    PREFIXES
        .iter()
        .any(|p| value.starts_with(p) && value.len() >= p.len() + 8 && !value.contains(' '))
}

/// What saving would do, worked out before anything is sent.
#[derive(Debug, Clone)]
pub struct BackupPlan {
    pub login: String,
    /// `None` when the repository will be created.
    pub repo: Option<github::OwnRepo>,
    /// `~/.zshrc:12 OPENAI_API_KEY`, one per line that looks like a secret.
    pub secrets: Vec<String>,
}

/// Looks up the account and the repository, and scans the files.
pub async fn plan_backup(token: &str) -> Result<BackupPlan> {
    let login = github::account(token).await?.login;
    let repo = github::own_repo(token, &login, BACKUP_REPO).await?;
    if let Some(r) = &repo
        && !r.private
    {
        return Err(CoreError::GitHub(format!(
            "github.com/{login}/{BACKUP_REPO} is public, so your shell config won't be saved there. Make it private on GitHub, then save again."
        )));
    }
    let mut secrets = Vec::new();
    for (rel, path) in backup_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (line, name) in find_secrets(&text) {
            secrets.push(format!("~/{rel}:{line} {name}"));
        }
    }
    Ok(BackupPlan {
        login,
        repo,
        secrets,
    })
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub url: String,
    pub files: usize,
    /// The repository was made by this save.
    pub created: bool,
    /// `false` when GitHub already had these exact files.
    pub changed: bool,
}

/// Creates the private repository when it's missing, then commits every
/// one of the user's startup files to it.
pub async fn save_backup(token: &str, plan: &BackupPlan) -> Result<Saved> {
    let files = backup_files();
    if files.is_empty() {
        return Err(CoreError::Config(
            "There are no shell config files in your home folder to save.".into(),
        ));
    }
    let mut contents = Vec::new();
    for (rel, path) in &files {
        contents.push((rel.clone(), std::fs::read(path)?));
    }
    let (repo, created) = match &plan.repo {
        Some(r) => (r.clone(), false),
        None => (
            github::create_private_repo(
                token,
                BACKUP_REPO,
                "Shell config files (~/.zshrc and friends), saved by Sawit.",
            )
            .await?,
            true,
        ),
    };
    let host = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "this Mac".into());
    let names: Vec<&str> = files.iter().map(|(rel, _)| rel.as_str()).collect();
    let message = format!("Save shell config from {host}\n\n{}", names.join("\n"));
    let changed = github::commit_files(
        token,
        &plan.login,
        BACKUP_REPO,
        &repo.default_branch,
        &contents,
        &message,
    )
    .await?;
    Ok(Saved {
        url: repo.html_url,
        files: files.len(),
        created,
        changed,
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
    fn backup_takes_only_the_users_files_by_home_path() {
        let home = Path::new("/Users/me");
        let config = |path: &str, scope| ShellConfig {
            path: PathBuf::from(path),
            shell: "zsh",
            note: "",
            scope,
            writable: true,
        };
        let files = backup_files_in(
            &[
                config("/Users/me/.zshrc", Scope::User),
                config("/Users/me/.config/fish/config.fish", Scope::User),
                config("/etc/zshrc", Scope::System),
            ],
            home,
        );
        let rels: Vec<_> = files.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(rels, [".zshrc", ".config/fish/config.fish"]);
    }

    #[test]
    fn secrets_are_found_but_references_are_not() {
        let text = "# export OLD_TOKEN=abc
export OPENAI_API_KEY=\"sk-123\"
export GITHUB_TOKEN=$(gh auth token)
NPM_TOKEN=\"$NPM\"
export PATH=$HOME/bin:$PATH
set -gx HF_TOKEN hf_abc
DB_PASSWORD='hunter2'
";
        let found: Vec<_> = find_secrets(text)
            .into_iter()
            .map(|(l, n)| format!("{l} {n}"))
            .collect();
        assert_eq!(found, ["2 OPENAI_API_KEY", "6 HF_TOKEN", "7 DB_PASSWORD"]);
    }

    #[test]
    fn secrets_in_other_names_shells_and_shapes_are_found() {
        let text = r#"export ANTHROPIC_KEY="sk-ant-api03-abcdefgh"
export GITHUB_PAT=ghp_0123456789abcdef
GOOGLE_CREDENTIALS='{"type": "service_account"}'
typeset -x OPENAI_TOKEN=sk-proj-abcdefgh
declare -x DB_PASSWORD=hunter2
launchctl setenv NPM_TOKEN npm_abcdefghijkl
setenv HF_TOKEN hf_abcdefghijkl
$env:OPENAI_API_KEY = "sk-abcdefgh12345678"
[Environment]::SetEnvironmentVariable("SLACK_SECRET", "xyz123", "User")
export WEBHOOK=xoxb-1234-5678-abcdefgh
export SOMETHING=AKIAABCDEFGHIJKLMNOP
export KEYTIMEOUT=1
export SSH_AUTH_SOCK=/private/tmp/agent.sock
export EDITOR=vim
"#;
        let found: Vec<_> = find_secrets(text)
            .into_iter()
            .map(|(l, n)| format!("{l} {n}"))
            .collect();
        assert_eq!(
            found,
            [
                "1 ANTHROPIC_KEY",
                "2 GITHUB_PAT",
                "3 GOOGLE_CREDENTIALS",
                "4 OPENAI_TOKEN",
                "5 DB_PASSWORD",
                "6 NPM_TOKEN",
                "7 HF_TOKEN",
                "8 OPENAI_API_KEY",
                "9 SLACK_SECRET",
                "10 WEBHOOK",
                "11 SOMETHING",
            ]
        );
    }

    #[test]
    fn the_login_shell_reads_its_own_files_and_bash_reads_profile() {
        assert!(reads(Some("zsh"), "zsh"));
        assert!(!reads(Some("zsh"), "bash"));
        assert!(!reads(Some("zsh"), "sh"));
        assert!(reads(Some("bash"), "sh"));
        assert!(!reads(Some("fish"), "sh"));
        assert!(reads(None, "pwsh"));
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
