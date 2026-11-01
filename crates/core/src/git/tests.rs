//! Tests for git: reading its output, the commit graph's layout, and real git in scratch
//! repositories.

use std::path::{Path, PathBuf};

use super::*;

/// The GitHub sign-in reaches git as a credential helper scoped to `https://github.com`, which
/// reads the token from the run's environment: the command line (what `ps` shows) holds the
/// helper, never the token, and without a sign-in there is no helper at all.
#[test]
fn the_sign_in_reaches_git_without_showing_on_its_command_line() {
    let token = "gho_secret123";
    let cmd = command(Path::new("/repo"), &["push"], Some(token));
    let cmd = cmd.as_std();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(
        args.iter().all(|a| !a.contains(token)),
        "the token is on the command line: {args:?}"
    );
    let helper = args
        .iter()
        .find(|a| a.starts_with("credential."))
        .expect("no credential helper");
    assert!(
        helper.starts_with("credential.https://github.com.helper="),
        "the helper isn't scoped to github.com: {helper}"
    );
    assert!(helper.contains(&format!("${TOKEN_ENV}")));
    assert!(
        helper.contains("test \"$1\" = get"),
        "the helper would answer store or erase too: {helper}"
    );
    assert_eq!(args.last().map(String::as_str), Some("push"));
    let env: Vec<(String, Option<String>)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    assert!(env.contains(&(TOKEN_ENV.into(), Some(token.into()))));
    assert!(env.contains(&("GIT_TERMINAL_PROMPT".into(), Some("0".into()))));

    let signed_out = command(Path::new("/repo"), &["push"], None);
    let signed_out = signed_out.as_std();
    assert!(
        signed_out
            .get_args()
            .all(|a| !a.to_string_lossy().starts_with("credential.")),
        "a helper without a sign-in"
    );
    assert!(signed_out.get_envs().all(|(k, _)| k != TOKEN_ENV));
}

#[test]
fn status_reads_branch_and_every_entry_kind() {
    let raw = "# branch.oid 1234567890abcdef\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa bbb src/a file.rs\0\
1 A. N... 000000 100644 100644 000 ccc new.rs\0\
2 R. N... 100644 100644 100644 ddd ddd R100 b.rs\0old b.rs\0\
u UU N... 100644 100644 100644 100644 e f g conflict.rs\0\
? notes.txt\0";
    let s = parse_status(raw);
    assert_eq!(s.branch.as_deref(), Some("main"));
    assert_eq!(s.head.as_deref(), Some("1234567"));
    assert_eq!(s.upstream.as_deref(), Some("origin/main"));
    assert_eq!((s.ahead, s.behind), (2, 1));
    let paths: Vec<_> = s.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "src/a file.rs",
            "new.rs",
            "b.rs",
            "conflict.rs",
            "notes.txt"
        ]
    );
    assert_eq!(s.files[2].from.as_deref(), Some("old b.rs"));
    assert_eq!(s.staged().count(), 2);
    assert_eq!(s.unstaged().count(), 2);
    assert_eq!(s.conflicted().count(), 1);
    assert!(s.files[4].untracked());
}

#[test]
fn status_before_first_commit_and_detached() {
    let s = parse_status("# branch.oid (initial)\0# branch.head (detached)\0");
    assert_eq!(s.head, None);
    assert_eq!(s.branch, None);
    assert!(s.files.is_empty());
}

#[test]
fn branches_skip_remote_head_and_mark_current() {
    let raw = "refs/heads/main\0origin/main\0*\nrefs/heads/feat\0\0 \nrefs/remotes/origin/HEAD\0\0 \nrefs/remotes/origin/main\0\0 \n";
    let b = parse_branches(raw);
    assert_eq!(b.len(), 3);
    assert!(b[0].current && !b[0].remote);
    assert_eq!(b[0].upstream.as_deref(), Some("origin/main"));
    assert_eq!(b[1].upstream, None);
    assert!(b[2].remote && b[2].name == "origin/main");
}

#[test]
fn github_slug_from_remote_urls() {
    let want = Some(("acme".to_string(), "app".to_string()));
    assert_eq!(github_slug("https://github.com/acme/app.git"), want);
    assert_eq!(github_slug("https://github.com/acme/app"), want);
    assert_eq!(github_slug("git@github.com:acme/app.git"), want);
    assert_eq!(github_slug("ssh://git@github.com/acme/app.git"), want);
    assert_eq!(github_slug("https://me@github.com/acme/app.git"), want);
    assert_eq!(github_slug("https://gitlab.com/acme/app.git"), None);
    assert_eq!(github_slug("https://github.com/acme"), None);
}

fn c(hash: &str, parents: &[&str]) -> Commit {
    Commit {
        hash: hash.into(),
        parents: parents.iter().map(|p| p.to_string()).collect(),
        author: String::new(),
        email: String::new(),
        time: 0,
        refs: Vec::new(),
        subject: String::new(),
    }
}

