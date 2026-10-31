//! The release pipeline's scripts, run against throwaway repositories: `.githooks/pre-push`, which
//! makes each push of main a release, and `scripts/release.sh`, which cuts it. Each test gets a
//! clone of a small workspace, released at v0.0.7 and pushed to a bare `origin`, both in a
//! temporary folder, with the hook and the script as they are in this checkout. `cargo` and
//! `osascript` are stand-ins first on the `PATH`; git reads no global or system config.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
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
        self.run_in(&self.work, program, args, env)
    }

    /// `program args` in the folder `dir`, as [`Repo::run`] runs them in the clone.
    fn run_in(
        &self,
        dir: &Path,
        program: &str,
        args: &[&str],
        env: Option<(&str, &str)>,
    ) -> Output {
        self.command(dir, program, args, env).output().unwrap()
    }

    /// `program args` in the folder `dir`, set up as [`Repo::run_in`] runs them, not yet started.
    fn command(
        &self,
        dir: &Path,
        program: &str,
        args: &[&str],
        env: Option<(&str, &str)>,
    ) -> Command {
        let path = std::env::var("PATH").unwrap_or_default();
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(dir)
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
        command
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

    /// `git push origin main`, with `meanwhile` run once origin has taken main but before the
    /// push has finished: origin's post-receive hook holds the push until `meanwhile` is done.
    /// So whatever `meanwhile` does in the clone is done before the release push the hook leaves
    /// in the background starts, which waits for this push to finish.
    fn push_main_meanwhile(&self, meanwhile: impl FnOnce()) -> Output {
        let root = self.work.parent().unwrap();
        let (taken, done) = (root.join("main-taken"), root.join("meanwhile-done"));
        let hook = self.remote.join("hooks/post-receive");
        script(
            &hook,
            &format!(
                "touch '{taken}'\ni=0\nwhile [ ! -e '{done}' ] && [ \"$i\" -lt 300 ]; do sleep 0.1; i=$((i + 1)); done\n",
                taken = taken.display(),
                done = done.display(),
            ),
        );
        let mut push = self
            .command(&self.work, "git", &["push", "origin", "main"], None)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while !taken.exists() {
            if push.try_wait().unwrap().is_some() {
                let out = push.wait_with_output().unwrap();
                panic!("the push ended before origin took main: {}", stderr(&out));
            }
            assert!(start.elapsed() < LANDS_WITHIN, "origin never took main");
            thread::sleep(Duration::from_millis(50));
        }
        meanwhile();
        // Only this push is held: the release push after it goes straight through.
        fs::remove_file(&hook).unwrap();
        fs::write(&done, "").unwrap();
        push.wait_with_output().unwrap()
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

    /// Waits for the background release push to move `rev` on `origin` to `commit`: a push of
    /// several refs lands them one at a time.
    fn wait_for_remote_at(&self, rev: &str, commit: &str) {
        let start = Instant::now();
        while self.remote(rev).as_deref() != Some(commit) {
            assert!(
                start.elapsed() < LANDS_WITHIN,
                "{rev} never reached origin at {commit}; the release log says: {}",
                self.release_log()
            );
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// Waits for the background release push to move `rev` on `origin` off `commit`; where it
    /// moved it.
    fn wait_for_remote_off(&self, rev: &str, commit: &str) -> String {
        let start = Instant::now();
        loop {
            match self.remote(rev) {
                Some(moved) if moved != commit => return moved,
                _ => {}
            }
            assert!(
                start.elapsed() < LANDS_WITHIN,
                "{rev} never moved off {commit} on origin; the release log says: {}",
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

    /// Waits for the hook to show a notification that says `what`; the notifications.
    fn wait_for_notification(&self, what: &str) -> String {
        let start = Instant::now();
        loop {
            let shown = self.notifications();
            if shown.contains(what) {
                return shown;
            }
            assert!(
                start.elapsed() < LANDS_WITHIN,
                "no notification says {what:?}: {shown}; the release log says: {}",
                self.release_log()
            );
            thread::sleep(Duration::from_millis(100));
        }
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
    assert_eq!(repo.local("main").as_deref(), Some(tagged.as_str()));
    repo.wait_for_remote_at("refs/heads/main", &tagged);
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
    let released = repo.local("main").unwrap();
    assert_eq!(
        repo.local("refs/tags/v0.0.8").as_deref(),
        Some(released.as_str())
    );
    repo.wait_for_remote_at("refs/heads/main", &released);
    repo.wait_for_log("no tags today");
    assert_eq!(repo.remote("refs/tags/v0.0.8"), None);

    repo.refuse_tags(false);
    repo.commit("fix(app): a later fix");
    let out = repo.push_main(None);
    let said = stderr(&out);
    assert!(said.contains("release v0.0.9 created"), "{said}");
    ok(out);
    repo.wait_for_remote("refs/tags/v0.0.9");
    repo.wait_for_remote_at("refs/tags/v0.0.8", &released);
}

/// Lanes move main with `git fetch . <lane>:main`, so main is checked out nowhere and is pushed
/// from a worktree of its own, which is then removed. The release push the hook leaves running
/// in the background ran in that worktree and logged in its git folder, so a worktree removed as
/// soon as the push returned took the release with it: origin got main without its release
/// commit and tag, and the log went too. Now that push runs in the main checkout, with the
/// repository's common git folder, and logs there.
#[test]
fn a_release_pushed_from_a_worktree_lands_though_the_worktree_is_removed_at_once() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    repo.git(&["switch", "-q", "-c", "feat/other"]);
    let worktree = repo.work.with_file_name("sawit-main");
    let worktree = worktree.to_str().unwrap();
    repo.git(&["worktree", "add", "-q", worktree, "main"]);
    let push_then_remove = "git -C \"$1\" push origin main && git worktree remove \"$1\"";
    let out = repo.run("sh", &["-c", push_then_remove, "sh", worktree], None);
    let said = stderr(&out);
    assert!(said.contains("release v0.0.8 created"), "{said}");
    ok(out);
    let tagged = repo.wait_for_remote("refs/tags/v0.0.8");
    assert_eq!(repo.subject("main"), "chore(release): v0.0.8");
    assert_eq!(repo.local("main").as_deref(), Some(tagged.as_str()));
    repo.wait_for_remote_at("refs/heads/main", &tagged);
    let log = repo.work.join(".git/sawit-release.log");
    assert!(said.contains(&format!("log: {}", log.display())), "{said}");
    repo.wait_for_log("v0.0.8");
}

/// The hook finds the repository's git folder with `cd`, which an exported `CDPATH` can send to
/// another folder of that name; the hook then stopped the push after cutting the release. It
/// leaves `CDPATH` out.
#[test]
fn an_exported_cdpath_leads_no_release_astray() {
    let repo = Repo::new();
    let decoy = repo.work.with_file_name("decoy");
    fs::create_dir_all(decoy.join(".git")).unwrap();
    repo.commit("feat(app): a new thing");
    let out = repo.push_main(Some(("CDPATH", decoy.to_str().unwrap())));
    let said = stderr(&out);
    ok(out);
    let log = repo.work.join(".git/sawit-release.log");
    assert!(said.contains(&format!("log: {})", log.display())), "{said}");
    let tagged = repo.wait_for_remote("refs/tags/v0.0.8");
    repo.wait_for_remote_at("refs/heads/main", &tagged);
}

/// Git finds a remote at a relative path from the checkout it runs in. The release push the hook
/// starts runs in the main checkout, which no `git worktree remove` takes away, so it finds the
/// remote a push of main from there finds: run in the git folder, it found none.
#[test]
fn a_release_reaches_a_remote_at_a_relative_path() {
    let repo = Repo::new();
    repo.git(&["remote", "set-url", "origin", "../remote.git"]);
    repo.commit("feat(app): a new thing");
    ok(repo.push_main(None));
    let tagged = repo.wait_for_remote("refs/tags/v0.0.8");
    repo.wait_for_remote_at("refs/heads/main", &tagged);
}

/// A release push that never landed (one that failed, or one an older hook ran in a worktree
/// removed under it) leaves origin with main but not its release commit and tag. Pushing main
/// again sent the release commit, but with nothing new to release, the hook left the tag behind:
/// that version got no GitHub release until the next release took its tag along. Now the tag
/// goes out with that push.
#[test]
fn a_release_left_behind_goes_out_when_main_is_pushed_again() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    ok(repo.push_main(Some(("SAWIT_SKIP_RELEASE", "1"))));
    assert_eq!(ok(repo.release(None)), "v0.0.8");
    let released = repo.local("main").unwrap();
    let out = repo.push_main(None);
    let said = stderr(&out);
    assert!(
        said.contains("nothing new since the last release"),
        "{said}"
    );
    ok(out);
    repo.wait_for_remote_at("refs/tags/v0.0.8", &released);
    assert_eq!(repo.remote("refs/heads/main"), Some(released));
}

/// Git runs the hook for `git push --dry-run` too, and doesn't tell it. A tag left behind
/// follows only a push of main that went through, so a dry run sends nothing.
#[test]
fn a_dry_run_of_main_sends_no_release_tag_left_behind() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    ok(repo.push_main(Some(("SAWIT_SKIP_RELEASE", "1"))));
    let pushed = repo.remote("refs/heads/main");
    assert_eq!(ok(repo.release(None)), "v0.0.8");
    ok(repo.run("git", &["push", "--dry-run", "origin", "main"], None));
    repo.wait_for_log("did not go out");
    assert_eq!(repo.remote("refs/heads/main"), pushed);
    assert_eq!(repo.remote("refs/tags/v0.0.8"), None);
}

/// Git runs the hook for `git push --dry-run` of main too, without saying so, and the hook cut
/// the release and pushed it for real: a dry run published a release. A release now follows only
/// a push of main that went through. A dry run still cuts it here, as the hook can't tell, and
/// says so; the next push of main takes it along.
#[test]
fn a_dry_run_of_main_publishes_no_release() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    let before = repo.remote("refs/heads/main");
    let out = repo.run("git", &["push", "--dry-run", "origin", "main"], None);
    let said = stderr(&out);
    assert!(said.contains("release v0.0.8 created"), "{said}");
    ok(out);
    let start = Instant::now();
    while !repo.release_log().contains("did not go out") {
        assert_eq!(
            repo.remote("refs/tags/v0.0.8"),
            None,
            "the dry run published v0.0.8; the release log says: {}",
            repo.release_log()
        );
        assert!(
            start.elapsed() < LANDS_WITHIN,
            "the release log never said main did not go out: {}",
            repo.release_log()
        );
        thread::sleep(Duration::from_millis(100));
    }
    let log = repo.release_log();
    assert!(
        log.contains("SAWIT_RELEASE_PUSH=1 git push --follow-tags origin main"),
        "{log}"
    );
    repo.wait_for_notification("v0.0.8");
    assert_eq!(repo.remote("refs/heads/main"), before);
    assert_eq!(repo.remote("refs/tags/v0.0.8"), None);

    let released = repo.local("refs/tags/v0.0.8").expect("no release here");
    let out = repo.push_main(None);
    let said = stderr(&out);
    assert!(
        said.contains("nothing new since the last release"),
        "{said}"
    );
    ok(out);
    repo.wait_for_remote_at("refs/tags/v0.0.8", &released);
    repo.wait_for_remote_at("refs/heads/main", &released);
}

/// A push to the remote's URL, not its name, moves no remote-tracking main. A release still
/// follows it once it went through: the hook then asks the remote where its main is.
#[test]
fn a_release_pushed_to_the_remotes_url_goes_out() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    let url = repo.remote.display().to_string();
    let out = repo.run("git", &["push", &url, "main"], None);
    let said = stderr(&out);
    assert!(said.contains("release v0.0.8 created"), "{said}");
    ok(out);
    let tagged = repo.wait_for_remote("refs/tags/v0.0.8");
    repo.wait_for_remote_at("refs/heads/main", &tagged);
}

/// With nothing new, a push of main to the remote's URL said "main did not go out" though it
/// had, and left the release tag behind: no remote-tracking main moved. Now the hook asks the
/// remote, and the tag follows.
#[test]
fn a_release_left_behind_goes_out_when_main_is_pushed_again_to_the_remotes_url() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    ok(repo.push_main(Some(("SAWIT_SKIP_RELEASE", "1"))));
    assert_eq!(ok(repo.release(None)), "v0.0.8");
    let released = repo.local("main").unwrap();
    let url = repo.remote.display().to_string();
    let out = repo.run("git", &["push", &url, "main"], None);
    let said = stderr(&out);
    assert!(
        said.contains("nothing new since the last release"),
        "{said}"
    );
    ok(out);
    repo.wait_for_remote_at("refs/tags/v0.0.8", &released);
    assert_eq!(repo.remote("refs/heads/main"), Some(released));
}

/// The release push the hook leaves in the background pushed `main` as it was when that push
/// ran, not the release: a commit made on main while main was pushed, or as soon as the push
/// returned, went out with the release commit, in no release. Now it sends the release commit.
#[test]
fn the_release_push_sends_the_release_commit_not_main_as_it_is_by_then() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    let pushed = repo.local("main").unwrap();
    let out = repo.push_main_meanwhile(|| repo.commit("feat(app): made while main was pushed"));
    let said = stderr(&out);
    assert!(said.contains("release v0.0.8 created"), "{said}");
    ok(out);
    let tagged = repo.wait_for_remote("refs/tags/v0.0.8");
    let main = repo.wait_for_remote_off("refs/heads/main", &pushed);
    assert_eq!(
        main,
        tagged,
        "origin's main isn't the release commit; the release log says: {}",
        repo.release_log()
    );
    assert_eq!(
        repo.subject("main"),
        "feat(app): made while main was pushed"
    );
}

