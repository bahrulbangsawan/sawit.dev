//! `.githooks/pre-push`: a push of main releases it, from a checkout of main only, and the
//! release commit and its tag follow the push to origin.

use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use crate::repo::Repo;
use crate::{LANDS_WITHIN, ok, stderr};

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
