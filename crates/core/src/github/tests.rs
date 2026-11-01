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

#[test]
fn repos_and_comments_parse() {
    let repos = parse_repos(
        &json!([{"full_name": "a/b", "description": "", "private": true,
            "clone_url": "https://github.com/a/b.git", "pushed_at": "1970-01-02T00:00:00Z"}]),
    );
    assert_eq!(repos[0].description, None);
    assert_eq!(repos[0].pushed_at, 86400);
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
