//! `scripts/release.sh`: it cuts a release off main only, stops before changing anything when
//! its tag exists, and writes notes that list what users get.

use crate::repo::Repo;
use crate::{ok, stderr};

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
