//! Permission rules: read, added, removed and moved without changing anything else in the file,
//! saved with a backup, secrets refused; Sawit's tools and what the rules say of them; the other
//! agents' settings.

use std::path::PathBuf;

use super::*;

const SETTINGS: &str = r#"{
    "model": "opus",
    "permissions": {
        "allow": [
            "Bash(npm run test:*)",
            "Read(~/notes/**)"
        ],
        "deny": ["WebFetch"],
        "defaultMode": "acceptEdits"
    },
    "env": {"FOO": "1"}
}
"#;

fn add(rule: &str, to: Bucket) -> Change {
    Change::Add {
        rule: rule.into(),
        to,
    }
}

#[test]
fn rules_are_read_bucket_by_bucket() {
    let rules = read_rules(SETTINGS).unwrap();
    assert_eq!(rules.allow, ["Bash(npm run test:*)", "Read(~/notes/**)"]);
    assert!(rules.ask.is_empty());
    assert_eq!(rules.deny, ["WebFetch"]);
    assert_eq!(rules.bucket_of("WebFetch"), Some(Bucket::Deny));
    assert_eq!(read_rules("").unwrap(), Rules::default());
    assert_eq!(read_rules("{}").unwrap(), Rules::default());
    assert!(read_rules("{ nope").is_err());
    assert!(read_rules(r#"{"permissions": {"allow": "Bash"}}"#).is_err());
}

/// Adding splices one line in, as the file writes its lists: indentation, the one-line list,
/// key order, the unknown keys all stay.
#[test]
fn an_added_rule_changes_only_its_line() {
    let after = apply(SETTINGS, &add("Bash(git status)", Bucket::Allow)).unwrap();
    assert_eq!(
        after,
        SETTINGS.replace(
            "            \"Read(~/notes/**)\"\n",
            "            \"Read(~/notes/**)\",\n            \"Bash(git status)\"\n"
        )
    );
    // The list on one line stays on one line.
    let after = apply(SETTINGS, &add("Bash(rm:*)", Bucket::Deny)).unwrap();
    assert!(
        after.contains(r#""deny": ["WebFetch", "Bash(rm:*)"],"#),
        "{after}"
    );
    // A bucket not there yet is added after the others, in the file's indentation.
    let after = apply(SETTINGS, &add("mcp__sawit__execute_sql", Bucket::Ask)).unwrap();
    assert!(
        after.contains(
            "        \"defaultMode\": \"acceptEdits\",\n        \"ask\": [\n            \"mcp__sawit__execute_sql\"\n        ]\n    },"
        ),
        "{after}"
    );
    // The rest of the file reads the same.
    let v: serde_json::Value = serde_json::from_str(&after).unwrap();
    assert_eq!(v["env"]["FOO"], "1");
    assert_eq!(v["permissions"]["defaultMode"], "acceptEdits");
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["model", "permissions", "env"]);
}

#[test]
fn a_file_without_permissions_or_a_new_file_gets_them() {
    let after = apply("{\n  \"model\": \"opus\"\n}\n", &add("Read", Bucket::Allow)).unwrap();
    assert_eq!(
        after,
        "{\n  \"model\": \"opus\",\n  \"permissions\": {\n    \"allow\": [\n      \"Read\"\n    ]\n  }\n}\n"
    );
    let new = apply("", &add("Bash(ls)", Bucket::Deny)).unwrap();
    assert_eq!(read_rules(&new).unwrap().deny, ["Bash(ls)"]);
    let empty = apply("{}", &add("Read", Bucket::Ask)).unwrap();
    assert_eq!(read_rules(&empty).unwrap().ask, ["Read"]);
}

#[test]
fn removing_and_moving_keep_the_rest() {
    let after = apply(
        SETTINGS,
        &Change::Remove {
            rule: "Bash(npm run test:*)".into(),
        },
    )
    .unwrap();
    assert_eq!(
        after,
        SETTINGS.replace("            \"Bash(npm run test:*)\",\n", "")
    );
    // The last one of a list, and the only one.
    let after = apply(
        SETTINGS,
        &Change::Remove {
            rule: "Read(~/notes/**)".into(),
        },
    )
    .unwrap();
    assert_eq!(read_rules(&after).unwrap().allow, ["Bash(npm run test:*)"]);
    assert!(
        after.contains("\"Bash(npm run test:*)\"\n        ],"),
        "{after}"
    );
    let after = apply(
        SETTINGS,
        &Change::Remove {
            rule: "WebFetch".into(),
        },
    )
    .unwrap();
    assert!(after.contains("\"deny\": [],"), "{after}");
    // Moving: out of allow, into deny.
    let moved = apply(
        SETTINGS,
        &Change::Move {
            rule: "Read(~/notes/**)".into(),
            to: Bucket::Deny,
        },
    )
    .unwrap();
    let rules = read_rules(&moved).unwrap();
    assert_eq!(rules.allow, ["Bash(npm run test:*)"]);
    assert_eq!(rules.deny, ["WebFetch", "Read(~/notes/**)"]);
    // Refusals.
    assert!(
        apply(
            SETTINGS,
            &Change::Remove {
                rule: "Nope".into()
            }
        )
        .is_err()
    );
    assert!(
        apply(SETTINGS, &add("WebFetch", Bucket::Allow))
            .unwrap_err()
            .contains("already in Deny")
    );
    assert!(
        apply(
            SETTINGS,
            &Change::Move {
                rule: "WebFetch".into(),
                to: Bucket::Deny
            }
        )
        .is_err()
    );
}

/// Comments and a trailing comma (as some editors leave them) survive an edit.
#[test]
fn comments_survive() {
    let text =
        "{\n  // mine\n  \"permissions\": {\n    \"allow\": [\"Read\",], /* keep */\n  },\n}\n";
    let after = apply(text, &add("Grep", Bucket::Allow)).unwrap();
    assert!(
        after.contains("// mine") && after.contains("/* keep */"),
        "{after}"
    );
    assert_eq!(read_rules(&after).unwrap().allow, ["Read", "Grep"]);
}

#[test]
fn secrets_and_non_rules_are_refused() {
    for bad in [
        "",
        "Bash(curl -H 'Authorization: Bearer ghp_abcdefghijklmnopqrstuvwxyz0123456789')",
        "Bash(export OPENAI_API_KEY=sk-proj-abc)",
        "Bash(echo 9f8e7d6c5b4a39281706f5e4d3c2b1a0aa11bb22)",
        "two\nlines",
        "(Bash)",
        "Bash(unclosed",
    ] {
        assert!(check_rule(bad).is_err(), "{bad:?}");
        assert!(
            apply(SETTINGS, &add(bad, Bucket::Allow)).is_err(),
            "{bad:?}"
        );
    }
    for good in [
        "Bash",
        "Bash(npm run test:*)",
        "Read(~/.zshrc)",
        "mcp__sawit__execute_sql",
        "mcp__sawit__*",
        "WebFetch(domain:docs.anthropic.com)",
    ] {
        assert_eq!(check_rule(good), Ok(()), "{good}");
    }
}

/// A plan shows its diff; saving backs the file up first and refuses a file changed since.
#[test]
fn a_plan_is_saved_with_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".claude/settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, SETTINGS).unwrap();
    let plan = plan(&path, &add("Bash(git status)", Bucket::Allow)).unwrap();
    assert!(
        plan.diff.contains("+            \"Bash(git status)\""),
        "{}",
        plan.diff
    );
    assert!(
        plan.diff.contains("-            \"Read(~/notes/**)\""),
        "{}",
        plan.diff
    );
    assert!(plan.diff.starts_with("--- "));
    save(&plan).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), plan.after);
    assert_eq!(
        std::fs::read_to_string(backup_path(&path)).unwrap(),
        SETTINGS
    );
    // Changed by someone else since: refused, nothing written.
    let stale = plan.clone();
    std::fs::write(&path, "{}\n").unwrap();
    assert!(save(&stale).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}\n");
    // A project file that isn't there yet is created, with its folder.
    let new = dir.path().join("p/.claude/settings.local.json");
    let plan = super::plan(&new, &add("Read", Bucket::Allow)).unwrap();
    save(&plan).unwrap();
    assert_eq!(
        read_rules(&std::fs::read_to_string(&new).unwrap())
            .unwrap()
            .allow,
        ["Read"]
    );
    assert!(!backup_path(&new).exists());
}

