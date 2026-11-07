//! Tests for GitHub: the sign-in's scopes and stores, and reading GitHub's answers.

use serde_json::json;

use super::backup::base64;
use super::http::parse_reply;
use super::token::user_account;
use super::*;

#[test]
fn base64_pads() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"export A=1\n"), "ZXhwb3J0IEE9MQo=");
}

/// The one sign-in asks for every scope a GitHub control needs: `repo` for pull requests,
/// issues, private clones, HTTPS push and the shell files' private repository; `workflow`,
/// without which GitHub refuses a push that changes `.github/workflows`; and the profile.
#[test]
fn the_sign_in_asks_for_what_every_control_needs() {
    assert_eq!(scope_list(), "repo workflow read:user");
    let granted = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(missing_scopes(&granted(&["repo", "workflow", "read:user"])).is_empty());
    assert!(missing_scopes(&granted(&["repo", "workflow", "user"])).is_empty());
    // A sign-in from before Sawit asked for `workflow`.
    assert_eq!(
        missing_scopes(&granted(&["read:user", "repo"])),
        ["workflow"]
    );
    assert_eq!(
        missing_scopes(&[]),
        ["repo", "workflow", "read:user"],
        "nothing granted lacks everything"
    );
}

/// The status and the scopes header come after the body, a line each; a curl too old for
/// `%header{…}` leaves the scopes unknown instead of making some up.
#[test]
fn a_reply_carries_its_status_and_scopes() {
    let r = parse_reply("{\"login\":\"me\"}\nrepo, workflow,read:user\n200");
    assert_eq!(r.code, 200);
    assert_eq!(r.body, "{\"login\":\"me\"}");
    assert_eq!(
        r.scopes.as_deref(),
        Some(&["repo".to_string(), "workflow".into(), "read:user".into()][..])
    );
    let r = parse_reply("line one\nline two\n\n404");
    assert_eq!((r.code, r.body.as_str()), (404, "line one\nline two"));
    assert_eq!(r.scopes, Some(Vec::new()), "an empty header is no scopes");
    let r = parse_reply("{}\n%header{x-oauth-scopes}\n200");
    assert_eq!((r.code, r.scopes), (200, None));
}

#[test]
fn accounts_parse() {
    let a = parse_account(
        &json!({"login": "me", "name": "", "avatar_url": "https://avatars.githubusercontent.com/u/1?v=4"}),
        None,
    );
    assert_eq!(a.login, "me");
    assert_eq!(a.name, None, "an empty name is none");
    assert_eq!(
        a.avatar_url.as_deref(),
        Some("https://avatars.githubusercontent.com/u/1?v=4")
    );
    let a = parse_account(
        &json!({"login": "me", "avatar_url": "file:///etc/passwd"}),
        None,
    );
    assert_eq!(a.avatar_url, None, "only an https picture is fetched");
}

/// The app's sign-in is the `sawit`/`github` Keychain item; a Sawit with a data folder of its
/// own keeps its own, so a scratch or demo copy never shows (nor signs out) the user's.
#[test]
fn a_separate_data_folder_keeps_a_separate_sign_in() {
    assert_eq!(user_account(None), "github");
    assert_eq!(user_account(Some("".as_ref())), "github");
    assert_eq!(
        user_account(Some("/Users/Shared/sawit-demo".as_ref())),
        "github:/Users/Shared/sawit-demo"
    );
}

#[test]
fn a_memory_store_keeps_one_token() {
    let store = MemoryStore::default();
    assert_eq!(store.load().unwrap(), None);
    store.save("t1").unwrap();
    store.save("t2").unwrap();
    assert_eq!(store.load().unwrap().as_deref(), Some("t2"));
    store.delete().unwrap();
    assert_eq!(store.load().unwrap(), None);
}

/// The Keychain store, on an item of the test's own: never the user's sign-in.
#[test]
#[ignore = "writes to the login Keychain"]
fn a_keychain_store_round_trips_on_its_own_item() {
    let store = Keychain::named("sawit.github-test", "probe");
    assert_ne!(store, Keychain::user());
    store.save("gho_test").unwrap();
    assert_eq!(store.load().unwrap().as_deref(), Some("gho_test"));
    store.delete().unwrap();
    assert_eq!(store.load().unwrap(), None);
}