#[test]
fn log_parses_records() {
    let raw = "aaa\0bbb ccc\0Ann\0ann@x\x001700000000\0HEAD -> main, origin/main\0Merge feat\x1e\nbbb\0\0Bo\0bo@x\x001600000000\0\0First\x1e\n";
    let log = parse_log(raw);
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].parents, ["bbb", "ccc"]);
    assert_eq!(log[0].refs, ["HEAD -> main", "origin/main"]);
    assert_eq!(log[0].subject, "Merge feat");
    assert!(log[1].parents.is_empty() && log[1].refs.is_empty());
    assert_eq!(log[1].time, 1600000000);
}

#[test]
fn graph_linear_history_stays_in_one_lane() {
    let rows = graph(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
    assert!(rows.iter().all(|r| r.lane == 0));
    assert_eq!(rows[0].edges, [Edge::Out(0)]);
    assert_eq!(rows[1].edges, [Edge::In(0), Edge::Out(0)]);
    assert_eq!(rows[2].edges, [Edge::In(0)]);
    assert_eq!(lane_count(&rows), 1);
}

#[test]
fn graph_merge_branches_out_and_joins_back() {
    // m merges f into b; f and b both come from a.
    let rows = graph(&[
        c("m", &["b", "f"]),
        c("f", &["a"]),
        c("b", &["a"]),
        c("a", &[]),
    ]);
    assert_eq!(rows[0].lane, 0);
    assert_eq!(rows[0].edges, [Edge::Out(0), Edge::Out(1)]);
    assert_eq!(rows[1].lane, 1);
    assert_eq!(rows[1].edges, [Edge::Pass(0), Edge::In(1), Edge::Out(1)]);
    assert_eq!(rows[2].lane, 0);
    // Both lanes wait for `a`; b's lane takes it, f's lane closes into it.
    assert_eq!(rows[2].edges, [Edge::In(0), Edge::Pass(1), Edge::Out(0)]);
    assert_eq!(rows[3].lane, 0);
    assert_eq!(rows[3].edges, [Edge::In(0), Edge::In(1)]);
    assert_eq!(lane_count(&rows), 2);
}

#[test]
fn graph_unrelated_tips_get_their_own_lanes() {
    let rows = graph(&[c("x", &["a"]), c("y", &["a"]), c("a", &[])]);
    assert_eq!(rows[0].lane, 0);
    assert_eq!(rows[1].lane, 1);
    assert_eq!(rows[1].edges, [Edge::Pass(0), Edge::Out(1)]);
    assert_eq!(rows[2].edges, [Edge::In(0), Edge::In(1)]);
}

/// Runs real git in a scratch repository.
#[test]
fn end_to_end_status_stage_commit_branch_merge() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        git_ok(repo, &["init", "-q", "-b", "main"], None, LOCAL_TIMEOUT)
            .await
            .unwrap();
        for (k, v) in [
            ("user.name", "Test"),
            ("user.email", "t@example.com"),
            ("commit.gpgsign", "false"),
        ] {
            git_ok(repo, &["config", k, v], None, LOCAL_TIMEOUT)
                .await
                .unwrap();
        }

        assert_eq!(
            discover(&repo.join("."))
                .await
                .map(|p| p.canonicalize().unwrap()),
            Some(repo.canonicalize().unwrap())
        );
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        let s = status(repo).await.unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert!(s.files[0].untracked());
        assert_eq!(
            diff(repo, &s.files[0], false)
                .await
                .unwrap()
                .lines()
                .filter(|l| l.starts_with('+'))
                .count(),
            2
        );

        stage_all(repo).await.unwrap();
        assert_eq!(status(repo).await.unwrap().staged().count(), 1);
        unstage_all(repo).await.unwrap();
        assert_eq!(status(repo).await.unwrap().staged().count(), 0);
        stage(repo, &["a.txt".into()]).await.unwrap();
        commit(repo, "first", false).await.unwrap();
        assert_eq!(last_message(repo).await.unwrap(), "first");

        create_branch(repo, "feat").await.unwrap();
        std::fs::write(repo.join("b.txt"), "two\n").unwrap();
        stage_all(repo).await.unwrap();
        commit(repo, "second", false).await.unwrap();
        let main = branches(repo)
            .await
            .unwrap()
            .into_iter()
            .find(|b| b.name == "main")
            .unwrap();
        switch(repo, &main).await.unwrap();
        assert!(merge(repo, "feat").await.unwrap().ok);

        let log = log(repo, 10, None).await.unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].subject, "second");
        let detail = show(repo, &log[0].hash).await.unwrap();
        assert_eq!(detail.body, "second");
        assert!(detail.patch.contains("+two"));

        std::fs::write(repo.join("a.txt"), "changed\n").unwrap();
        let s = status(repo).await.unwrap();
        discard(repo, &s.files[0]).await.unwrap();
        assert!(status(repo).await.unwrap().files.is_empty());
    });
}

