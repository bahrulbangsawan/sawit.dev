//! The entry each client gets: its shape, the bridge to remote servers, and the `PATH` apps
//! are told.

use serde_json::json;

use super::*;
use crate::mcp_clients::shape::{node_path, own_path};

#[test]
fn each_client_gets_its_own_entry_shape() {
    let server = "/bin/sawit-mcp";
    let shape = |c| entry(c, &Spec::sawit(Path::new(server)));
    assert_eq!(shape(Client::ClaudeDesktop).0, "mcpServers");
    assert_eq!(shape(Client::Cursor).1["command"], server);
    assert_eq!(shape(Client::Zed).0, "context_servers");
    assert_eq!(shape(Client::OpenCode).0, "mcp");
    assert_eq!(shape(Client::OpenCode).1["command"][0], server);
    assert_eq!(shape(Client::OpenCode).1["type"], "local");
    assert_eq!(shape(Client::VsCode).0, "servers");
}

#[test]
fn agent_specific_entry_shapes() {
    let s = Path::new("/bin/sawit-mcp");
    assert_eq!(entry(Client::Amp, &Spec::sawit(s)).0, "amp.mcpServers");
    let copilot = entry(Client::CopilotCli, &Spec::sawit(s)).1;
    assert_eq!(copilot["type"], "local");
    assert_eq!(copilot["tools"][0], "*");
    assert_eq!(
        entry(Client::FactoryDroid, &Spec::sawit(s)).1["type"],
        "stdio"
    );
    assert_eq!(entry(Client::Kiro, &Spec::sawit(s)).1["disabled"], false);
    assert_eq!(entry(Client::KiloCode, &Spec::sawit(s)).0, "mcp");
    assert_eq!(
        entry(Client::KiloCode, &Spec::sawit(s)).1["command"][0],
        "/bin/sawit-mcp"
    );
    assert_eq!(entry(Client::VsCodeInsiders, &Spec::sawit(s)).0, "servers");
    assert_eq!(entry(Client::Cursor, &Spec::sawit(s)).1["type"], "stdio");
    assert_eq!(
        entry(Client::RooCode, &Spec::sawit(s)).1["alwaysAllow"],
        serde_json::json!([])
    );
}

#[test]
fn remote_servers_use_each_clients_format_or_the_bridge() {
    let spec = Spec::http("mobbin", "https://api.mobbin.com/mcp").with_npx("/opt/bin/npx");
    let url = "https://api.mobbin.com/mcp";
    assert_eq!(
        entry(Client::ClaudeCode, &spec).1,
        json!({"type": "http", "url": url})
    );
    assert_eq!(entry(Client::Gemini, &spec).1["httpUrl"], url);
    assert_eq!(entry(Client::Windsurf, &spec).1["serverUrl"], url);
    assert_eq!(entry(Client::OpenCode, &spec).1["type"], "remote");
    assert_eq!(
        entry(Client::Zed, &spec),
        ("context_servers", json!({"url": url}))
    );
    // Claude Desktop only runs programs: the mcp-remote bridge, told
    // where node is.
    assert_eq!(
        entry(Client::ClaudeDesktop, &spec).1,
        json!({
            "command": "/opt/bin/npx",
            "args": ["-y", "mcp-remote", url],
            "env": {"PATH": "/opt/bin:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"}
        })
    );
    let keyed = Spec::http("es", "https://es.dev/mcp").header("Authorization", "Bearer ${ES_KEY}");
    assert_eq!(
        entry(Client::Cursor, &keyed).1["headers"]["Authorization"],
        "Bearer ${ES_KEY}"
    );
    assert_eq!(
        entry(Client::ClaudeDesktop, &keyed).1["args"],
        json!([
            "-y",
            "mcp-remote",
            "https://es.dev/mcp",
            "--header",
            "Authorization:Bearer ${ES_KEY}"
        ])
    );
}

#[test]
fn clients_that_only_run_programs_need_the_bridge() {
    for c in [
        Client::ClaudeDesktop,
        Client::LmStudio,
        Client::Warp,
        Client::Junie,
    ] {
        assert!(needs_bridge(c), "{c:?}");
    }
    for c in [
        Client::ClaudeCode,
        Client::Codex,
        Client::Cursor,
        Client::Zed,
    ] {
        assert!(!needs_bridge(c), "{c:?}");
    }
}

/// Apps such as Claude Desktop start with launchd's `PATH` (no Homebrew,
/// no nvm), and `npx` starts with `#!/usr/bin/env node`: their entries say
/// where `node` is. Agents run from a terminal keep its `PATH`.
#[test]
fn apps_started_by_launchd_are_told_where_node_is() {
    let npx = "/Users/me/.nvm/versions/node/v22.11.0/bin/npx";
    let bridge = Spec::http("mobbin", "https://api.mobbin.com/mcp").with_npx(npx);
    let path = "/Users/me/.nvm/versions/node/v22.11.0/bin:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";
    assert_eq!(
        entry(Client::ClaudeDesktop, &bridge).1["env"],
        json!({ "PATH": path })
    );
    let rea = Spec::stdio("rea", npx, vec!["-y".into(), "rea-agents@6.3.0".into()]).with_npx(npx);
    assert_eq!(entry(Client::LmStudio, &rea).1["env"]["PATH"], path);
    // An agent run from a terminal: its own PATH.
    assert_eq!(entry(Client::ClaudeCode, &rea).1["env"], json!({}));
    // Not npx: nothing to add.
    let sawit = Spec::sawit(Path::new(
        "/Applications/Sawit.app/Contents/MacOS/sawit-mcp",
    ));
    assert!(entry(Client::ClaudeDesktop, &sawit).1.get("env").is_none());
    // npx in /usr/bin finds a node there.
    let system = Spec::http("x", "https://x.dev/mcp").with_npx("/usr/bin/npx");
    assert!(entry(Client::ClaudeDesktop, &system).1.get("env").is_none());
}