#[test]
fn device_code_and_poll_answers() {
    let d = parse_device_code(&json!({
        "device_code": "dc", "user_code": "ABCD-1234",
        "verification_uri": "https://github.com/login/device", "interval": 5, "expires_in": 899
    }))
    .unwrap();
    assert_eq!(d.user_code, "ABCD-1234");
    assert_eq!(d.expires_in, 899);
    assert_eq!(parse_device_code(&json!({"error": "x"})), None);
    assert_eq!(
        parse_poll(&json!({"access_token": "t"})),
        Poll::Token("t".into())
    );
    assert_eq!(
        parse_poll(&json!({"error": "authorization_pending"})),
        Poll::Pending
    );
    assert_eq!(parse_poll(&json!({"error": "slow_down"})), Poll::SlowDown);
    assert!(matches!(
        parse_poll(&json!({"error": "access_denied"})),
        Poll::Failed(_)
    ));
}

#[test]
fn items_tell_pull_requests_from_issues() {
    let items = parse_items(&json!([
        {"number": 3, "title": "Fix", "user": {"login": "ann"}, "state": "open",
         "pull_request": {"url": "x"}, "comments": 2, "created_at": "2024-01-02T03:04:05Z",
         "updated_at": "2024-01-02T03:04:05Z", "html_url": "u", "body": null},
        {"number": 4, "title": "Bug", "user": {"login": "bo"}, "state": "closed"}
    ]));
    assert_eq!(items.len(), 2);
    assert!(items[0].pull && items[0].open && items[0].body.is_empty());
    assert!(!items[1].pull && !items[1].open);
}

/// A repository as `/user/repos` sends it: its owner, whether it is a fork or archived, its
/// language and stars. Fields GitHub leaves out or sends empty read as none; a repository with
/// no owner object belongs to the name before the slash.
#[test]
fn repos_and_comments_parse() {
    let repos = parse_repos(&json!([
        {"full_name": "acme/site", "owner": {"login": "acme"}, "description": "The site",
            "private": false, "fork": true, "archived": true, "language": "Rust",
            "stargazers_count": 12, "clone_url": "https://github.com/acme/site.git",
            "pushed_at": "1970-01-02T00:00:00Z"},
        {"full_name": "a/b", "description": "", "private": true, "language": null,
            "clone_url": "https://github.com/a/b.git", "pushed_at": null},
        {"full_name": "no/clone-url"}
    ]));
    assert_eq!(
        repos.len(),
        2,
        "a repository without a clone URL is left out"
    );
    assert_eq!(
        repos[0],
        Repo {
            full_name: "acme/site".into(),
            owner: "acme".into(),
            description: Some("The site".into()),
            private: false,
            fork: true,
            archived: true,
            language: Some("Rust".into()),
            stars: 12,
            clone_url: "https://github.com/acme/site.git".into(),
            pushed_at: 86400,
        }
    );
    let b = &repos[1];
    assert_eq!(b.owner, "a");
    assert_eq!(b.description, None);
    assert!(b.private && !b.fork && !b.archived);
    assert_eq!((b.language.as_deref(), b.stars, b.pushed_at), (None, 0, 0));
    let c = parse_comments(
        &json!([{"user": {"login": "ann"}, "body": "hi", "created_at": "2000-03-01T00:00:00Z"}]),
    );
    assert_eq!(c[0].created_at, 951868800);
}