#[test]
fn claude_files_come_in_claude_codes_order() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let project = home.join("app");
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::write(project.join(".claude/settings.json"), SETTINGS).unwrap();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/settings.json"), "{ broken").unwrap();
    let profiles = vec![
        (None, home.join(".claude")),
        (Some("work".to_string()), home.join(".claude-work")),
    ];
    let files = claude_files(
        &profiles,
        std::slice::from_ref(&project),
        &home.join("no-managed.json"),
    );
    let labels: Vec<String> = files.iter().map(|f| f.scope.label()).collect();
    assert_eq!(
        labels,
        ["Project, only you", "Project", "User", "User · work"]
    );
    assert!(!files[0].exists && files[0].editable);
    assert_eq!(files[1].rules.deny, ["WebFetch"]);
    // A file that isn't JSON is shown, not edited.
    assert!(files[2].problem.is_some() && !files[2].editable);
    assert_eq!(files[3].path, home.join(".claude-work/settings.json"));
    assert_eq!(files[3].scope.id(), "user:work");
}

#[test]
fn sawits_confirm_tools_and_what_the_rules_say() {
    assert!(CONFIRM_TOOLS.iter().any(|(t, _)| *t == "execute_sql"));
    assert!(sawit_tools::matches(
        "mcp__sawit__execute_sql",
        "execute_sql"
    ));
    assert!(sawit_tools::matches("mcp__sawit", "execute_sql"));
    assert!(sawit_tools::matches("mcp__sawit__*", "execute_sql"));
    assert!(sawit_tools::matches("mcp__sawit__execute_*", "execute_sql"));
    assert!(!sawit_tools::matches("mcp__sawit__query", "execute_sql"));
    assert!(!sawit_tools::matches("mcp__sawitx__*", "execute_sql"));
    let file = |scope: Scope, allow: &[&str], deny: &[&str]| RulesFile {
        scope,
        path: PathBuf::from("/x"),
        exists: true,
        editable: true,
        rules: Rules {
            allow: allow.iter().map(|s| s.to_string()).collect(),
            ask: vec![],
            deny: deny.iter().map(|s| s.to_string()).collect(),
        },
        problem: None,
    };
    let files = [
        file(
            Scope::Project("/p".into()),
            &[],
            &["mcp__sawit__drop_database"],
        ),
        file(Scope::User(None), &["mcp__sawit__*"], &[]),
    ];
    let states = sawit_tool_states(&files);
    let state = |t: &str| states.iter().find(|s| s.tool == t).unwrap().rule.clone();
    // Deny wins over allow, wherever it is.
    assert_eq!(
        state("drop_database"),
        Some((
            Bucket::Deny,
            "mcp__sawit__drop_database".into(),
            "Project".into()
        ))
    );
    assert_eq!(
        state("execute_sql"),
        Some((Bucket::Allow, "mcp__sawit__*".into(), "User".into()))
    );
    assert_eq!(sawit_tool_states(&[])[0].rule, None);
}

