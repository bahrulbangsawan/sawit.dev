//! GitHub's server: every client is handed Sawit's sign-in by a command, never the token, in
//! its own format, its file or its CLI.

use std::path::PathBuf;

use serde_json::json;

use super::*;
use crate::mcp_clients::merge::merge_entry;

const SAWIT: &str = "/Applications/Sawit.app/Contents/MacOS/sawit";
const GITHUB_URL: &str = "https://api.githubcopilot.com/mcp/";

fn github() -> Spec {
    Spec::http("github", GITHUB_URL)
        .with_npx("/opt/homebrew/bin/npx")
        .with_token_helper(SAWIT)
}

fn target_at(client: Client, config: PathBuf) -> Target {
    Target {
        client,
        profile: "default".into(),
        config,
        profile_dir: None,
    }
}

/// GitHub's server takes Sawit's sign-in, and no client's config holds the token: Claude Code
/// and Codex name the helper by its absolute path, the clients that run the bridge start it
/// through `sawit github token --exec` with the header's variable by name, and VS Code and
/// Copilot CLI get the bare URL, to sign in to GitHub themselves. Every client, every format.
#[test]
fn github_is_handed_the_sign_in_by_the_helper_never_the_token() {
    let dir = tempfile::tempdir().unwrap();
    let spec = github();
    let helper = format!("'{SAWIT}' github token --header");
    for client in Client::ALL {
        let file = if client == Client::Codex {
            "config.toml"
        } else {
            "mcp.json"
        };
        let t = target_at(client, dir.path().join(format!("{client:?}")).join(file));
        let text = if client.editable() {
            install_file(&t, &spec).unwrap();
            assert_eq!(status(&t, &spec), Status::Installed, "{client:?}");
            fs::read_to_string(&t.config).unwrap()
        } else {
            snippet(&t, &spec)
        };
        for secret in ["Bearer", "gho_", "\"headers\"", "http_headers ="] {
            assert!(!text.contains(secret), "{client:?} holds {secret}: {text}");
        }
        assert!(text.contains(GITHUB_URL), "{client:?}: {text}");
        match handover(client) {
            Handover::Helper => assert!(text.contains(&helper), "{client:?}: {text}"),
            Handover::Bridge => {
                for part in [
                    SAWIT,
                    "--exec",
                    "/opt/homebrew/bin/npx",
                    "mcp-remote",
                    "Authorization:${SAWIT_GITHUB_AUTHORIZATION}",
                ] {
                    assert!(text.contains(part), "{client:?} lacks {part}: {text}");
                }
            }
            Handover::ClientSignIn => {
                assert!(!text.contains("sawit"), "{client:?}: {text}")
            }
        }
    }
    let at = |c: Client, f: &str| dir.path().join(format!("{c:?}")).join(f);
    assert_eq!(
        json_at(&at(Client::ClaudeCode, "mcp.json"))["mcpServers"]["github"],
        json!({"type": "http", "url": GITHUB_URL, "headersHelper": helper})
    );
    let codex = fs::read_to_string(at(Client::Codex, "config.toml")).unwrap();
    assert!(
        codex.contains(&format!("http_headers_helper = \"{helper}\"")),
        "{codex}"
    );
    assert!(!codex.contains("command"), "{codex}");
    // An app started with launchd's PATH is told where node is, for the bridge behind sawit.
    let desktop = json_at(&at(Client::ClaudeDesktop, "mcp.json"))["mcpServers"]["github"].clone();
    assert_eq!(desktop["command"], SAWIT);
    assert!(
        desktop["env"]["PATH"]
            .as_str()
            .unwrap()
            .starts_with("/opt/homebrew/bin:")
    );
    assert_eq!(
        json_at(&at(Client::VsCode, "mcp.json"))["servers"]["github"],
        json!({"type": "http", "url": GITHUB_URL})
    );
}