#[test]
fn timeline_events_and_review_comments() {
    let mut events = parse_timeline(&json!([
        {"event": "committed", "sha": "a1f3c2e9", "message": "IG review\n\nbody", "author": {"name": "Tres", "date": "2024-01-01T00:00:00Z"}},
        {"event": "committed", "sha": "7be019d0", "message": "docs", "author": {"name": "Tres", "date": "2024-01-01T01:00:00Z"}},
        {"event": "reviewed", "id": 9, "user": {"login": "kevin"}, "state": "changes_requested", "body": "Add a down step", "submitted_at": "2024-01-02T00:00:00Z"},
        {"event": "commented", "user": {"login": "tres"}, "body": "Done", "created_at": "2024-01-03T00:00:00Z"},
        {"event": "committed", "sha": "5a38d8b0", "message": "Merge", "author": {"name": "Kev", "date": "2024-01-04T00:00:00Z"}},
        {"event": "labeled", "actor": {"login": "ann"}, "label": {"name": "bug"}, "created_at": "2024-01-04T00:00:00Z"},
        {"event": "subscribed", "actor": {"login": "ann"}},
        {"event": "merged", "actor": {"login": "ann"}, "commit_id": "abcdef1234", "created_at": "2024-01-05T00:00:00Z"}
    ]));
    assert_eq!(events.len(), 7);
    attach_review_comments(
        &mut events,
        parse_review_comments(&json!([
            {"pull_request_review_id": 9, "path": "db.sql", "line": 12, "body": "on conflict"},
            {"pull_request_review_id": 9, "path": "a.ts", "line": null, "body": "moved"}
        ])),
    );
    let Event::Reviewed {
        state, comments, ..
    } = &events[2]
    else {
        panic!()
    };
    assert_eq!(*state, ReviewState::ChangesRequested);
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[1].line, None);
    let entries = group(&events);
    assert_eq!(entries.len(), 6);
    let Entry::Pushed {
        author,
        commits,
        time,
    } = &entries[0]
    else {
        panic!()
    };
    assert_eq!(
        (author.as_str(), commits.len(), *time),
        ("Tres", 2, 1704070800)
    );
    assert_eq!(commits[0], ("a1f3c2e".into(), "IG review".into()));
    assert!(matches!(&entries[3], Entry::Pushed { author, .. } if author == "Kev"));
    assert!(
        matches!(&entries[5], Entry::Event(Event::Action { kind: ActionKind::Merged, detail, .. }) if detail == "abcdef1")
    );
}

#[test]
fn times() {
    assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_time("2024-02-29T12:00:00Z"), Some(1709208000));
    assert_eq!(parse_time("nope"), None);
    assert_eq!(parse_time("1970-01-01T07:00:00+07:00"), Some(0));
    assert_eq!(parse_time("1970-01-01T00:00:00-01:30"), Some(5400));
    assert_eq!(ago(0, 30), "just now");
    assert_eq!(ago(0, 120), "2 min ago");
    assert_eq!(ago(0, 3600), "1 hour ago");
    assert_eq!(ago(0, 3 * 86400), "3 days ago");
}

/// Why a sign-in ended is kept with the time, the newest lines only, one reason a line.
#[test]
fn the_session_log_keeps_why_a_sign_in_ended() {
    assert_eq!(super::session_log::stamp(0), "1970-01-01T00:00:00Z");
    assert_eq!(
        super::session_log::stamp(1_793_961_302),
        "2026-11-06T10:35:02Z"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join(SESSION_LOG);
    note_session(&path, "signed out: the user pressed Sign Out");
    note_session(&path, "kept: a 401\nfrom /repos");
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].ends_with("Z signed out: the user pressed Sign Out"));
    assert!(
        lines[1].ends_with("kept: a 401 from /repos"),
        "one line per reason"
    );
    for i in 0..250 {
        note_session(&path, &format!("line {i}"));
    }
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 200);
    assert!(text.lines().last().unwrap().ends_with("line 249"));
}

