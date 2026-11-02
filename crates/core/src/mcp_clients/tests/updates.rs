//! Updating an entry: the user's own keys stay, the `PATH` Sawit wrote moves, and the backup
//! is the file as it was before the change.

use serde_json::json;

use super::*;
use crate::mcp_clients::shape::node_path;

/// A Cline config whose Sawit entry points at an older build, with the
/// fields a user fills in after the install.
fn customised_cline(h: &tempfile::TempDir) -> Target {
    let dir = h.path().join(".cline/data/settings");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
            dir.join("cline_mcp_settings.json"),
            r#"{"mcpServers": {"sawit": {"command": "/old/sawit-mcp", "args": [], "disabled": false, "autoApprove": ["query", "list_instances"], "env": {"SAWIT_HOME": "/x"}, "timeout": 120}, "other": {"command": "y"}}}"#,
        )
        .unwrap();
    find(&discover(h.path(), None), Client::Cline, "default").clone()
}

#[test]
fn an_update_keeps_the_users_own_keys() {
    let h = home();
    let t = customised_cline(&h);
    let server = Path::new("/Applications/Sawit.app/Contents/MacOS/sawit-mcp");
    assert!(matches!(
        status(&t, &Spec::sawit(server)),
        Status::Outdated(_)
    ));
    install_file(&t, &Spec::sawit(server)).unwrap();
    let v = json_at(&t.config);
    let entry = &v["mcpServers"]["sawit"];
    assert_eq!(entry["command"], server.display().to_string());
    assert_eq!(entry["autoApprove"], json!(["query", "list_instances"]));
    assert_eq!(entry["env"], json!({"SAWIT_HOME": "/x"}));
    assert_eq!(entry["timeout"], 120);
    assert_eq!(entry["disabled"], false);
    // The entry keeps its place among the servers, and its keys their order.
    let servers: Vec<_> = v["mcpServers"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(servers, ["sawit", "other"]);
    let keys: Vec<_> = entry.as_object().unwrap().keys().cloned().collect();
    assert_eq!(
        keys,
        [
            "command",
            "args",
            "disabled",
            "autoApprove",
            "env",
            "timeout"
        ]
    );
    assert_eq!(status(&t, &Spec::sawit(server)), Status::Installed);
}

/// CATALOG M1: the `env.PATH` Sawit writes is worked out from where `npx` is, so it's
/// Sawit's, not the user's. An update after nvm moved on (its default v20 → v22, then
/// `nvm uninstall 20`) points it at the new `npx`'s folder: it kept v20's, as the user's
/// own value, and `#!/usr/bin/env node` found no node, so the server couldn't start. It goes
/// when the new entry needs none; a `PATH` the user wrote stays.
#[test]
fn an_update_moves_the_path_sawit_wrote_for_npx() {
    let h = home();
    let t = find(&discover(h.path(), None), Client::ClaudeDesktop, "default").clone();
    let npx = |version: &str| format!("/Users/me/.nvm/versions/node/{version}/bin/npx");
    let bridge = |npx: &str| Spec::http("mobbin", "https://api.mobbin.com/mcp").with_npx(npx);
    let mobbin = || json_at(&t.config)["mcpServers"]["mobbin"].clone();
    install_file(&t, &bridge(&npx("v20.0.0"))).unwrap();
    install_file(&t, &bridge(&npx("v22.11.0"))).unwrap();
    assert_eq!(mobbin()["command"], npx("v22.11.0"));
    assert_eq!(
        mobbin()["env"],
        json!({"PATH": node_path(&npx("v22.11.0"), None).unwrap()})
    );

    // An npx where launchd's PATH finds node needs none: Sawit's goes, and with it the
    // `env` it alone filled.
    install_file(&t, &bridge("/usr/bin/npx")).unwrap();
    assert_eq!(mobbin()["command"], "/usr/bin/npx");
    assert!(mobbin().get("env").is_none(), "{}", mobbin());

    // A PATH the user wrote, and other keys beside it, stay theirs.
    install_file(&t, &bridge(&npx("v20.0.0"))).unwrap();
    let mut config = json_at(&t.config);
    config["mcpServers"]["mobbin"]["env"] =
        json!({"PATH": "/opt/mine/bin:/usr/bin:/bin", "DEBUG": "1"});
    fs::write(&t.config, config.to_string()).unwrap();
    install_file(&t, &bridge(&npx("v22.11.0"))).unwrap();
    assert_eq!(
        mobbin()["env"],
        json!({"PATH": "/opt/mine/bin:/usr/bin:/bin", "DEBUG": "1"})
    );

    // Editors that have the shell's PATH no longer get one (CATALOG M2): what an earlier
    // build wrote goes at their next update, in Cursor's `env` and in Kilo's `environment`
    // (whose `command` holds the arguments too).
    let args = ["-y", "rea-agents@6.3.0"];
    let rea = |npx: &str| {
        Spec::stdio("rea", npx, args.iter().map(|a| a.to_string()).collect()).with_npx(npx)
    };
    let old_path = node_path(&npx("v20.0.0"), None).unwrap();
    fs::create_dir_all(h.path().join(".cursor")).unwrap();
    fs::create_dir_all(h.path().join(".config/kilo")).unwrap();
    let targets = discover(h.path(), None);
    let cursor = find(&targets, Client::Cursor, "default");
    fs::write(
            &cursor.config,
            json!({"mcpServers": {"rea": {"type": "stdio", "command": npx("v20.0.0"), "args": args, "env": {"PATH": old_path}}}})
                .to_string(),
        )
        .unwrap();
    install_file(cursor, &rea(&npx("v22.11.0"))).unwrap();
    assert_eq!(
        json_at(&cursor.config)["mcpServers"]["rea"],
        json!({"type": "stdio", "command": npx("v22.11.0"), "args": args})
    );
    let kilo = find(&targets, Client::KiloCode, "default");
    fs::write(
            &kilo.config,
            json!({"mcp": {"rea": {"type": "local", "command": [npx("v20.0.0"), args[0], args[1]], "enabled": true, "environment": {"PATH": old_path}}}})
                .to_string(),
        )
        .unwrap();
    install_file(kilo, &rea(&npx("v22.11.0"))).unwrap();
    assert_eq!(
        json_at(&kilo.config)["mcp"]["rea"],
        json!({"type": "local", "command": [npx("v22.11.0"), args[0], args[1]], "enabled": true})
    );
}

#[test]
fn switching_between_a_program_and_a_url_drops_the_other_kinds_keys() {
    let h = home();
    let t = customised_cline(&h);
    let spec = Spec::http("sawit", "https://sawit.dev/mcp").header("X-Team", "a");
    install_file(&t, &spec).unwrap();
    let entry = json_at(&t.config)["mcpServers"]["sawit"].clone();
    assert_eq!(entry["url"], "https://sawit.dev/mcp");
    assert_eq!(entry["type"], "streamableHttp");
    assert_eq!(entry["headers"], json!({"X-Team": "a"}));
    for gone in ["command", "args", "env"] {
        assert!(entry.get(gone).is_none(), "{gone} stayed: {entry}");
    }
    assert_eq!(entry["autoApprove"], json!(["query", "list_instances"]));
    assert_eq!(entry["timeout"], 120);
}

#[test]
fn a_codex_update_edits_the_table_in_place() {
    let h = home();
    let config = h.path().join(".codex/config.toml");
    let original = "model = \"o3\"\n\n# Sawit, from the app\n[mcp_servers.sawit]\ncommand = \"/old/sawit-mcp\" # moved since\nargs = []\nstartup_timeout_sec = 20\ntool_timeout_sec = 90\nenabled = true\n\n[mcp_servers.sawit.env]\nSAWIT_HOME = \"/x\"\n";
    fs::write(&config, original).unwrap();
    let t = find(&discover(h.path(), None), Client::Codex, "default").clone();
    install_file(&t, &Spec::sawit(Path::new("/new/sawit-mcp"))).unwrap();
    let text = fs::read_to_string(&config).unwrap();
    assert_eq!(
        text,
        original.replace("\"/old/sawit-mcp\"", "\"/new/sawit-mcp\""),
        "{text}"
    );
}

#[test]
fn the_backup_holds_the_file_as_it_was_before_the_last_change() {
    let h = home();
    let t = customised_cline(&h);
    install_file(&t, &Spec::sawit(Path::new("/one/sawit-mcp"))).unwrap();
    // The user ticks one more tool, then Sawit updates the entry again.
    let edited = fs::read_to_string(&t.config).unwrap().replace(
        "\"list_instances\"",
        "\"list_instances\", \"describe_table\"",
    );
    fs::write(&t.config, &edited).unwrap();
    install_file(&t, &Spec::sawit(Path::new("/two/sawit-mcp"))).unwrap();
    assert_eq!(fs::read_to_string(backup_path(&t.config)).unwrap(), edited);
}
