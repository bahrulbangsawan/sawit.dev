//! Agent Accounts' tests: names, the store, the active pointer, and Claude Code's sign-in and
//! plan, from fixtures (Claude Pool's own, with made-up accounts). No test runs `claude` or reads
//! a real account folder.

use std::path::{Path, PathBuf};

use super::claude::{self, AccountDetails};
use super::*;

const SIGNED_IN: &str = include_str!("../fixtures/auth-status-claudeai.json");
const SIGNED_OUT: &str = include_str!("../fixtures/auth-status-signed-out.json");
const FORBIDDEN: &str = include_str!("../fixtures/auth-status-forbidden-fields.json");

mod shell;
mod usage;

pub(super) fn account(name: &str, dir: &Path, is_default: bool) -> Account {
    Account {
        id: new_id(),
        kind: AgentKind::ClaudeCode,
        name: name.into(),
        dir: dir.to_path_buf(),
        is_default,
        color: "#D97757".into(),
        order: 0,
        tracking: false,
        original_status_line: None,
        created_at: 0,
    }
}

#[test]
fn names_are_kebab_case() {
    assert_eq!(kebab("Client — Northwind"), "client-northwind");
    assert_eq!(kebab("My Work"), "my-work");
    assert_eq!(kebab("Café 2"), "cafe-2");
    assert_eq!(kebab("--x--"), "x");
    assert_eq!(kebab(&"a".repeat(40)).len(), MAX_NAME);
    let id = new_id();
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "4");
}

/// The store keeps the user's order and unique names, finds an account by name, number or the
/// start of one name, and the pointer names the active folder (nothing for the default).
#[test]
fn accounts_are_kept_found_and_made_active() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("sawit"), tmp.path().join("home"));
    let mut all = Accounts::default();
    all.add(account("default", &home.join(".claude"), true))
        .unwrap();
    all.add(account("Work", &home.join(".claude-work"), false))
        .unwrap();
    all.add(account("work", &home.join(".claude-w2"), false))
        .unwrap();
    assert!(
        all.add(account("x", &home.join(".claude-work"), false))
            .is_err()
    );
    let names: Vec<&str> = all
        .of(AgentKind::ClaudeCode)
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(names, ["default", "work", "work-2"]);
    assert_eq!(all.find(AgentKind::ClaudeCode, "2").unwrap().name, "work");
    assert_eq!(
        all.find(AgentKind::ClaudeCode, "DEF").unwrap().name,
        "default"
    );
    assert!(
        all.find(AgentKind::ClaudeCode, "wo").is_none(),
        "two start with it"
    );
    all.save(&root).unwrap();
    let mode = std::fs::metadata(Accounts::path(&root))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(Accounts::load(&root).unwrap(), all);

    assert_eq!(
        all.active(AgentKind::ClaudeCode, &root).unwrap().name,
        "default"
    );
    let work = all.find(AgentKind::ClaudeCode, "work").unwrap().clone();
    write_active(&root, &work).unwrap();
    assert_eq!(
        read_active(&root, AgentKind::ClaudeCode),
        Some(home.join(".claude-work"))
    );
    assert_eq!(
        all.active(AgentKind::ClaudeCode, &root).unwrap().name,
        "work"
    );
    let default = all.find(AgentKind::ClaudeCode, "default").unwrap().clone();
    write_active(&root, &default).unwrap();
    assert_eq!(
        std::fs::read_to_string(active_path(&root, AgentKind::ClaudeCode)).unwrap(),
        ""
    );

    assert_eq!(
        all.rename(&work.id, "Side Project").unwrap(),
        "side-project"
    );
    let gone = all.remove(&work.id).unwrap();
    assert_eq!(gone.dir, home.join(".claude-work"));
    let orders: Vec<usize> = all
        .of(AgentKind::ClaudeCode)
        .iter()
        .map(|a| a.order)
        .collect();
    assert_eq!(orders, [0, 1]);

    std::fs::write(Accounts::path(&root), "{broken").unwrap();
    assert!(
        Accounts::load(&root).is_err(),
        "a broken file is never saved over"
    );
}

#[test]
fn account_folders_are_discovered_without_claude_pool() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    for d in [".claude", ".claude-2", ".claude-work", ".claude-pool"] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    std::fs::write(home.join(".claude-notes"), "a file").unwrap();
    let found: Vec<PathBuf> = discover(AgentKind::ClaudeCode, home);
    let names: Vec<String> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, [".claude", ".claude-2", ".claude-work"]);
}