/// With nothing new to release, the push that takes a tag left behind along pushed `main` as it
/// was by then: a lane landing on main as soon as main was pushed (`git fetch . <lane>:main`)
/// went out with that tag, in no release. Now it sends the commit that went out as main.
#[test]
fn a_tag_left_behind_follows_the_commit_pushed_not_main_as_it_is_by_then() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    ok(repo.push_main(Some(("SAWIT_SKIP_RELEASE", "1"))));
    assert_eq!(ok(repo.release(None)), "v0.0.8");
    let released = repo.local("main").unwrap();
    let out = repo.push_main_meanwhile(|| repo.commit("feat(app): a lane landed"));
    let said = stderr(&out);
    assert!(
        said.contains("nothing new since the last release"),
        "{said}"
    );
    ok(out);
    repo.wait_for_remote_at("refs/tags/v0.0.8", &released);
    // Until the release push has finished, or has moved origin's main.
    let start = Instant::now();
    while !repo.release_log().contains("is out")
        && repo.remote("refs/heads/main").as_deref() == Some(released.as_str())
    {
        assert!(
            start.elapsed() < LANDS_WITHIN,
            "the release push never finished: {}",
            repo.release_log()
        );
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        repo.remote("refs/heads/main"),
        Some(released),
        "the release log says: {}",
        repo.release_log()
    );
    assert_eq!(repo.subject("main"), "feat(app): a lane landed");
}

