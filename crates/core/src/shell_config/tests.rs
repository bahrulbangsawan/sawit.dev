//! Tests for shell startup files: finding them, what a backup takes, and spotting secrets.

use std::path::{Path, PathBuf};

use super::backup::backup_files_in;
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
