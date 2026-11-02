//! [`Repo`]: a throwaway clone released at v0.0.7 with its bare `origin`, and the waits for what
//! the hook's background release push does.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::{LANDS_WITHIN, install, ok, script, stderr};

/// A clone on main, released at v0.0.7 and pushed, with the release hook enabled.
pub(super) struct Repo {
    _dir: tempfile::TempDir,
    /// The clone: main checked out, `.githooks` its hooks, `origin` the bare remote.
    pub(super) work: PathBuf,
    pub(super) remote: PathBuf,
    /// The stand-ins for `cargo` and `osascript`, first on the `PATH`.
    bin: PathBuf,
}

impl Repo {
    pub(super) fn new() -> Repo {
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
    pub(super) fn run(&self, program: &str, args: &[&str], env: Option<(&str, &str)>) -> Output {
        self.run_in(&self.work, program, args, env)
    }

    /// `program args` in the folder `dir`, as [`Repo::run`] runs them in the clone.
    pub(super) fn run_in(
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
    pub(super) fn git(&self, args: &[&str]) -> String {
        ok(self.run("git", args, None))
    }

    fn write(&self, file: &str, text: &str) {
        fs::write(self.work.join(file), text).unwrap();
    }

    pub(super) fn read(&self, file: &str) -> String {
        fs::read_to_string(self.work.join(file)).unwrap()
    }

    /// Commits a change to the app with `subject`.
    pub(super) fn commit(&self, subject: &str) {
        let app = self.read("app.txt");
        self.write("app.txt", &format!("{app}{subject}\n"));
        self.git(&["commit", "-q", "-am", subject]);
    }

    /// `git push origin main`, with `env`.
    pub(super) fn push_main(&self, env: Option<(&str, &str)>) -> Output {
        self.run("git", &["push", "origin", "main"], env)
    }

    /// `git push origin main`, with `meanwhile` run once origin has taken main but before the
    /// push has finished: origin's post-receive hook holds the push until `meanwhile` is done.
    /// So whatever `meanwhile` does in the clone is done before the release push the hook leaves
    /// in the background starts, which waits for this push to finish.
    pub(super) fn push_main_meanwhile(&self, meanwhile: impl FnOnce()) -> Output {
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
    pub(super) fn release(&self, env: Option<(&str, &str)>) -> Output {
        self.run("sh", &["scripts/release.sh"], env)
    }

    /// The commit `rev` names in the clone, if it names one.
    pub(super) fn local(&self, rev: &str) -> Option<String> {
        self.commit_of(&self.work.join(".git"), rev)
    }

    /// The commit `rev` names on `origin`, if it names one.
    pub(super) fn remote(&self, rev: &str) -> Option<String> {
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
    pub(super) fn wait_for_remote(&self, rev: &str) -> String {
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
    pub(super) fn wait_for_remote_at(&self, rev: &str, commit: &str) {
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
    pub(super) fn wait_for_remote_off(&self, rev: &str, commit: &str) -> String {
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

    pub(super) fn release_log(&self) -> String {
        fs::read_to_string(self.work.join(".git/sawit-release.log")).unwrap_or_default()
    }

    /// Waits for the release log to say `what`; the log.
    pub(super) fn wait_for_log(&self, what: &str) -> String {
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
    pub(super) fn notifications(&self) -> String {
        let shown = self.work.parent().unwrap().join("notifications");
        fs::read_to_string(shown).unwrap_or_default()
    }

    /// Waits for the hook to show a notification that says `what`; the notifications.
    pub(super) fn wait_for_notification(&self, what: &str) -> String {
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
    pub(super) fn refuse_tags(&self, refuse: bool) {
        let hook = self.remote.join("hooks/update");
        if refuse {
            let refuse = "case \"$1\" in refs/tags/*) echo \"no tags today\" >&2; exit 1 ;; esac\n";
            script(&hook, refuse);
        } else {
            fs::remove_file(hook).unwrap();
        }
    }

    /// The subject of the commit `rev` names in the clone.
    pub(super) fn subject(&self, rev: &str) -> String {
        self.git(&["log", "-1", "--format=%s", rev])
    }
}