/// The refusal's steps get run as printed, by agents too, from wherever `git push` was typed.
/// They named the worktree `../sawit-main`, which from a folder inside the checkout put it
/// inside the checkout, and they removed it straight after the push. Now they name it by its
/// full path, beside the checkout, and say to remove it once the release's tag is on origin.
#[test]
fn following_the_refusal_from_a_folder_inside_the_checkout_releases_main() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    repo.git(&["switch", "-q", "-c", "feat/other"]);
    let inside = repo.work.join("docs");
    fs::create_dir(&inside).unwrap();
    let out = repo.run_in(&inside, "git", &["push", "origin", "main"], None);
    let said = stderr(&out);
    assert!(!out.status.success(), "the push went through: {said}");
    let steps: Vec<&str> = said
        .lines()
        .filter_map(|line| line.strip_prefix("pre-push:   "))
        .collect();
    let (remove, add_and_push) = steps.split_last().expect("no steps");
    for step in add_and_push {
        ok(repo.run_in(&inside, "sh", &["-c", step], None));
    }
    let worktree = repo.work.with_file_name("sawit-main");
    assert!(worktree.is_dir(), "no worktree beside the checkout: {said}");
    assert_eq!(repo.git(&["status", "--porcelain"]), "", "{said}");
    assert!(said.contains("git ls-remote --tags origin"), "{said}");
    let tagged = repo.wait_for_remote("refs/tags/v0.0.8");
    ok(repo.run_in(&inside, "sh", &["-c", remove], None));
    assert!(!worktree.exists(), "{said}");
    assert_eq!(repo.local("main"), Some(tagged));
}

