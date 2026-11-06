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

// ---------------------------------------------------------------- sync

/// The tree GitHub answers for a `sawit-shell-config` repository: `.zshrc` and `.zprofile`
/// changed there, `.bashrc` as here, a README and a path outside the home folder the sync
/// must never write.
const TREE: &str = include_str!("fixtures/tree.json");

fn sync_home() -> (tempfile::TempDir, Vec<(String, PathBuf)>) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let known = known_files(&home, &home.join(".config"));
    (dir, known)
}

fn tree() -> crate::github::RemoteTree {
    let v: serde_json::Value = serde_json::from_str(TREE).unwrap();
    crate::github::RemoteTree {
        commit: "c0ffee".into(),
        files: crate::github::parse_tree(&v),
    }
}

#[test]
fn the_sync_writes_only_the_shell_files_it_knows() {
    let (dir, known) = sync_home();
    let rels: Vec<&str> = known.iter().map(|(r, _)| r.as_str()).collect();
    assert!(rels.contains(&".zshrc") && rels.contains(&".config/fish/config.fish"));
    assert!(
        known
            .iter()
            .all(|(_, p)| p.starts_with(dir.path().join("home")))
    );
    // An XDG folder outside the home folder isn't synced.
    let outside = known_files(&dir.path().join("home"), Path::new("/elsewhere"));
    assert!(outside.iter().all(|(r, _)| !r.contains("fish")));
}

/// Each file's state from the fixture tree, the files here and the last sync: the README and
/// the path outside the home folder are left out.
#[test]
fn files_are_planned_against_the_fixture_tree() {
    use crate::github::{FileState, SyncBase, blob_sha};
    let (_dir, known) = sync_home();
    let path = |rel: &str| known.iter().find(|(r, _)| r == rel).unwrap().1.clone();
    std::fs::write(path(".zshrc"), "export A=1\n").unwrap();
    std::fs::write(path(".bashrc"), "export A=1\n").unwrap();
    std::fs::write(path(".profile"), "export A=1\n").unwrap();
    let local = local_hashes(&known);
    assert_eq!(local[".zshrc"], blob_sha(b"export A=1\n"));
    let mut base = SyncBase::default();
    // Synced before with .zshrc as it is here: GitHub changed it since.
    base.files
        .insert(".zshrc".into(), blob_sha(b"export A=1\n"));
    let plan = plan_files(&known, &local, &tree(), &base);
    let states: Vec<(&str, FileState)> = plan.iter().map(|f| (f.rel.as_str(), f.state)).collect();
    assert_eq!(
        states,
        [
            (".bashrc", FileState::Same),
            (".config/fish/config.fish", FileState::OnlyOnGitHub),
            (".profile", FileState::OnlyHere),
            (".zprofile", FileState::OnlyOnGitHub),
            (".zshrc", FileState::ChangedOnGitHub),
        ]
    );
    assert_eq!(
        plan[4].remote.as_deref(),
        Some("d9c8f705450d043515699a285086e6915fda4a77")
    );
}

/// A download backs each file up beside itself first, keeps its permissions (600 here, 600 for
/// a new one), records the sync, and Undo puts everything back; a file the sync doesn't know is
/// refused before anything is written.
#[test]
fn a_download_backs_up_keeps_permissions_and_undoes() {
    use std::os::unix::fs::PermissionsExt as _;
    let (dir, known) = sync_home();
    let state = dir.path().join("shell-files-sync.json");
    let path = |rel: &str| known.iter().find(|(r, _)| r == rel).unwrap().1.clone();
    let zshrc = path(".zshrc");
    std::fs::write(&zshrc, "export A=1\n").unwrap();
    std::fs::set_permissions(&zshrc, std::fs::Permissions::from_mode(0o600)).unwrap();

    let refused = apply_download(
        &known,
        &[
            (".zshrc".into(), b"export A=2\n".to_vec()),
            ("../.ssh/config".into(), b"Host *\n".to_vec()),
        ],
        &state,
        "c0ffee",
        1_793_961_302,
    );
    assert!(refused.is_err());
    assert_eq!(
        std::fs::read_to_string(&zshrc).unwrap(),
        "export A=1\n",
        "written before refusing"
    );

    let files = [
        (".zshrc".to_string(), b"export A=2\n".to_vec()),
        (".zprofile".to_string(), b"alias g=git\n".to_vec()),
    ];
    let done = apply_download(&known, &files, &state, "c0ffee", 1_793_961_302).unwrap();
    assert_eq!(std::fs::read_to_string(&zshrc).unwrap(), "export A=2\n");
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&zshrc), 0o600);
    assert_eq!(mode(&path(".zprofile")), 0o600);
    let backup = done.replaced[0].backup.clone().unwrap();
    assert_eq!(
        backup,
        zshrc.with_file_name(".zshrc.sawit-backup-20261106-103502")
    );
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "export A=1\n");
    assert_eq!(done.replaced[1].backup, None, "nothing to back up");
    let base = crate::github::SyncBase::load(&state);
    assert_eq!(base.commit.as_deref(), Some("c0ffee"));
    assert_eq!(
        base.files[".zshrc"],
        crate::github::blob_sha(b"export A=2\n")
    );

    undo_download(&done, &state).unwrap();
    assert_eq!(std::fs::read_to_string(&zshrc).unwrap(), "export A=1\n");
    assert!(!backup.exists());
    assert!(!path(".zprofile").exists());
    assert_eq!(
        crate::github::SyncBase::load(&state),
        crate::github::SyncBase::default()
    );
}