#[test]
fn other_agents_settings_are_read() {
    assert_eq!(
        others::codex(
            "approval_policy = \"on-request\"\nsandbox_mode = \"workspace-write\"\n[profiles.fast]\napproval_policy = \"never\"\n"
        ),
        [
            "approval_policy = on-request",
            "sandbox_mode = workspace-write",
            "profiles.fast.approval_policy = never"
        ]
    );
    assert_eq!(
        others::gemini(
            r#"{"tools": {"allowed": ["run_shell_command(git status)"], "exclude": ["web_fetch"]}}"#
        ),
        [
            "tools.allowed: run_shell_command(git status)",
            "tools.exclude: web_fetch"
        ]
    );
    assert_eq!(
        others::opencode(
            "// c\n{\"permission\": {\"edit\": \"ask\", \"bash\": {\"git *\": \"allow\"}}}"
        ),
        ["edit: ask", "bash git *: allow"]
    );
    assert!(others::codex("model = \"o3\"").is_empty());
}

#[test]
fn diffs_show_the_change_with_context() {
    let d = unified("a\nb\nc\n", "a\nb\nx\nc\n", "f");
    assert_eq!(d, "--- f\n+++ f\n@@ -1,3 +1,4 @@\n a\n b\n+x\n c\n");
    assert_eq!(unified("a\n", "a\n", "f"), "");
    // A changed line: the old one first.
    assert_eq!(
        unified("{\n\"a\": [1]\n}\n", "{\n\"a\": [1, 2]\n}\n", "f"),
        "--- f\n+++ f\n@@ -1,3 +1,3 @@\n {\n-\"a\": [1]\n+\"a\": [1, 2]\n }\n"
    );
}