/// The section release.sh wrote for `version` in CHANGELOG.md, without its heading line.
fn section(changelog: &str, version: &str) -> String {
    let heading = format!("## [{version}]");
    let start = changelog.find(&heading).expect("no section");
    let rest = &changelog[start + heading.len()..];
    let end = rest.find("\n## [").unwrap_or(rest.len());
    rest[..end].to_string()
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

/// RELEASE-5: the notes go to the GitHub release, the updater and the in-app Changelog, so they
/// list what users get. Docs, tests, refactors and build, CI and chore work (by type, or a docs
/// scope) are left out; before, every subject was listed, plan records and test fixes included.
/// So is the sawit.dev website (a site scope), which deploys on its own and isn't in the app: its
/// `feat(site)` and `fix(site)` commits read as app features and fixes in v0.0.8's notes.
#[test]
fn the_notes_list_what_users_get_and_leave_out_internal_work() {
    let repo = Repo::new();
    for subject in [
        "feat(site): the sawit.dev website",
        "fix(site): the landing page's copy button",
        "fix(site)!: a new address for the changelog page",
        "feat(app): a new thing",
        "fix(db): an old bug",
        "perf: a faster start",
        "a subject with no type",
        "docs: record the live check",
        "docs(readme): new screenshots",
        "test(core): cover the parser",
        "chore: ignore the worktrees",
        "refactor(mcp): one decoder",
        "build: ship the notice",
        "ci: pin the actions",
        "style: rustfmt",
        "fix(docs): readme wording",
    ] {
        repo.commit(subject);
    }
    assert_eq!(ok(repo.release(None)), "v0.0.8");
    let notes = section(&repo.read("CHANGELOG.md"), "0.0.8");
    for listed in [
        "### Features\n\n- **app**: a new thing\n",
        "### Fixes\n\n- **db**: an old bug\n",
        "### Performance\n\n- a faster start\n",
        "### Other\n\n- a subject with no type\n",
    ] {
        assert!(notes.contains(listed), "{listed:?} isn't in {notes}");
    }
    for left_out in [
        "live check",
        "screenshots",
        "parser",
        "worktrees",
        "decoder",
        "notice",
        "actions",
        "rustfmt",
        "wording",
        "website",
        "copy button",
        "changelog page",
    ] {
        assert!(!notes.contains(left_out), "{left_out:?} is in {notes}");
    }
}

/// RELEASE-5: a push of nothing but internal work is still a release (every push of main is),
/// and its notes list that work rather than nothing.
#[test]
fn the_notes_list_internal_work_when_that_is_all_there_is() {
    let repo = Repo::new();
    repo.commit("docs: a typo");
    repo.commit("test(core): a flaky test");
    assert_eq!(ok(repo.release(None)), "v0.0.8");
    let notes = section(&repo.read("CHANGELOG.md"), "0.0.8");
    assert!(
        notes.contains("### Other\n\n- **core**: a flaky test\n- a typo\n"),
        "{notes}"
    );
}

/// RELEASE-5: `release.sh --dry-run` prints the section it would write, so the notes can be read
/// before a push publishes them, and changes nothing.
#[test]
fn release_sh_dry_run_prints_the_notes_and_changes_nothing() {
    let repo = Repo::new();
    repo.commit("feat(app): a new thing");
    let head = repo.local("HEAD");
    let printed = ok(repo.run("sh", &["scripts/release.sh", "--dry-run"], None));
    assert!(printed.starts_with("## [0.0.8] - "), "{printed}");
    assert!(
        printed.contains("### Features\n\n- **app**: a new thing"),
        "{printed}"
    );
    assert_eq!(repo.local("HEAD"), head);
    assert_eq!(repo.local("refs/tags/v0.0.8"), None);
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

/// The shell script of the step `name` in `.github/workflows/release.yml`: its `run: |` block,
/// dedented. The workflow keeps it inline, so it runs for any tag, also one from before a script
/// file it could call.
fn workflow_step(name: &str) -> String {
    let workflow = fs::read_to_string(checkout().join(".github/workflows/release.yml")).unwrap();
    let mut lines = workflow.lines();
    lines
        .by_ref()
        .find(|line| line.trim() == format!("- name: {name}"))
        .unwrap_or_else(|| panic!("release.yml has no step {name:?}"));
    let run = lines
        .by_ref()
        .find(|line| line.trim() == "run: |")
        .expect("the step runs a script");
    let indent = run.len() - run.trim_start().len();
    let mut body = String::new();
    for line in lines {
        let blank = line.trim().is_empty();
        if !blank && line.len() - line.trim_start().len() <= indent {
            break;
        }
        body.push_str(line.get(indent + 2..).unwrap_or(""));
        body.push('\n');
    }
    body
}

/// release.yml's Publish step, for tag v0.0.8, run in a temporary folder that has the build's
/// zip and DMG and the notes, against a stand-in `gh` that records each call and keeps one
/// release in a file, there from the start when `exists`; what it printed and the calls.
fn publish(exists: bool) -> (Output, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (calls, release) = (root.join("calls"), root.join("release"));
    if exists {
        fs::write(&release, "v0.0.8").unwrap();
    }
    let gh = format!(
        r#"printf '%s\n' "$*" >>'{calls}'
case "$1 $2" in
"release view") [ -e '{release}' ] || {{ echo "release not found" >&2; exit 1; }} ;;
"release create")
    [ ! -e '{release}' ] || {{ echo "HTTP 422: Validation Failed (Release.tag_name already_exists)" >&2; exit 1; }}
    echo "$3" >'{release}' ;;
"release upload") [ -e '{release}' ] || {{ echo "release not found" >&2; exit 1; }} ;;
*) echo "unexpected: gh $*" >&2; exit 1 ;;
esac
"#,
        calls = calls.display(),
        release = release.display(),
    );
    fs::create_dir_all(root.join("bin")).unwrap();
    script(&root.join("bin/gh"), &gh);
    fs::create_dir_all(root.join("target/release")).unwrap();
    for file in [
        "target/release/Sawit-0.0.8-macos-arm64.zip",
        "target/release/Sawit.dmg",
        "notes.md",
    ] {
        fs::write(root.join(file), "").unwrap();
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let out = Command::new("sh")
        .args(["-euc", &workflow_step("Publish")])
        .current_dir(root)
        .env("PATH", format!("{}:{path}", root.join("bin").display()))
        .env("TAG", "v0.0.8")
        .output()
        .unwrap();
    (out, fs::read_to_string(calls).unwrap_or_default())
}

