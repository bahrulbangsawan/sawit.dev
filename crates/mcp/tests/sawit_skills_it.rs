//! A real install and removal of Sawit Skills by the `sawit-mcp` binary, started with `env -i` in
//! a temporary home folder and Sawit data folder: nothing of the real home's is read or written.
//! The agents there are folders, as a fresh install of each leaves them; the user's own
//! CLAUDE.md comes back byte for byte, the files Sawit made go, and so do the MCP entries the
//! install added, while the ones that were there before stay.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// Calls `install_sawit_skills` with `args` in a `sawit-mcp` started with `env -i` and only
/// `HOME`, `SAWIT_HOME`, `TMPDIR` and the system `PATH`: its answer's text, and whether it failed.
fn call(root: &Path, args: Value) -> (String, bool) {
    let mut child = Command::new("/usr/bin/env")
        .arg("-i")
        .arg(format!("HOME={}", root.join("home").display()))
        .arg(format!("SAWIT_HOME={}", root.join("data").display()))
        .arg(format!("TMPDIR={}/", root.join("tmp").display()))
        .arg("PATH=/usr/bin:/bin:/usr/sbin:/sbin")
        .arg(env!("CARGO_BIN_EXE_sawit-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("sawit-mcp starts");
    let mut stdin = child.stdin.take().unwrap();
    let messages = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "sawit-skills-it", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "install_sawit_skills", "arguments": args}}),
    ];
    for message in messages {
        writeln!(stdin, "{message}").unwrap();
    }
    let mut answer = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let message: Value = serde_json::from_str(&line.unwrap()).unwrap_or_default();
        if message["id"] == 2 {
            answer = Some(message);
            break;
        }
    }
    drop(stdin);
    let _ = child.wait();
    let result = &answer.expect("an answer to the call")["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    (text.to_string(), result["isError"] == true)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn install_and_remove_in_a_temporary_home() {
    let dir = tempfile::tempdir().unwrap();
    // macOS's temporary folder is reached through /var, a link to /private/var.
    let root = dir.path().canonicalize().unwrap();
    let home = root.join("home");
    for agent in [
        ".claude",
        ".codex",
        ".gemini",
        ".config/opencode",
        ".cursor",
    ] {
        std::fs::create_dir_all(home.join(agent)).unwrap();
    }
    std::fs::create_dir_all(root.join("tmp")).unwrap();
    let mine = "# Mine\n\nKeep this exactly.\n";
    let claude_md = home.join(".claude/CLAUDE.md");
    std::fs::write(&claude_md, mine).unwrap();
    let block = sawit_core::sawit_skills::block_text();
    // Entries named sawit that were there before: Gemini CLI's runs another Sawit, Cursor's this
    // one. An install leaves both, and a removal too.
    let elsewhere = r#"{"mcpServers":{"sawit":{"command":"/Applications/Sawit.app/Contents/MacOS/sawit-mcp","args":[]}}}"#;
    std::fs::write(home.join(".gemini/settings.json"), elsewhere).unwrap();
    let here =
        json!({"mcpServers": {"sawit": {"command": env!("CARGO_BIN_EXE_sawit-mcp"), "args": []}}});
    std::fs::write(home.join(".cursor/mcp.json"), here.to_string()).unwrap();

    let (said, failed) = call(&root, json!({"confirm": true}));
    assert!(!failed, "{said}");
    // The skills, in each agent's skills folder.
    let pack = sawit_core::skills::get(sawit_core::skills::SAWIT_PACK).unwrap();
    for folder in [
        ".claude/skills",
        ".agents/skills",
        ".gemini/skills",
        ".config/opencode/skills",
        ".cursor/skills",
    ] {
        for skill in pack.skills {
            let skill_md = home.join(folder).join(skill).join("SKILL.md");
            assert!(skill_md.is_file(), "{}\n{said}", skill_md.display());
        }
    }
    // The block: at the top of the user's CLAUDE.md (which opencode reads too), alone in the
    // files Sawit made; the user's file copied into Sawit's data folder first.
    assert_eq!(read(&claude_md), format!("{block}\n{mine}"));
    assert_eq!(read(&home.join(".codex/AGENTS.md")), block);
    assert_eq!(read(&home.join(".gemini/GEMINI.md")), block);
    assert!(!home.join(".config/opencode/AGENTS.md").exists());
    let copy = root.join("data/instruction-backups/home/.claude/CLAUDE.md");
    assert_eq!(read(&copy), mine);
    // Sawit's MCP server, added to each client that had no entry for it, and noted.
    let ours = env!("CARGO_BIN_EXE_sawit-mcp");
    let added = [
        ".claude.json",
        ".codex/config.toml",
        ".config/opencode/opencode.json",
    ];
    for config in added {
        let text = read(&home.join(config));
        assert!(text.contains(ours), "{config}: {text}");
    }
    assert_eq!(read(&home.join(".gemini/settings.json")), elsewhere);
    assert_eq!(read(&home.join(".cursor/mcp.json")), here.to_string());
    let noted = read(&root.join("data/mcp-entries.json"));
    for config in added {
        assert!(
            noted.contains(&format!("home/{config}\"")),
            "{config}: {noted}"
        );
    }
    assert!(
        !noted.contains("settings.json") && !noted.contains("mcp.json\""),
        "{noted}"
    );
    for path in [
        "~/.claude/CLAUDE.md",
        "~/.codex/AGENTS.md",
        "~/.cursor/mcp.json",
    ] {
        assert!(said.contains(path), "{path}\n{said}");
    }

    // Again: nothing changes.
    let (said, failed) = call(&root, json!({"confirm": true}));
    assert!(!failed, "{said}");
    assert!(
        said.starts_with("Sawit Skills are already current everywhere."),
        "{said}"
    );

    let (said, failed) = call(&root, json!({"confirm": true, "remove": true}));
    assert!(!failed, "{said}");
    assert_eq!(read(&claude_md), mine);
    assert!(!home.join(".codex/AGENTS.md").exists(), "{said}");
    assert!(!home.join(".gemini/GEMINI.md").exists(), "{said}");
    for skill in pack.skills {
        assert!(!home.join(".claude/skills").join(skill).exists(), "{said}");
    }
    // The entries the install added are gone; the ones that were there before stay, and say so.
    for config in added {
        let text = read(&home.join(config));
        assert!(!text.contains(ours), "{config}: {text}");
    }
    assert_eq!(read(&home.join(".gemini/settings.json")), elsewhere);
    assert_eq!(read(&home.join(".cursor/mcp.json")), here.to_string());
    assert!(
        said.contains("~/.claude.json (removed: Install Skills had added it)"),
        "{said}"
    );
    assert!(
        said.contains("~/.cursor/mcp.json (stays: it was there before Install Skills)"),
        "{said}"
    );
    assert!(
        said.contains("~/.gemini/settings.json (stays: it was there before Install Skills)"),
        "{said}"
    );
    assert!(!read(&root.join("data/mcp-entries.json")).contains("home/"));
}