/// A server run with `bunx` or `pnpm dlx` instead of `npx -y` is written with that runner,
/// starts in an app with launchd's PATH (the runner's folder, then nvm's node), and reads as
/// the same server whichever package manager wrote it.
#[test]
fn another_package_manager_runs_the_same_server() {
    let node = Some("/Users/me/.nvm/versions/node/v22.11.0/bin".to_string());
    let bunx = "/Users/me/.bun/bin/bunx";
    let rea_with = |program: &str, runner: &[&str]| {
        let mut args: Vec<String> = runner.iter().map(|a| a.to_string()).collect();
        args.extend(["rea-agents@6.3.0".into(), "mcp".into()]);
        Spec::stdio("rea", program, args).with_runner(
            program,
            runner.iter().map(|a| a.to_string()).collect(),
            node.clone(),
        )
    };
    let rea = rea_with(bunx, &[]);
    let (_, v) = entry(Client::ClaudeDesktop, &rea);
    assert_eq!(v["command"], bunx);
    assert_eq!(v["args"], json!(["rea-agents@6.3.0", "mcp"]));
    assert_eq!(
        v["env"]["PATH"],
        "/Users/me/.bun/bin:/Users/me/.nvm/versions/node/v22.11.0/bin:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    );
    // The bridge to a remote server, with pnpm.
    let bridge = Spec::http("mobbin", "https://api.mobbin.com/mcp").with_runner(
        "/opt/homebrew/bin/pnpm",
        vec!["dlx".into()],
        None,
    );
    let (_, v) = entry(Client::LmStudio, &bridge);
    assert_eq!(v["command"], "/opt/homebrew/bin/pnpm");
    assert_eq!(
        v["args"],
        json!(["dlx", "mcp-remote", "https://api.mobbin.com/mcp"])
    );
    // Written by npx, read with pnpm picked (and the other way round): installed.
    let npx = "/opt/homebrew/bin/npx";
    let config = |command: &str, args: &[&str]| {
        Entries::Servers([("rea".to_string(), format!("{command} {}", args.join(" ")))].into())
    };
    let by_npx = config(npx, &["-y", "rea-agents@6.3.0", "mcp"]);
    let by_pnpm = config(
        "/opt/homebrew/bin/pnpm",
        &["dlx", "rea-agents@6.3.0", "mcp"],
    );
    let pnpm = rea_with("/opt/homebrew/bin/pnpm", &["dlx"]);
    let npx_spec = rea_with(npx, &["-y"]);
    assert_eq!(status_in(&by_npx, &pnpm), Status::Installed);
    assert_eq!(status_in(&by_pnpm, &npx_spec), Status::Installed);
    assert_eq!(status_in(&by_npx, &rea), Status::Installed);
    // Another version is still another.
    let old = config(npx, &["-y", "rea-agents@6.2.0", "mcp"]);
    assert!(matches!(status_in(&old, &rea), Status::Outdated(_)));
    // The PATH Sawit wrote for bunx with nvm's node is Sawit's: an update moves it.
    let written = node_path(bunx, node.as_deref()).unwrap();
    assert!(own_path(&written, bunx));
    assert!(own_path(&node_path(bunx, None).unwrap(), bunx));
    assert!(!own_path("/Users/me/bin:/usr/bin", bunx));
}

/// CATALOG M2: only apps that start with launchd's `PATH` (Claude Desktop, LM Studio) are
/// told where `node` is. VS Code, its forks and extensions, and Zed read the login shell's
/// environment and lay an entry's `env` over it: a `PATH` there hid `~/.cargo/bin`, pyenv's
/// shims and the rest of the user's from the server and what it starts.
#[test]
fn only_apps_without_the_shells_path_are_told_where_node_is() {
    let npx = "/Users/me/.nvm/versions/node/v22.11.0/bin/npx";
    let rea = Spec::stdio("rea", npx, vec!["-y".into(), "rea-agents@6.3.0".into()]).with_npx(npx);
    let bridge = Spec::http("x", "https://x.dev/mcp").with_npx(npx);
    let path = |client, spec| {
        let (_, v) = entry(client, spec);
        ["env", "environment"]
            .iter()
            .find_map(|k| v.get(*k)?.get("PATH").cloned())
    };
    for client in [Client::ClaudeDesktop, Client::LmStudio] {
        assert!(path(client, &rea).is_some(), "{client:?}");
        assert!(path(client, &bridge).is_some(), "{client:?} (bridge)");
    }
    for client in [
        Client::Cursor,
        Client::Windsurf,
        Client::VsCode,
        Client::VsCodeInsiders,
        Client::Zed,
        Client::Antigravity,
        Client::Trae,
        Client::RooCode,
        Client::KiloCode,
        Client::Continue,
    ] {
        assert_eq!(path(client, &rea), None, "{client:?}");
        assert_eq!(path(client, &bridge), None, "{client:?} (bridge)");
    }
}
