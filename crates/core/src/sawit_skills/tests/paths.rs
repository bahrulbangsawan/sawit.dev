//! Each agent's own instruction file, as its docs give it, for the agents found: Claude Code per
//! profile, Codex (its override when that's the one it reads), the agents that share a file, and
//! the ones without a file of their own, each with its reason.

use std::path::{Path, PathBuf};

use crate::mcp_clients::{Client, Target};
use crate::sawit_skills::agents::{AgentFile, agent_files};

fn target(client: Client, profile_dir: Option<&Path>) -> Target {
    Target {
        client,
        profile: profile_dir
            .and_then(|d| d.file_name())
            .map(|n| {
                n.to_string_lossy()
                    .trim_start_matches(".claude-")
                    .to_string()
            })
            .unwrap_or_else(|| "default".into()),
        config: PathBuf::from("/x"),
        profile_dir: profile_dir.map(Path::to_path_buf),
    }
}

fn paths(files: &[AgentFile], home: &Path) -> Vec<(String, String)> {
    files
        .iter()
        .map(|f| {
            let rel = f.path.strip_prefix(home).unwrap().display().to_string();
            (f.label.clone(), rel)
        })
        .collect()
}

#[test]
fn every_agent_gets_its_own_documented_file() {
    let home = Path::new("/Users/me");
    let work = home.join(".claude-work");
    let clients: Vec<Target> = [
        (Client::ClaudeCode, None),
        (Client::ClaudeCode, Some(work.as_path())),
        (Client::Codex, None),
        (Client::Gemini, None),
        (Client::Antigravity, None),
        (Client::Qwen, None),
        (Client::Amp, None),
        (Client::CopilotCli, None),
        (Client::FactoryDroid, None),
        (Client::Goose, None),
        (Client::Zed, None),
        (Client::Junie, None),
        (Client::Crush, None),
        (Client::Cline, None),
        (Client::Kiro, None),
    ]
    .into_iter()
    .map(|(c, p)| target(c, p))
    .collect();
    let (files, skipped) = agent_files(home, &clients);
    let expected = [
        ("Claude Code", ".claude/CLAUDE.md"),
        ("Claude Code · work", ".claude-work/CLAUDE.md"),
        ("Codex", ".codex/AGENTS.md"),
        ("Gemini CLI, Antigravity", ".gemini/GEMINI.md"),
        ("Qwen Code", ".qwen/QWEN.md"),
        ("Amp", ".config/amp/AGENTS.md"),
        ("GitHub Copilot CLI", ".copilot/copilot-instructions.md"),
        ("Factory Droid", ".factory/AGENTS.md"),
        ("Goose", ".config/goose/.goosehints"),
        ("Zed", ".config/zed/AGENTS.md"),
        ("JetBrains Junie", ".junie/AGENTS.md"),
        ("Crush", ".config/crush/CRUSH.md"),
        ("Cline", ".agents/AGENTS.md"),
        ("Kiro", ".kiro/steering/AGENTS.md"),
    ]
    .map(|(l, p)| (l.to_string(), p.to_string()));
    assert_eq!(paths(&files, home), expected);
    assert!(skipped.is_empty(), "{skipped:?}");
}

/// opencode and Kilo Code read Claude Code's CLAUDE.md while they have no AGENTS.md of their own:
/// making theirs would hide it, so they share Claude Code's, and a removal looks in theirs too.
/// Once theirs is there, it is theirs that gets the block.
#[test]
fn opencode_shares_claude_codes_file_until_it_has_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let clients = [Client::ClaudeCode, Client::OpenCode, Client::KiloCode].map(|c| target(c, None));
    let (files, _) = agent_files(home, &clients);
    assert_eq!(
        paths(&files, home),
        [(
            "Claude Code, opencode, Kilo Code".to_string(),
            ".claude/CLAUDE.md".to_string()
        )]
    );
    assert_eq!(
        files[0].also,
        [
            home.join(".config/opencode/AGENTS.md"),
            home.join(".config/kilo/AGENTS.md")
        ]
    );

    let own = home.join(".config/opencode/AGENTS.md");
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(&own, "mine\n").unwrap();
    let (files, _) = agent_files(home, &clients[1..2]);
    assert_eq!(files[0].path, own);
    // Without Claude Code or its CLAUDE.md, opencode's own file is made.
    let (files, _) = agent_files(home, &clients[2..]);
    assert_eq!(files[0].path, home.join(".config/kilo/AGENTS.md"));
}

/// Codex reads AGENTS.override.md instead of AGENTS.md when it isn't empty: the block goes there,
/// and a removal takes it out of AGENTS.md too.
#[test]
fn codex_uses_its_override_when_it_reads_it() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let codex = home.join(".codex");
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::write(codex.join("AGENTS.override.md"), "").unwrap();
    let (files, _) = agent_files(home, &[target(Client::Codex, None)]);
    assert_eq!(files[0].path, codex.join("AGENTS.md"));
    std::fs::write(codex.join("AGENTS.override.md"), "rules\n").unwrap();
    let (files, _) = agent_files(home, &[target(Client::Codex, None)]);
    assert_eq!(files[0].path, codex.join("AGENTS.override.md"));
    assert_eq!(files[0].also, [codex.join("AGENTS.md")]);
}

/// Agents whose instructions aren't one file are named with the reason; Windsurf (now Devin)
/// gets the block only in a global rules file that is there.
#[test]
fn agents_without_a_file_are_named_with_why() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let clients: Vec<Target> = [
        Client::Cursor,
        Client::ClaudeDesktop,
        Client::VsCode,
        Client::RooCode,
        Client::Warp,
        Client::Windsurf,
        Client::Windsurf,
    ]
    .map(|c| target(c, None))
    .to_vec();
    let (files, skipped) = agent_files(home, &clients);
    assert!(files.is_empty());
    let labels: Vec<&str> = skipped.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Cursor",
            "Claude Desktop",
            "VS Code",
            "Roo Code",
            "Warp",
            "Windsurf / Devin"
        ]
    );
    assert!(skipped[0].why.contains("Cursor's settings"));
    assert!(skipped[5].why.contains(".devin/global_rules.md"));

    let rules = home.join(".devin/global_rules.md");
    std::fs::create_dir_all(rules.parent().unwrap()).unwrap();
    std::fs::write(&rules, "").unwrap();
    let (files, skipped) = agent_files(home, &clients[5..]);
    assert_eq!(files[0].path, rules);
    assert!(skipped.is_empty());
}

/// A profile whose CLAUDE.md is a link to the default profile's shares that one file.
#[test]
fn a_linked_profile_file_is_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let (main, work) = (home.join(".claude"), home.join(".claude-work"));
    std::fs::create_dir_all(&main).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(main.join("CLAUDE.md"), "mine\n").unwrap();
    std::os::unix::fs::symlink(main.join("CLAUDE.md"), work.join("CLAUDE.md")).unwrap();
    let clients = [
        target(Client::ClaudeCode, None),
        target(Client::ClaudeCode, Some(&work)),
    ];
    let (files, _) = agent_files(home, &clients);
    assert_eq!(
        paths(&files, home),
        [(
            "Claude Code, Claude Code · work".to_string(),
            ".claude/CLAUDE.md".to_string()
        )]
    );
}