/// A tag's first run publishes its release, with its notes and the zip and DMG.
#[test]
fn publishing_a_tag_creates_its_release() {
    let (out, calls) = publish(false);
    ok(out);
    assert!(
        calls.contains(
            "release create v0.0.8 --title Sawit v0.0.8 --notes-file notes.md \
             target/release/Sawit-0.0.8-macos-arm64.zip target/release/Sawit.dmg\n"
        ),
        "{calls}"
    );
}

/// RELEASE-11: release.yml can be run by hand to rebuild a tag, but its publish step always
/// created the release, which GitHub refuses once the tag has one: a rebuild helped only after a
/// run that failed before publishing. Now a release that exists gets the new assets in place of
/// its old ones, and keeps its notes.
#[test]
fn a_rebuild_replaces_the_assets_of_a_release_that_exists() {
    let (out, calls) = publish(true);
    let said = stderr(&out);
    assert!(out.status.success(), "{said}\n{calls}");
    assert!(
        calls.contains(
            "release upload v0.0.8 --clobber \
             target/release/Sawit-0.0.8-macos-arm64.zip target/release/Sawit.dmg\n"
        ),
        "{calls}"
    );
    assert!(!calls.contains("release create"), "{calls}");
}

/// The step of a workflow whose `uses:` line is `lines[at]`: from its `- ` line to the next line
/// indented no deeper than that dash.
fn step_at(lines: &[&str], at: usize) -> String {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let start = (0..=at)
        .rev()
        .find(|&i| lines[i].trim_start().starts_with("- "))
        .expect("a step");
    let dash = indent(lines[start]);
    let end = (start + 1..lines.len())
        .find(|&i| !lines[i].trim().is_empty() && indent(lines[i]) <= dash)
        .unwrap_or(lines.len());
    lines[start..end].join("\n")
}