/// Only whitelisted fields are read, and a secret-looking one never, even if a future Claude
/// Code puts it in the output.
#[test]
fn sign_in_status_reads_only_the_whitelist() {
    let s = claude::parse_auth_status(SIGNED_IN, 0).unwrap();
    assert!(s.signed_in && s.reports_usage());
    assert_eq!(s.email.as_deref(), Some("person@example.com"));
    assert_eq!(
        s.identity().as_deref(),
        Some("person@example.com · Example Org")
    );
    assert_eq!(s.subscription_type.as_deref(), Some("max"));
    let out = claude::parse_auth_status(SIGNED_OUT, 1).unwrap();
    assert!(!out.signed_in);
    assert!(
        claude::parse_auth_status(SIGNED_IN, 2).is_none(),
        "unknown exit"
    );
    let fields = claude::allowed_fields(FORBIDDEN);
    let all: String = fields.values().cloned().collect();
    assert!(!all.contains("SHOULD-NEVER-APPEAR"));
    assert_eq!(
        fields.keys().map(String::as_str).collect::<Vec<_>>(),
        ["authMethod", "email", "loggedIn", "orgName"]
    );
    let api = claude::parse_auth_status(FORBIDDEN, 0).unwrap();
    assert!(!api.reports_usage());
    assert_eq!(
        s.same_account(&api),
        Some(true),
        "same email, one org unknown"
    );
}

/// The plan: the cached name when there is one, "Max 20x" only when the tier says so, a seat for
/// Team, and nothing from a cache of another email.
#[test]
fn plans_are_never_guessed() {
    let details = |json: &str| claude::parse_details(json).unwrap();
    let max20 = details(
        r#"{"projects": {"secret": 1}, "primaryApiKey": "x", "oauthAccount": {"emailAddress": "a@x.com", "organizationRateLimitTier": "default_claude_max_20x", "organizationType": "claude_max"}}"#,
    );
    assert_eq!(
        claude::plan(Some("max"), Some(&max20), Some("A@x.com")).as_deref(),
        Some("Max 20x")
    );
    assert_eq!(
        claude::plan(Some("max"), None, None).as_deref(),
        Some("Max · Unknown Tier")
    );
    assert_eq!(
        claude::plan(Some("max"), Some(&max20), Some("b@x.com")).as_deref(),
        Some("Max · Unknown Tier"),
        "another email's cache is ignored"
    );
    let team = AccountDetails {
        seat_tier: Some("premium".into()),
        ..Default::default()
    };
    assert_eq!(
        claude::plan(Some("team"), Some(&team), None).as_deref(),
        Some("Team · Premium seat")
    );
    assert_eq!(
        claude::plan(Some("pro"), None, None).as_deref(),
        Some("Pro")
    );
    let named = details(r#"{"oauthAccount": {"planDisplayName": "Max 5x"}}"#);
    assert_eq!(
        claude::plan(Some("max"), Some(&named), None).as_deref(),
        Some("Max 5x")
    );
    assert_eq!(claude::plan(None, None, None), None);
    assert!(claude::parse_details(r#"{"other": 1}"#).is_none());
}

/// The default account runs with `CLAUDE_CONFIG_DIR` unset, never set to `~/.claude`.
#[test]
fn the_default_account_runs_with_the_variable_unset() {
    let home = Path::new("/Users/me");
    let default = account("default", &home.join(".claude"), true);
    let env = claude::environment(&default, home, "me", "/usr/bin:/bin");
    assert!(env.iter().all(|(k, _)| k != "CLAUDE_CONFIG_DIR"));
    assert!(env.iter().any(|(k, v)| k == "LOGNAME" && v == "me"));
    let work = account("work", &home.join(".claude-work"), false);
    let env = claude::environment(&work, home, "me", "/usr/bin");
    assert!(env.contains(&("CLAUDE_CONFIG_DIR".into(), "/Users/me/.claude-work".into())));
    assert_eq!(
        claude::global_config(&default, home),
        home.join(".claude.json")
    );
    assert_eq!(
        claude::global_config(&work, home),
        home.join(".claude-work/.claude.json")
    );
}
