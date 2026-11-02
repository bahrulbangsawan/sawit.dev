//! What the footer's button says: Install Skills while no agent has any of it, Skills Installed
//! when an install would change nothing it can, else Update Skills.

use std::path::PathBuf;

use super::*;
use crate::sawit_skills::agents::AgentFile;
use crate::sawit_skills::files::Holds;
use crate::sawit_skills::state::level_for;
use crate::skills::Status;

fn skills(statuses: &[Option<Status>]) -> Vec<(String, PathBuf, Option<Status>)> {
    statuses
        .iter()
        .map(|s| ("Agent".to_string(), PathBuf::from("/x"), *s))
        .collect()
}

fn files(holds: &[Holds]) -> Vec<(AgentFile, Holds)> {
    holds
        .iter()
        .map(|h| {
            let file = AgentFile {
                label: "Agent".into(),
                path: PathBuf::from("/x/AGENTS.md"),
                also: Vec::new(),
            };
            (file, h.clone())
        })
        .collect()
}

#[test]
fn the_button_reads_what_an_install_would_do() {
    let installed = Some(Status::Installed { plugin: false });
    let none: Vec<String> = Vec::new();
    // Nothing anywhere, or only folders that aren't Sawit's.
    assert_eq!(
        level_for(
            &skills(&[Some(Status::Missing), None]),
            &files(&[Holds::Nothing]),
            &none
        ),
        Level::NotInstalled
    );
    // Everything current; a newer block, a damaged file and a folder Sawit leaves alone don't
    // ask for an update a click can't make.
    let current = files(&[
        Holds::Current,
        Holds::Newer(VERSION + 1),
        Holds::Damaged("x".into()),
    ]);
    assert_eq!(
        level_for(&skills(&[installed, None]), &current, &none),
        Level::Installed
    );
    // This Sawit ships a newer version: of the skills, or of the block.
    assert_eq!(
        level_for(
            &skills(&[Some(Status::Differs)]),
            &files(&[Holds::Current]),
            &none
        ),
        Level::Update
    );
    assert_eq!(
        level_for(
            &skills(&[installed]),
            &files(&[Holds::Older(Some(0))]),
            &none
        ),
        Level::Update
    );
    // An agent found since has none of it, or a client lacks the MCP server.
    assert_eq!(
        level_for(
            &skills(&[installed, Some(Status::Missing)]),
            &files(&[Holds::Current, Holds::Nothing]),
            &none
        ),
        Level::Update
    );
    assert_eq!(
        level_for(
            &skills(&[installed]),
            &files(&[Holds::Current]),
            &["Cursor".into()]
        ),
        Level::Update
    );
}

/// What a run says when it changed nothing: already current, nothing to remove, or no agent to
/// install into.
#[test]
fn a_run_that_changes_nothing_says_why() {
    let nothing = Outcome::default();
    assert!(nothing.summary(false).starts_with("No coding agent"));
    assert!(nothing.summary(true).starts_with("Nothing of Sawit Skills"));
    let current = Outcome {
        mcp: vec![McpOutcome {
            label: "Cursor".into(),
            config: PathBuf::from("/x/mcp.json"),
            result: Ok(McpDone::Present),
        }],
        ..Outcome::default()
    };
    assert_eq!(
        current.summary(false),
        "Sawit Skills are already current everywhere."
    );
    assert_eq!(
        current.lines(std::path::Path::new("/x")),
        [
            "Sawit's MCP server:",
            "  · Cursor: ~/mcp.json (already there)",
        ]
    );
}

/// Every path a result gives is written from the home folder as `~/…`: the files changed, and
/// where a file's first copy went in Sawit's data folder.
#[test]
fn a_result_writes_paths_from_home() {
    use crate::sawit_skills::files::{Change, Done};
    let home = std::path::Path::new("/Users/me");
    let data = home.join("Library/Application Support/sawit");
    let outcome = Outcome {
        files: vec![FileOutcome {
            label: "Claude Code".into(),
            path: home.join(".claude/CLAUDE.md"),
            result: Ok(Change {
                done: Done::Added,
                backup: Some(data.join("instruction-backups/home/.claude/CLAUDE.md")),
            }),
        }],
        mcp: vec![McpOutcome {
            label: "Cursor".into(),
            config: home.join(".cursor/mcp.json"),
            result: Ok(McpDone::Removed),
        }],
        ..Outcome::default()
    };
    assert_eq!(
        outcome.lines(home),
        [
            "Instruction files:",
            "  ✓ Claude Code: ~/.claude/CLAUDE.md (added; its first copy: ~/Library/Application Support/sawit/instruction-backups/home/.claude/CLAUDE.md)",
            "Sawit's MCP server:",
            "  ✓ Cursor: ~/.cursor/mcp.json (removed: Install Skills had added it)",
        ]
    );
}