/// An entry another `sawit` hands the sign-in to (an older build, an app since moved) is out
/// of date, and installing again points its helper here, keeping the user's own keys. One
/// that signs in some other way (an entry made by hand, VS Code's) is the server installed.
#[test]
fn a_helper_of_another_sawit_is_out_of_date_and_moves_here() {
    let dir = tempfile::tempdir().unwrap();
    let claude = target_at(Client::ClaudeCode, dir.path().join(".claude.json"));
    fs::write(
            &claude.config,
            format!(
                r#"{{"mcpServers": {{"github": {{"type": "http", "url": "{GITHUB_URL}", "headersHelper": "'/old/sawit' github token --header", "timeout": 30}}}}}}"#
            ),
        )
        .unwrap();
    assert!(matches!(status(&claude, &github()), Status::Outdated(_)));
    install_file(&claude, &github()).unwrap();
    let entry = json_at(&claude.config)["mcpServers"]["github"].clone();
    assert_eq!(
        entry["headersHelper"],
        format!("'{SAWIT}' github token --header")
    );
    assert_eq!(entry["timeout"], 30);
    assert_eq!(status(&claude, &github()), Status::Installed);

    let codex = target_at(Client::Codex, dir.path().join("config.toml"));
    fs::write(
            &codex.config,
            format!(
                "[mcp_servers.github]\nurl = \"{GITHUB_URL}\" # GitHub\nbearer_token_env_var = \"GITHUB_PAT\"\nstartup_timeout_sec = 20\n"
            ),
        )
        .unwrap();
    // Made by hand with a token of the user's: installed, though not through Sawit's sign-in.
    assert_eq!(status(&codex, &github()), Status::Installed);
    install_file(&codex, &github()).unwrap();
    let text = fs::read_to_string(&codex.config).unwrap();
    // Codex would send that variable's token instead of the helper's.
    assert!(!text.contains("bearer_token_env_var"), "{text}");
    assert!(text.contains("# GitHub"), "{text}");
    assert!(text.contains("startup_timeout_sec = 20"), "{text}");
    assert!(text.contains("http_headers_helper"), "{text}");

    // Sawit's helper goes when the server no longer takes the sign-in; a user's own stays.
    let plain = Spec::http("github", GITHUB_URL);
    install_file(&claude, &plain).unwrap();
    assert!(
        json_at(&claude.config)["mcpServers"]["github"]
            .get("headersHelper")
            .is_none()
    );
    install_file(&codex, &plain).unwrap();
    assert!(
        !fs::read_to_string(&codex.config)
            .unwrap()
            .contains("http_headers_helper")
    );
    let mine = merge_entry(
        &json!({"url": "https://x.dev", "headersHelper": "~/bin/my-headers"}),
        json!({"type": "http", "url": "https://x.dev"}),
    );
    assert_eq!(mine["headersHelper"], "~/bin/my-headers");
}

/// Through Claude Code's CLI the entry goes in whole (`mcp add-json`: `mcp add` takes no
/// `headersHelper`), naming the helper and no token; Codex's CLI can't take the helper, so its
/// file is edited instead.
#[test]
fn github_goes_through_claudes_add_json_and_codexs_file() {
    let h = home();
    let targets = discover(h.path(), None);
    let claude = find(&targets, Client::ClaudeCode, "work");
    let call = cli_add(claude, &github(), Path::new("/bin/claude")).unwrap();
    assert_eq!(
        call.args[..5],
        ["mcp", "add-json", "--scope", "user", "github"]
    );
    let added: Value = serde_json::from_str(&call.args[5]).unwrap();
    assert_eq!(
        added,
        json!({"type": "http", "url": GITHUB_URL, "headersHelper": format!("'{SAWIT}' github token --header")})
    );
    assert_eq!(call.args.len(), 6);
    let codex = find(&targets, Client::Codex, "default");
    assert!(cli_add(codex, &github(), Path::new("/bin/codex")).is_none());
    // Which clients take which way, as the MCP page says.
    assert!(bridged(Client::Cursor, &github()));
    assert!(bridged(Client::ClaudeDesktop, &github()));
    assert!(!bridged(Client::ClaudeCode, &github()));
    assert!(!bridged(Client::VsCode, &github()));
    assert!(!bridged(Client::Cursor, &Spec::http("x", "https://x.dev")));
}