/// Runs `f` on a current-thread runtime, as the end-to-end test does.
fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

/// A new repository in `dir`, with one commit so that a worktree can be added.
async fn scratch_repo(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git_ok(dir, &["init", "-q", "-b", "main"], None, LOCAL_TIMEOUT)
        .await
        .unwrap();
    let config = ["-c", "user.name=Test", "-c", "user.email=t@example.com"];
    let commit = [
        "commit",
        "-q",
        "--allow-empty",
        "--no-gpg-sign",
        "-m",
        "first",
    ];
    git_ok(
        dir,
        &[&config[..], &commit[..]].concat(),
        None,
        LOCAL_TIMEOUT,
    )
    .await
    .unwrap();
    discover(dir).await.unwrap()
}

/// Whether git ignores `path` (relative to `repo`).
async fn ignored(repo: &Path, path: &str) -> bool {
    git(repo, &["check-ignore", "-q", path], None, LOCAL_TIMEOUT)
        .await
        .unwrap()
        .ok
}

/// Add to .git/info/exclude in a linked worktree (or a submodule), where `.git` is a file
/// naming the real folder, writes the exclude file git reads, which the worktrees share
/// (FILES-7). It used to read `<worktree>/.git/info/exclude` and fail with "Not a directory".
#[test]
fn excluding_in_a_linked_worktree_writes_the_exclude_file_git_reads() {
    let d = tempfile::tempdir().unwrap();
    block_on(async {
        let main = scratch_repo(&d.path().join("main")).await;
        let tree = d.path().join("tree");
        git_ok(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "tree",
                &tree.to_string_lossy(),
            ],
            None,
            LOCAL_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(tree.join(".git").is_file());
        let tree = discover(&tree).await.unwrap();
        std::fs::write(tree.join("build.log"), "").unwrap();

        assert!(ignore(&tree, &tree.join("build.log"), true).await.unwrap());
        let exclude = std::fs::read_to_string(main.join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|l| l == "/build.log"), "{exclude}");
        assert!(ignored(&tree, "build.log").await);
    });
}

/// Add to .gitignore on a link ignores the link, not what it leads to (FILES-8): the line
/// is the link's path, without the trailing `/` that matches only folders. A name with a
/// line break would write a second, unanchored line, so it is refused; spaces that end a
/// name are kept, which git otherwise drops from a line.
#[test]
fn ignoring_a_link_ignores_the_link() {
    let d = tempfile::tempdir().unwrap();
    block_on(async {
        let repo = scratch_repo(&d.path().join("repo")).await;
        std::fs::create_dir_all(repo.join("releases/v2")).unwrap();
        std::fs::write(repo.join("releases/v2/a.txt"), "").unwrap();
        std::os::unix::fs::symlink("releases/v2", repo.join("current")).unwrap();

        assert!(ignore(&repo, &repo.join("current"), false).await.unwrap());
        let gitignore = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert_eq!(gitignore, "/current\n");
        assert!(ignored(&repo, "current").await);
        assert!(!ignored(&repo, "releases/v2/a.txt").await);

        let odd = repo.join("x\nsrc");
        std::fs::write(&odd, "").unwrap();
        let refused = ignore(&repo, &odd, false).await.unwrap_err();
        assert!(refused.to_string().contains("line break"), "{refused}");
        let spaced = repo.join("notes ");
        std::fs::write(&spaced, "").unwrap();
        assert!(ignore(&repo, &spaced, false).await.unwrap());
        assert_eq!(
            std::fs::read_to_string(repo.join(".gitignore")).unwrap(),
            "/current\n/notes\\ \n"
        );
        assert!(ignored(&repo, "notes ").await);
    });
}

/// A `.gitignore` that is a link isn't written through: a cloned repository could point it
/// at a file of the user's (`~/.zshrc`), and Add to .gitignore on a file it names
/// `$(…)` would add that command there. Git itself doesn't read a `.gitignore` that is a link.
#[test]
fn an_ignore_file_that_is_a_link_is_not_written_through() {
    let d = tempfile::tempdir().unwrap();
    block_on(async {
        let repo = scratch_repo(&d.path().join("repo")).await;
        let outside = d.path().join("zshrc");
        std::fs::write(&outside, "export A=1\n").unwrap();
        std::os::unix::fs::symlink(&outside, repo.join(".gitignore")).unwrap();
        std::fs::write(repo.join("$(touch pwned)"), "").unwrap();

        let refused = ignore(&repo, &repo.join("$(touch pwned)"), false)
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("link"), "{refused}");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "export A=1\n");
    });
}
