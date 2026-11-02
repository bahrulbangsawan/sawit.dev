//! The skills tools: what they refuse before anything runs, the way they pick, and how they tell
//! what ran.

use super::*;

fn said(result: &CallToolResult) -> String {
    let texts = result.content.iter().filter_map(|c| c.as_text());
    texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
}

fn sawit() -> Sawit {
    Sawit::new(sawit_core::instances::AppPaths {
        root: std::env::temp_dir(),
    })
}

/// Nothing is read, let alone run, until the agent confirms and names a pack of the catalog
/// and a package manager there is.
#[tokio::test]
async fn installs_and_removals_refuse_before_anything_runs() {
    let sawit = sawit();
    let install = |skill: &str, pm: Option<&str>, confirm: bool| {
        sawit.install_skill_tool(InstallSkillArgs {
            skill: skill.into(),
            agents: vec!["Claude Code".into()],
            method: None,
            package_manager: pm.map(String::from),
            confirm,
        })
    };
    let r = install("superpowers", None, false).await;
    assert_eq!(r.is_error, Some(true));
    assert!(said(&r).contains("confirm=true"), "{}", said(&r));
    let r = install("sawit-palsu", None, true).await;
    assert!(said(&r).contains("Unknown skill pack"), "{}", said(&r));
    let r = install("cf-cli", Some("deno"), true).await;
    assert!(said(&r).contains("isn't a package manager"), "{}", said(&r));
    let r = sawit
        .uninstall_skill_tool(UninstallSkillArgs {
            skill: "superpowers".into(),
            agents: vec!["Claude Code".into()],
            confirm: "yes".into(),
        })
        .await;
    assert_eq!(r.is_error, Some(true));
    assert!(said(&r).contains("confirm=\"superpowers\""), "{}", said(&r));
}

/// The way is the one named when the pack has it, else its first that reaches every agent:
/// a plugin never goes to an agent other than Claude Code.
#[test]
fn the_way_reaches_every_agent_given() {
    let claude = skills::Target {
        label: "Claude Code".into(),
        clients: vec![],
        dir: "/h/.claude/skills".into(),
        claude: Some(skills::ClaudeHome {
            dir: "/h/.claude".into(),
            profile_dir: None,
        }),
        cli_agent: Some("claude-code"),
    };
    let cursor = skills::Target {
        label: "Cursor".into(),
        clients: vec![],
        dir: "/h/.cursor/skills".into(),
        claude: None,
        cli_agent: Some("cursor"),
    };
    let mcp_apps = skills::get("mcp-apps").unwrap();
    assert_eq!(
        method_for(mcp_apps, None, &[&claude]).unwrap().id(),
        "plugin"
    );
    assert_eq!(
        method_for(mcp_apps, None, &[&claude, &cursor])
            .unwrap()
            .id(),
        "skills-cli"
    );
    assert_eq!(
        method_for(mcp_apps, Some("skills-cli"), &[&claude])
            .unwrap()
            .id(),
        "skills-cli"
    );
    assert!(method_for(mcp_apps, Some("copy"), &[&claude]).is_err());
    let superpowers = skills::get("superpowers").unwrap();
    assert!(method_for(superpowers, None, &[&cursor]).is_err());
    // The skill Sawit ships installs built in, into any agent.
    let canvas_design = skills::get("sawit-canvas-design").unwrap();
    for way in [None, Some("built-in")] {
        let method = method_for(canvas_design, way, &[&claude, &cursor]).unwrap();
        assert_eq!(method.id(), "built-in");
    }
    assert!(method_for(canvas_design, Some("copy"), &[&claude]).is_err());
}

/// A run that failed before its shell started (the pack's lock was held, or the skill
/// couldn't be written out) doesn't say its lines ran.
#[test]
fn lines_that_never_started_are_not_run() {
    let lines = ["mkdir -p a".to_string(), "cp -R b a/".to_string()];
    assert_eq!(commands(&lines, true), "Ran:\nmkdir -p a\ncp -R b a/");
    assert_eq!(commands(&lines, false), "Not run:\nmkdir -p a\ncp -R b a/");
}

#[test]
fn args_parse() {
    let a: InstallSkillArgs = serde_json::from_str(
        r#"{"skill":"cf-cli","agents":["codex"],"package_manager":"bun","confirm":true}"#,
    )
    .unwrap();
    assert_eq!(a.package_manager.as_deref(), Some("bun"));
    assert!(a.method.is_none());
    assert!(serde_json::from_str::<UninstallSkillArgs>(r#"{"skill":"x","agents":[]}"#).is_err());
}
