//! The release pipeline's scripts, run against throwaway repositories: `.githooks/pre-push`, which
//! makes each push of main a release, and `scripts/release.sh`, which cuts it. Each test gets a
//! clone of a small workspace, released at v0.0.7 and pushed to a bare `origin`, both in a
//! temporary folder, with the hook and the script as they are in this checkout. `cargo` and
//! `osascript` are stand-ins first on the `PATH`; git reads no global or system config.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

/// How long the release push the hook leaves running in the background gets to land.
const LANDS_WITHIN: Duration = Duration::from_secs(30);

/// This checkout's root, where the hook and the script under test are.
fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Writes an executable shell script.
fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Copies one of this checkout's scripts into `dir`, executable.
fn install(from: &str, dir: &Path) {
    let to = dir.join(from);
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    fs::copy(checkout().join(from), &to).unwrap();
    fs::set_permissions(&to, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The output's stdout, after checking the command succeeded.
fn ok(out: Output) -> String {
    assert!(
        out.status.success(),
        "failed ({}): {}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A clone on main, released at v0.0.7 and pushed, with the release hook enabled.
struct Repo {
    _dir: tempfile::TempDir,
    /// The clone: main checked out, `.githooks` its hooks, `origin` the bare remote.
    work: PathBuf,
    remote: PathBuf,
    /// The stand-ins for `cargo` and `osascript`, first on the `PATH`.
    bin: PathBuf,
}

impl Repo {
    fn new() -> Repo {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let work = root.join("work");
        let remote = root.join("remote.git");
        let bin = root.join("bin");
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(&bin).unwrap();
        // release.sh updates Cargo.lock with cargo after a bump; this workspace has nothing to
        // update.
        script(&bin.join("cargo"), "exit 0\n");
        // Records the notifications the hook shows.
        let shown = root.join("notifications");
        script(
            &bin.join("osascript"),
            &format!("printf '%s\\n' \"$*\" >>'{}'\n", shown.display()),
        );
        let repo = Repo {
            _dir: dir,
            work,
            remote,
            bin,
        };
        let remote = repo.remote.display().to_string();
        repo.git(&["init", "-q", "--bare", "-b", "main", &remote]);
        repo.git(&["init", "-q", "-b", "main"]);
        repo.write(
            "Cargo.toml",
            "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.0.7\"\n",
        );
        repo.write("Cargo.lock", "version = 4\n");
        repo.write(
            "CHANGELOG.md",
            "# Changelog\n\nEvery change, by version.\n\n## [0.0.7] - 2026-10-29\n\n### Features\n\n- the first release\n",
        );
        repo.write("app.txt", "0\n");
        install("scripts/release.sh", &repo.work);
        install(".githooks/pre-push", &repo.work);
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "feat: the first release"]);
        repo.git(&["tag", "-a", "v0.0.7", "-m", "v0.0.7"]);
        repo.git(&["remote", "add", "origin", &remote]);
        repo.git(&["config", "core.hooksPath", ".githooks"]);
        ok(repo.run(
            "git",
            &["push", "-q", "origin", "main", "v0.0.7"],
            Some(("SAWIT_RELEASE_PUSH", "1")),
        ));
        repo
    }

    /// `program args` in the clone, with the stand-ins first on the `PATH`, no git config but
    /// the clone's, and none of the release's switches but `env`.
    fn run(&self, program: &str, args: &[&str], env: Option<(&str, &str)>) -> Output {
        let path = std::env::var("PATH").unwrap_or_default();
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&self.work)
            .env("PATH", format!("{}:{path}", self.bin.display()))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Sawit Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Sawit Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com");
        for var in [
            "SAWIT_RELEASE_PUSH",
            "SAWIT_SKIP_RELEASE",
            "SAWIT_RELEASE_ANY_BRANCH",
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
        ] {
            command.env_remove(var);
        }
        if let Some((key, value)) = env {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    /// `git args` in the clone, which must succeed; its stdout.
    fn git(&self, args: &[&str]) -> String {
        ok(self.run("git", args, None))
    }

    fn write(&self, file: &str, text: &str) {
        fs::write(self.work.join(file), text).unwrap();
    }

    fn read(&self, file: &str) -> String {
        fs::read_to_string(self.work.join(file)).unwrap()
    }

    /// Commits a change to the app with `subject`.
    fn commit(&self, subject: &str) {
        let app = self.read("app.txt");
        self.write("app.txt", &format!("{app}{subject}\n"));
        self.git(&["commit", "-q", "-am", subject]);
    }

    /// `git push origin main`, with `env`.
    fn push_main(&self, env: Option<(&str, &str)>) -> Output {
        self.run("git", &["push", "origin", "main"], env)
    }

    /// `scripts/release.sh`, with `env`.
    fn release(&self, env: Option<(&str, &str)>) -> Output {
        self.run("sh", &["scripts/release.sh"], env)
    }

    /// The commit `rev` names in the clone, if it names one.
    fn local(&self, rev: &str) -> Option<String> {
        self.commit_of(&self.work.join(".git"), rev)
    }

    /// The commit `rev` names on `origin`, if it names one.
    fn remote(&self, rev: &str) -> Option<String> {
        self.commit_of(&self.remote, rev)
    }

    fn commit_of(&self, git_dir: &Path, rev: &str) -> Option<String> {
        let git_dir = format!("--git-dir={}", git_dir.display());
        let rev = format!("{rev}^{{commit}}");
        let out = self.run(
            "git",
            &[&git_dir, "rev-parse", "-q", "--verify", &rev],
            None,
        );
        out.status.success().then(|| ok(out))
    }

    /// Waits for the background release push to put `rev` on `origin`; its commit.
    fn wait_for_remote(&self, rev: &str) -> String {
        let start = Instant::now();
        loop {
            if let Some(commit) = self.remote(rev) {
                return commit;
            }
            assert!(
                start.elapsed() < LANDS_WITHIN,
                "{rev} never reached origin; the release log says: {}",
                self.release_log()
            );
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn release_log(&self) -> String {
        fs::read_to_string(self.work.join(".git/sawit-release.log")).unwrap_or_default()
    }

    /// Waits for the release log to say `what`; the log.
    fn wait_for_log(&self, what: &str) -> String {
        let start = Instant::now();
        loop {
            let log = self.release_log();
            if log.contains(what) {
                return log;
            }
            assert!(
                start.elapsed() < LANDS_WITHIN,
                "the release log never said {what:?}: {log}"
            );
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// What the hook asked `osascript` to show, one call a line.
    fn notifications(&self) -> String {
        let shown = self.work.parent().unwrap().join("notifications");
        fs::read_to_string(shown).unwrap_or_default()
    }

    /// From now on `origin` takes no tags, as a remote that fails part of a push does; or takes
    /// them again.
    fn refuse_tags(&self, refuse: bool) {
        let hook = self.remote.join("hooks/update");
        if refuse {
            let refuse = "case \"$1\" in refs/tags/*) echo \"no tags today\" >&2; exit 1 ;; esac\n";
            script(&hook, refuse);
        } else {
            fs::remove_file(hook).unwrap();
        }
    }

    /// The subject of the commit `rev` names in the clone.
    fn subject(&self, rev: &str) -> String {
        self.git(&["log", "-1", "--format=%s", rev])
    }
}

/// A push of main from a checkout of main is a release: the hook cuts it, and once the push is
/// done, the release commit and its tag follow it to origin.
#[test]
fn a_push_of_main_from_main_releases_it() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    let out = repo.push_main(None);
    let said = stderr(&out);
    assert!(said.contains("release v0.0.8 created"), "{said}");
    ok(out);
    let tagged = repo.wait_for_remote("refs/tags/v0.0.8");
    assert_eq!(repo.subject("main"), "chore(release): v0.0.8");
    assert_eq!(Some(tagged), repo.local("main"));
    repo.wait_for_remote("refs/heads/main");
    assert_eq!(repo.remote("refs/heads/main"), repo.local("main"));
}

/// RELEASE-1: the release is cut on HEAD, so a push of main from a checkout of another branch
/// would release nothing. Before, the hook let such a push through without a word; now it
/// refuses it and says where to push from, and nothing reaches origin.
#[test]
fn a_push_of_main_from_another_branch_is_refused_and_says_where_to_push_from() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    repo.git(&["switch", "-q", "-c", "feat/other"]);
    let before = repo.remote("refs/heads/main");
    let out = repo.push_main(None);
    let said = stderr(&out);
    assert!(!out.status.success(), "the push went through: {said}");
    assert!(said.contains("feat/other"), "{said}");
    assert!(said.contains("checkout of main"), "{said}");
    assert!(said.contains("SAWIT_SKIP_RELEASE=1"), "{said}");
    assert_eq!(repo.remote("refs/heads/main"), before);
    assert_eq!(repo.local("refs/tags/v0.0.8"), None);
    assert_eq!(repo.subject("main"), "feat(app): a new thing");
}

/// RELEASE-1: a detached HEAD (how a scratch checkout pushes) is no checkout of main either.
#[test]
fn a_push_of_main_from_a_detached_head_is_refused() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    repo.git(&["switch", "-q", "--detach", "main"]);
    let before = repo.remote("refs/heads/main");
    let out = repo.push_main(None);
    let said = stderr(&out);
    assert!(!out.status.success(), "the push went through: {said}");
    assert!(said.contains("detached"), "{said}");
    assert_eq!(repo.remote("refs/heads/main"), before);
    assert_eq!(repo.local("refs/tags/v0.0.8"), None);
}

/// RELEASE-1: `SAWIT_SKIP_RELEASE=1` pushes main from anywhere, on purpose without a release.
#[test]
fn skip_release_pushes_main_without_a_release() {
    let repo = Repo::new();
    repo.commit("docs: a typo");
    repo.git(&["switch", "-q", "-c", "feat/other"]);
    ok(repo.push_main(Some(("SAWIT_SKIP_RELEASE", "1"))));
    assert_eq!(repo.remote("refs/heads/main"), repo.local("main"));
    assert_eq!(repo.local("refs/tags/v0.0.8"), None);
    assert_eq!(repo.subject("main"), "docs: a typo");
}

/// RELEASE-3: release.sh releases HEAD, so run on another branch it made that branch's tree the
/// release every install's updater downloads. Now it refuses, and changes nothing.
#[test]
fn release_sh_refuses_to_cut_a_release_off_main() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/other"]);
    repo.commit("feat(app): half done");
    let head = repo.local("HEAD");
    let out = repo.release(None);
    let said = stderr(&out);
    assert!(!out.status.success(), "it released: {}", ok(out));
    assert!(said.contains("main"), "{said}");
    assert!(said.contains("SAWIT_RELEASE_ANY_BRANCH=1"), "{said}");
    assert_eq!(repo.local("HEAD"), head);
    assert_eq!(repo.local("refs/tags/v0.0.8"), None);
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

/// RELEASE-3: `SAWIT_RELEASE_ANY_BRANCH=1` cuts a release off main on purpose.
#[test]
fn release_sh_cuts_one_off_main_when_asked() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "hotfix"]);
    repo.commit("fix(app): the crash");
    let tag = ok(repo.release(Some(("SAWIT_RELEASE_ANY_BRANCH", "1"))));
    assert_eq!(tag, "v0.0.8");
    assert_eq!(repo.local("refs/tags/v0.0.8"), repo.local("hotfix"));
    assert_eq!(repo.subject("hotfix"), "chore(release): v0.0.8");
}

/// RELEASE-3: a tag cut by mistake on another branch named the version main releases next.
/// release.sh then committed `chore(release)` on main and only failed at the tag, leaving that
/// untagged commit behind (and, from the hook, an aborted push). Now it stops before it changes
/// anything, and says which tag is in the way.
#[test]
fn release_sh_stops_before_changing_anything_when_its_tag_exists() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/other"]);
    repo.commit("feat(app): half done");
    repo.git(&["tag", "-a", "v0.0.8", "-m", "cut by mistake"]);
    repo.git(&["switch", "-q", "main"]);
    repo.commit("feat(app): a new thing");
    let head = repo.local("HEAD");
    let out = repo.release(None);
    let said = stderr(&out);
    assert!(!out.status.success(), "it released: {}", ok(out));
    assert!(said.contains("v0.0.8"), "{said}");
    assert_eq!(repo.local("HEAD"), head);
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

/// RELEASE-4: when the release push fails after the hook has returned, its output went only to
/// the log, which nobody reads. Now a notification says so, and the log says how to push it.
#[test]
fn a_failed_release_push_says_so() {
    let repo = Repo::new();
    repo.refuse_tags(true);
    repo.commit("feat(app): a new thing");
    ok(repo.push_main(None));
    repo.wait_for_log("no tags today");
    let start = Instant::now();
    while !repo.notifications().contains("v0.0.8") {
        assert!(
            start.elapsed() < LANDS_WITHIN,
            "no notification; the release log says: {}",
            repo.release_log()
        );
        thread::sleep(Duration::from_millis(100));
    }
    let log = repo.wait_for_log("--follow-tags");
    assert!(
        log.contains("SAWIT_RELEASE_PUSH=1 git push --follow-tags origin main"),
        "{log}"
    );
    assert_eq!(repo.remote("refs/tags/v0.0.8"), None);
}

/// RELEASE-4: a release tag a failed push left behind was never pushed again: the next release
/// went out without it, so its version never became a GitHub release. Now the next release's
/// push takes it along.
#[test]
fn a_release_tag_left_behind_goes_out_with_the_next_release() {
    let repo = Repo::new();
    repo.refuse_tags(true);
    repo.commit("feat(app): a new thing");
    ok(repo.push_main(None));
    repo.wait_for_log("no tags today");
    let released = repo.wait_for_remote("refs/heads/main");
    assert_eq!(Some(released), repo.local("refs/tags/v0.0.8"));
    assert_eq!(repo.remote("refs/tags/v0.0.8"), None);

    repo.refuse_tags(false);
    repo.commit("fix(app): a later fix");
    let out = repo.push_main(None);
    let said = stderr(&out);
    assert!(said.contains("release v0.0.9 created"), "{said}");
    ok(out);
    repo.wait_for_remote("refs/tags/v0.0.9");
    assert_eq!(
        repo.remote("refs/tags/v0.0.8"),
        repo.local("refs/tags/v0.0.8"),
        "the release log says: {}",
        repo.release_log()
    );
}

/// RELEASE-10: `git tag` strips `#` lines from a message by default, so the tag's notes lost
/// their Features and Fixes headings. Now the tag keeps the notes as CHANGELOG.md has them.
#[test]
fn the_release_tag_keeps_the_notes_headings() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    repo.commit("fix(db): an old bug");
    assert_eq!(ok(repo.release(None)), "v0.0.8");
    let tag = repo.git(&["cat-file", "-p", "v0.0.8"]);
    let notes = tag.split_once("\n\n").expect("a tag message").1;
    assert!(
        notes.starts_with("### Features\n\n- **app**: a new thing"),
        "{tag}"
    );
    assert!(notes.contains("### Fixes\n\n- **db**: an old bug"), "{tag}");
    assert!(!notes.ends_with("\n\n"), "{tag:?}");
}
