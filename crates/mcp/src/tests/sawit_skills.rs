//! What the Sawit pack's skills and the block tell agents names only tools this server has, and
//! only arguments those tools take; `install_sawit_skills` refuses until the agent confirms.

use std::collections::HashSet;

use sawit_core::skills::{self, SAWIT_PACK, bundled};

use crate::Sawit;
use crate::sawit_skills::InstallSawitSkillsArgs;

/// Every word in `text` written like a tool or argument name: lowercase, with an underscore
/// (`browser_snapshot`, `project_dir`), a family as `browser_*`, or after a client's prefix
/// (`mcp__sawit__list_instances`).
fn names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '*')) {
        let word = word.strip_prefix("mcp__sawit__").unwrap_or(word);
        let snake = word.contains('_')
            && !word.starts_with('_')
            && word
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '*');
        if snake {
            out.push(word.to_string());
        }
    }
    out
}

#[test]
fn the_skills_and_the_block_name_only_real_tools_and_arguments() {
    let tools = Sawit::router().list_all();
    let tool_names: HashSet<String> = tools.iter().map(|t| t.name.to_string()).collect();
    let arguments: HashSet<String> = tools
        .iter()
        .filter_map(|t| t.input_schema.get("properties")?.as_object())
        .flat_map(|props| props.keys().cloned())
        .collect();
    let pack = skills::get(SAWIT_PACK).unwrap();
    let mut texts: Vec<(String, String)> = pack
        .skills
        .iter()
        .map(|s| {
            (
                s.to_string(),
                bundled::text(s, "SKILL.md").unwrap().to_string(),
            )
        })
        .collect();
    texts.push(("the block".into(), sawit_core::sawit_skills::block_text()));
    let mut checked = 0;
    for (name, text) in &texts {
        for word in names(text) {
            // Programs a skill names beside the tools.
            if ["pg_dump"].contains(&word.as_str()) {
                continue;
            }
            let known = match word.strip_suffix('*') {
                Some(family) => tool_names.iter().any(|t| t.starts_with(family)),
                None => tool_names.contains(&word) || arguments.contains(&word),
            };
            assert!(
                known,
                "{name} names `{word}`, which sawit-mcp has no tool or argument of"
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} names checked");
    // The tools each skill is about are named in it, as tools.
    for (skill, must) in [
        (
            "sawit-databases",
            &[
                "create_instance",
                "start_instance",
                "query",
                "execute_sql",
                "write_env",
                "detect_databases",
            ][..],
        ),
        (
            "sawit-browser-testing",
            &[
                "browser_navigate",
                "browser_snapshot",
                "browser_click",
                "browser_type",
                "browser_screenshot",
                "browser_console",
                "browser_network",
                "browser_resize",
                "browser_annotations",
            ],
        ),
        (
            "sawit-canvas",
            &[
                "canvas_design_guide",
                "canvas_add_diagram",
                "canvas_begin",
                "canvas_progress",
            ],
        ),
        (
            "sawit-terminal",
            &[
                "terminal_run",
                "app_run_script",
                "list_ports",
                "stop_processes",
                "ssh_exec",
            ],
        ),
    ] {
        let text = bundled::text(skill, "SKILL.md").unwrap();
        for tool in must {
            assert!(tool_names.contains(*tool), "{tool}");
            assert!(
                text.contains(&format!("`{tool}`")),
                "{skill} doesn't name `{tool}`"
            );
        }
    }
    // Each says what to do without the server.
    for skill in pack.skills {
        let text = bundled::text(skill, "SKILL.md").unwrap();
        assert!(
            text.contains("MCP server isn't connected")
                || text.contains("Without the sawit MCP server"),
            "{skill}"
        );
    }
}

#[tokio::test]
async fn install_sawit_skills_waits_for_confirm() {
    let dir = tempfile::tempdir().unwrap();
    let sawit = Sawit::new(sawit_core::instances::AppPaths {
        root: dir.path().to_path_buf(),
    });
    let args = |confirm, scope: Option<&str>| InstallSawitSkillsArgs {
        scope: scope.map(String::from),
        project: Some(dir.path().display().to_string()),
        remove: None,
        confirm,
    };
    let said = |r: &rmcp::model::CallToolResult| {
        r.content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let r = sawit.install_sawit_skills_tool(args(false, None)).await;
    assert_eq!(r.is_error, Some(true));
    assert!(said(&r).contains("confirm=true"), "{}", said(&r));
    let r = sawit
        .install_sawit_skills_tool(args(true, Some("everywhere")))
        .await;
    assert!(
        said(&r).contains("\"global\" or \"project\""),
        "{}",
        said(&r)
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    // A project: its AGENTS.md, made with the block alone, and taken out again.
    let r = sawit
        .install_sawit_skills_tool(args(true, Some("project")))
        .await;
    assert_ne!(r.is_error, Some(true), "{}", said(&r));
    let agents = dir.path().join("AGENTS.md");
    assert_eq!(
        std::fs::read_to_string(&agents).unwrap(),
        sawit_core::sawit_skills::block_text()
    );
    assert!(said(&r).contains("AGENTS.md (created)"), "{}", said(&r));
    let r = sawit
        .install_sawit_skills_tool(InstallSawitSkillsArgs {
            remove: Some(true),
            ..args(true, Some("project"))
        })
        .await;
    assert_ne!(r.is_error, Some(true), "{}", said(&r));
    assert!(!agents.exists(), "{}", said(&r));
}