/// RELEASE-9: a workflow's token reaches the repository, and its jobs hold more: the release
/// job's token can write to the repository, and the site job deploys the `install.sh` users pipe
/// into `sh`. So each workflow says what its token may do rather than taking the repository's
/// default, pins each action it runs to a commit, since a tag can be moved to other code, and
/// has checkout leave no token in `.git/config`, where every later step could read it. The
/// site's workflow came later and had none of this.
#[test]
fn every_workflow_pins_its_actions_and_keeps_no_token_on_disk() {
    let mut checked = Vec::new();
    for entry in fs::read_dir(checkout().join(".github/workflows")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.ends_with(".yml") && !name.ends_with(".yaml") {
            continue;
        }
        let workflow = fs::read_to_string(&path).unwrap();
        assert!(
            workflow
                .lines()
                .any(|line| line.starts_with("permissions:")),
            "{name} doesn't say what its token may do"
        );
        let lines: Vec<&str> = workflow.lines().collect();
        let mut checkouts = 0;
        for (at, line) in lines.iter().enumerate() {
            let Some(action) = line.trim().trim_start_matches("- ").strip_prefix("uses: ") else {
                continue;
            };
            if action.starts_with("./") {
                continue;
            }
            let commit = action.split_once('@').map_or("", |(_, at)| at);
            let commit = commit.split_whitespace().next().unwrap_or("");
            assert!(
                commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit()),
                "{name}: {action} isn't pinned to a commit"
            );
            if action.starts_with("actions/checkout@") {
                checkouts += 1;
                let step = step_at(&lines, at);
                assert!(
                    step.contains("persist-credentials: false"),
                    "{name}: checkout leaves the token on disk:\n{step}"
                );
            }
        }
        assert!(checkouts > 0, "{name} has no checkout step");
        checked.push(name);
    }
    for workflow in ["release.yml", "site.yml"] {
        assert!(checked.iter().any(|name| name == workflow), "{checked:?}");
    }
}

/// RELEASE-8: NOTICE goes into Sawit.app with the licence texts of what the binary carries. A
/// crate from a git repository brings code no crates.io release vouches for, so NOTICE names
/// each such repository: craft-libs, compiled in through FigCraft since the native canvas, was
/// missing.
#[test]
fn notice_names_every_git_repository_the_build_uses() {
    let lock = fs::read_to_string(checkout().join("Cargo.lock")).unwrap();
    let notice = fs::read_to_string(checkout().join("NOTICE")).unwrap();
    let mut repositories: Vec<&str> = lock
        .lines()
        .filter_map(|line| line.strip_prefix("source = \"git+"))
        .filter_map(|url| url.split(['?', '#', '"']).next())
        .collect();
    repositories.sort_unstable();
    repositories.dedup();
    assert!(!repositories.is_empty(), "Cargo.lock has no git source");
    let missing: Vec<&str> = repositories
        .into_iter()
        .filter(|repository| !notice.contains(repository))
        .collect();
    assert!(missing.is_empty(), "NOTICE doesn't name {missing:?}");
}