/// How a file stands is judged against the last sync: a side that still has the synced hash is
/// the one that didn't change. Without a sync from here, files that differ are a conflict.
#[test]
fn sync_states_follow_the_last_sync() {
    use std::collections::BTreeMap;
    let map = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    let local = map(&[
        ("same", "1"),
        ("here", "2"),
        ("there", "1"),
        ("both", "2"),
        ("new", "1"),
        ("never", "1"),
    ]);
    let remote = map(&[
        ("same", "1"),
        ("here", "1"),
        ("there", "2"),
        ("both", "3"),
        ("gone", "1"),
        ("never", "2"),
    ]);
    let base = SyncBase {
        files: map(&[("here", "1"), ("there", "1"), ("both", "1")]),
        ..Default::default()
    };
    let plan: BTreeMap<String, FileState> = plan_sync(&local, &remote, &base).into_iter().collect();
    assert_eq!(plan["same"], FileState::Same);
    assert_eq!(plan["here"], FileState::ChangedHere);
    assert_eq!(plan["there"], FileState::ChangedOnGitHub);
    assert_eq!(plan["both"], FileState::ChangedOnBoth);
    assert_eq!(plan["new"], FileState::OnlyHere);
    assert_eq!(plan["gone"], FileState::OnlyOnGitHub);
    assert_eq!(plan["never"], FileState::ChangedOnBoth);
    assert!(FileState::ChangedHere.uploads() && !FileState::ChangedOnBoth.uploads());
    assert!(FileState::OnlyOnGitHub.downloads() && !FileState::ChangedOnBoth.downloads());
    assert_eq!(
        serde_json::to_value(FileState::ChangedOnGitHub).unwrap(),
        "changed_on_github"
    );
}

/// Deletes since the last sync: a path synced before and now gone on one side only is a delete
/// there (carried over by the next sync), unless the other side changed it since, which is a
/// conflict. A path never synced stays "only here" or "only on GitHub".
#[test]
fn deletes_since_the_last_sync_are_carried_over() {
    use std::collections::BTreeMap;
    let map = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    let local = map(&[
        ("deleted-there", "1"),
        ("changed-here", "2"),
        ("new-here", "1"),
    ]);
    let remote = map(&[
        ("deleted-here", "1"),
        ("changed-there", "2"),
        ("new-there", "1"),
    ]);
    let base = SyncBase {
        files: map(&[
            ("deleted-here", "1"),
            ("deleted-there", "1"),
            ("changed-here", "1"),
            ("changed-there", "1"),
        ]),
        ..Default::default()
    };
    let plan: BTreeMap<String, FileState> = plan_sync(&local, &remote, &base).into_iter().collect();
    assert_eq!(plan["deleted-here"], FileState::DeletedHere);
    assert_eq!(plan["deleted-there"], FileState::DeletedOnGitHub);
    assert_eq!(plan["changed-here"], FileState::ChangedOnBoth);
    assert_eq!(plan["changed-there"], FileState::ChangedOnBoth);
    assert_eq!(plan["new-here"], FileState::OnlyHere);
    assert_eq!(plan["new-there"], FileState::OnlyOnGitHub);
    assert!(FileState::DeletedHere.uploads() && !FileState::DeletedHere.downloads());
    assert!(FileState::DeletedOnGitHub.downloads() && !FileState::DeletedOnGitHub.uploads());
    assert_eq!(
        serde_json::to_value(FileState::DeletedOnGitHub).unwrap(),
        "deleted_on_github"
    );
}

/// Git's blob hash, as GitHub's tree lists it (`git hash-object`), and base64 both ways as
/// GitHub sends a blob (wrapped in lines).
#[test]
fn blob_hashes_and_base64_match_git() {
    assert_eq!(
        blob_sha(b"hello\n"),
        "ce013625030ba8dba906f756967f9e9ca394464a"
    );
    assert_eq!(
        blob_sha(b"export A=2\n"),
        "d9c8f705450d043515699a285086e6915fda4a77"
    );
    for bytes in [
        &b""[..],
        b"f",
        b"fo",
        b"foo",
        b"export A=1\n",
        &[0u8, 255, 128, 7],
    ] {
        assert_eq!(super::backup::base64_decode(&base64(bytes)).unwrap(), bytes);
    }
    assert_eq!(
        super::backup::base64_decode("ZXhw\nb3J0\nIEE9MQo=\n").unwrap(),
        b"export A=1\n"
    );
    assert!(super::backup::base64_decode("Z$==").is_none());
}
