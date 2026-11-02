//! Finding the clients and profiles on this Mac.

use super::*;

#[test]
fn discovers_every_claude_profile_and_installed_clients() {
    let h = home();
    let targets = discover(h.path(), None);
    let default = find(&targets, Client::ClaudeCode, "default");
    assert_eq!(default.config, h.path().join(".claude.json"));
    assert_eq!(default.profile_dir, None);
    let work = find(&targets, Client::ClaudeCode, "work");
    assert_eq!(work.config, h.path().join(".claude-work/.claude.json"));
    assert_eq!(
        work.profile_dir.as_deref(),
        Some(h.path().join(".claude-work").as_path())
    );
    find(&targets, Client::ClaudeCode, "personal");
    assert!(!targets.iter().any(|t| t.profile == "pool"));
    find(&targets, Client::Codex, "default");
    find(&targets, Client::ClaudeDesktop, "default");
    find(&targets, Client::Zed, "default");
    // Clients that aren't there aren't offered.
    assert!(!targets.iter().any(|t| t.client == Client::Cursor));
}

#[test]
fn a_claude_config_dir_outside_home_is_included() {
    let h = home();
    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(elsewhere.path().join(".claude.json"), "{}").unwrap();
    let targets = discover(h.path(), Some(elsewhere.path().to_path_buf()));
    let t = targets
        .iter()
        .find(|t| t.config == elsewhere.path().join(".claude.json"))
        .expect("CLAUDE_CONFIG_DIR profile");
    assert_eq!(t.client, Client::ClaudeCode);
    // The same folder via the env var is not listed twice.
    let again = discover(h.path(), Some(h.path().join(".claude-work")));
    assert_eq!(again.iter().filter(|t| t.profile == "work").count(), 1);
}

#[test]
fn popular_agents_and_editors_are_detected() {
    let h = home();
    let p = h.path();
    for dir in [
        ".gemini",
        ".qwen",
        ".config/opencode",
        ".config/amp",
        ".copilot",
        ".factory",
        ".kiro",
        ".junie",
        ".augment",
        ".warp",
        ".lmstudio",
        ".config/kilo",
        ".config/devin",
        ".gemini/config",
        ".cline/data/settings",
        ".config/goose",
        ".continue",
        ".config/crush",
        "Library/Application Support/Code - Insiders/User",
    ] {
        fs::create_dir_all(p.join(dir)).unwrap();
    }
    let targets = discover(p, None);
    let config = |c: Client| {
        targets
            .iter()
            .find(|t| t.client == c)
            .unwrap_or_else(|| panic!("{c:?} not detected"))
            .config
            .strip_prefix(p)
            .unwrap()
            .to_path_buf()
    };
    assert_eq!(config(Client::Qwen), Path::new(".qwen/settings.json"));
    assert_eq!(config(Client::Amp), Path::new(".config/amp/settings.json"));
    assert_eq!(
        config(Client::CopilotCli),
        Path::new(".copilot/mcp-config.json")
    );
    assert_eq!(config(Client::FactoryDroid), Path::new(".factory/mcp.json"));
    assert_eq!(config(Client::Kiro), Path::new(".kiro/settings/mcp.json"));
    assert_eq!(config(Client::Junie), Path::new(".junie/mcp/mcp.json"));
    assert_eq!(config(Client::Auggie), Path::new(".augment/settings.json"));
    assert_eq!(config(Client::Warp), Path::new(".warp/.mcp.json"));
    assert_eq!(config(Client::LmStudio), Path::new(".lmstudio/mcp.json"));
    assert_eq!(
        config(Client::KiloCode),
        Path::new(".config/kilo/kilo.json")
    );
    assert_eq!(
        config(Client::Windsurf),
        Path::new(".config/devin/mcp_config.json")
    );
    assert_eq!(
        config(Client::Antigravity),
        Path::new(".gemini/config/mcp_config.json")
    );
    assert_eq!(
        config(Client::Cline),
        Path::new(".cline/data/settings/cline_mcp_settings.json")
    );
    assert_eq!(
        config(Client::Goose),
        Path::new(".config/goose/config.yaml")
    );
    assert_eq!(config(Client::Continue), Path::new(".continue/config.yaml"));
    assert_eq!(config(Client::Crush), Path::new(".config/crush/crushrc"));
    assert_eq!(
        config(Client::VsCodeInsiders),
        Path::new("Library/Application Support/Code - Insiders/User/mcp.json")
    );
}

#[test]
fn unverified_locations_need_the_file_itself() {
    let h = home();
    let p = h.path();
    let trae = p.join("Library/Application Support/Trae/User");
    fs::create_dir_all(&trae).unwrap();
    let roo = p.join(
        "Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings",
    );
    fs::create_dir_all(&roo).unwrap();
    fs::create_dir_all(p.join(".aws/amazonq")).unwrap();
    // Folders alone: not offered (we'd be guessing a file name).
    let targets = discover(p, None);
    for c in [Client::Trae, Client::RooCode, Client::AmazonQ] {
        assert!(!targets.iter().any(|t| t.client == c), "{c:?}");
    }
    fs::write(trae.join("mcp.json"), "{}").unwrap();
    fs::write(roo.join("mcp_settings.json"), "{}").unwrap();
    fs::write(p.join(".aws/amazonq/mcp.json"), "{}").unwrap();
    let targets = discover(p, None);
    for c in [Client::Trae, Client::RooCode, Client::AmazonQ] {
        assert!(targets.iter().any(|t| t.client == c), "{c:?}");
    }
}

#[test]
fn legacy_locations_are_offered_when_their_file_exists() {
    let h = home();
    let p = h.path();
    fs::create_dir_all(p.join(".codeium/windsurf")).unwrap();
    fs::write(p.join(".codeium/windsurf/mcp_config.json"), "{}").unwrap();
    let targets = discover(p, None);
    let windsurf: Vec<_> = targets
        .iter()
        .filter(|t| t.client == Client::Windsurf)
        .collect();
    assert_eq!(windsurf.len(), 1);
    assert!(
        windsurf[0]
            .config
            .ends_with(".codeium/windsurf/mcp_config.json")
    );
}

#[test]
fn every_client_has_a_category() {
    for c in Client::ALL {
        let _ = c.category();
        assert!(!c.name().is_empty());
    }
    assert_eq!(Client::ClaudeCode.category(), Category::Agent);
    assert_eq!(Client::Cursor.category(), Category::Editor);
}
