//! Installing and removing by editing config files: what is kept, created, refused and
//! reported.

use super::*;
use crate::mcp_clients::install::has_legacy;

/// A Codex config that doesn't parse says where, never what the line
/// holds: here an MCP server's key. Its status, an install and an
/// uninstall all keep it out of what they say.
#[test]
fn a_broken_toml_line_is_never_quoted() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    fs::write(
            &config,
            "[mcp_servers.exa]\nurl = \"https://mcp.exa.ai/mcp\"\nenv = { EXA_API_KEY = \"exa-sawit-secret-0123\" \n",
        )
        .unwrap();
    let codex = Target {
        client: Client::Codex,
        profile: "default".into(),
        config: config.clone(),
        profile_dir: None,
    };
    let spec = Spec::http("exa", "https://mcp.exa.ai/mcp");
    let Status::Unreadable(why) = status(&codex, &spec) else {
        panic!("{:?}", status(&codex, &spec))
    };
    assert!(why.starts_with("TOML parse error at line "), "{why}");
    assert!(why.contains(", column "), "{why}");
    assert!(!why.contains('\n'), "{why}");
    let install = install_file(&codex, &spec).unwrap_err().to_string();
    let uninstall = uninstall_file(&codex, "exa").unwrap_err().to_string();
    for said in [&why, &install, &uninstall] {
        assert!(!said.contains("exa-sawit-secret"), "{said}");
    }
    assert!(install.contains(&why), "{install}");
    // Nothing was written over it.
    assert!(
        fs::read_to_string(&config)
            .unwrap()
            .contains("exa-sawit-secret")
    );
}

#[test]
fn json_install_keeps_other_settings_and_order() {
    let h = home();
    let targets = discover(h.path(), None);
    let t = find(&targets, Client::ClaudeCode, "default");
    let server = Path::new("/Apps/Sawit.app/Contents/MacOS/sawit-mcp");
    assert_eq!(status(t, &Spec::sawit(server)), Status::NotInstalled);
    install_file(t, &Spec::sawit(server)).unwrap();
    let text = fs::read_to_string(&t.config).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["numStartups"], 3);
    assert_eq!(
        v["mcpServers"]["sawit"]["command"],
        server.display().to_string()
    );
    assert_eq!(v["mcpServers"]["sawit"]["type"], "stdio");
    // Original key order kept, new key appended.
    let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["numStartups", "projects", "mcpServers"]);
    assert_eq!(status(t, &Spec::sawit(server)), Status::Installed);
    // Backed up before the change.
    let backup = fs::read_to_string(backup_path(&t.config)).unwrap();
    assert_eq!(backup, r#"{"numStartups": 3, "projects": {}}"#);
    // And again before the next one.
    let installed = fs::read_to_string(&t.config).unwrap();
    install_file(t, &Spec::sawit(server)).unwrap();
    assert_eq!(
        fs::read_to_string(backup_path(&t.config)).unwrap(),
        installed
    );

    // A different binary is reported, then replaced.
    let other = Path::new("/old/sawit-mcp");
    assert_eq!(
        status(t, &Spec::sawit(other)),
        Status::Outdated(server.display().to_string())
    );

    uninstall_file(t, SERVER_NAME).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&t.config).unwrap()).unwrap();
    assert!(v["mcpServers"].get("sawit").is_none());
    assert_eq!(v["numStartups"], 3);
    assert_eq!(status(t, &Spec::sawit(server)), Status::NotInstalled);
}

#[test]
fn missing_config_files_are_created() {
    let h = home();
    fs::create_dir_all(h.path().join(".cursor")).unwrap();
    let targets = discover(h.path(), None);
    let t = find(&targets, Client::Cursor, "default");
    assert!(!t.config.exists());
    install_file(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
    assert_eq!(
        status(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))),
        Status::Installed
    );
}

#[test]
fn install_replaces_the_pre_rename_entry() {
    let h = home();
    fs::create_dir_all(h.path().join(".cursor")).unwrap();
    fs::write(
        h.path().join(".cursor/mcp.json"),
        r#"{"mcpServers": {"pgdesk": {"command": "/old/pgdesk-mcp"}, "other": {}}}"#,
    )
    .unwrap();
    fs::write(
        h.path().join(".codex/config.toml"),
        "[mcp_servers.pgdesk]\ncommand = \"/old/pgdesk-mcp\"\n",
    )
    .unwrap();
    let targets = discover(h.path(), None);
    for client in [Client::Cursor, Client::Codex] {
        let t = find(&targets, client, "default");
        assert!(has_legacy(t));
        install_file(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
        assert!(!has_legacy(t));
        assert_eq!(
            status(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))),
            Status::Installed
        );
    }
    let text = fs::read_to_string(h.path().join(".cursor/mcp.json")).unwrap();
    assert!(text.contains("\"other\""), "{text}");
}

#[test]
fn codex_toml_keeps_comments_and_other_servers() {
    let h = home();
    let config = h.path().join(".codex/config.toml");
    fs::write(
        &config,
        "# my model\nmodel = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
    )
    .unwrap();
    let targets = discover(h.path(), None);
    let t = find(&targets, Client::Codex, "default");
    install_file(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
    let text = fs::read_to_string(&config).unwrap();
    assert!(text.starts_with("# my model\nmodel = \"o3\""), "{text}");
    assert!(text.contains("[mcp_servers.other]"));
    assert!(
        text.contains("[mcp_servers.sawit]\ncommand = \"/bin/sawit-mcp\""),
        "{text}"
    );
    assert_eq!(
        status(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))),
        Status::Installed
    );
    uninstall_file(t, SERVER_NAME).unwrap();
    let text = fs::read_to_string(&config).unwrap();
    assert!(!text.contains("sawit"), "{text}");
    assert!(text.contains("[mcp_servers.other]"));
}

#[test]
fn commented_json_is_left_alone() {
    let h = home();
    let config = h.path().join(".config/zed/settings.json");
    let original = "// Zed settings\n{\n  \"theme\": \"One Dark\", // mine\n}\n";
    fs::write(&config, original).unwrap();
    let targets = discover(h.path(), None);
    let t = find(&targets, Client::Zed, "default");
    assert!(matches!(
        status(t, &Spec::sawit(Path::new("/bin/x"))),
        Status::Unreadable(_)
    ));
    let err = install_file(t, &Spec::sawit(Path::new("/bin/x"))).unwrap_err();
    assert!(err.to_string().contains("manual"), "{err}");
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    assert!(snippet(t, &Spec::sawit(Path::new("/bin/x"))).contains("context_servers"));
}

#[test]
fn amp_uses_a_flat_dotted_key() {
    let h = home();
    fs::create_dir_all(h.path().join(".config/amp")).unwrap();
    let targets = discover(h.path(), None);
    let t = find(&targets, Client::Amp, "default");
    install_file(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&t.config).unwrap()).unwrap();
    assert_eq!(v["amp.mcpServers"]["sawit"]["command"], "/bin/sawit-mcp");
    assert!(v.get("amp").is_none());
}

#[test]
fn yaml_and_script_configs_are_manual() {
    let h = home();
    let p = h.path();
    fs::create_dir_all(p.join(".config/goose")).unwrap();
    fs::write(p.join(".config/goose/config.yaml"), "GOOSE_MODEL: x\n").unwrap();
    fs::create_dir_all(p.join(".continue")).unwrap();
    let targets = discover(p, None);
    let server = Path::new("/bin/sawit-mcp");
    let goose = find(&targets, Client::Goose, "default");
    assert!(!goose.client.editable());
    assert!(matches!(
        status(goose, &Spec::sawit(server)),
        Status::Manual
    ));
    assert!(install_file(goose, &Spec::sawit(server)).is_err());
    assert_eq!(
        fs::read_to_string(&goose.config).unwrap(),
        "GOOSE_MODEL: x\n"
    );
    assert!(snippet(goose, &Spec::sawit(server)).contains("cmd: /bin/sawit-mcp"));
    // Pasted in by hand: reported as installed.
    fs::write(&goose.config, snippet(goose, &Spec::sawit(server))).unwrap();
    assert_eq!(status(goose, &Spec::sawit(server)), Status::Installed);
    let cont = find(&targets, Client::Continue, "default");
    assert!(snippet(cont, &Spec::sawit(server)).contains("- name: sawit"));
}

#[test]
fn remote_servers_install_and_report_status() {
    let h = home();
    let targets = discover(h.path(), None);
    let spec = Spec::http("aws", "https://aws-mcp.us-east-1.api.aws/mcp");
    for client in [Client::ClaudeCode, Client::ClaudeDesktop, Client::Codex] {
        let t = find(&targets, client, "default");
        assert_eq!(status(t, &spec), Status::NotInstalled);
        install_file(t, &spec).unwrap();
        assert_eq!(status(t, &spec), Status::Installed, "{client:?}");
        // Another region is the same server pointing elsewhere.
        let other = Spec::http("aws", "https://aws-mcp.eu-west-1.api.aws/mcp");
        assert!(
            matches!(status(t, &other), Status::Outdated(_)),
            "{client:?}"
        );
        uninstall_file(t, "aws").unwrap();
        assert_eq!(status(t, &spec), Status::NotInstalled);
    }
    let codex = find(&targets, Client::Codex, "default");
    let keyed = Spec::http("es", "https://es.dev/mcp").header("Authorization", "Bearer ${ES_KEY}");
    install_file(codex, &keyed).unwrap();
    let text = fs::read_to_string(&codex.config).unwrap();
    assert!(text.contains("url = \"https://es.dev/mcp\""), "{text}");
    assert!(text.contains("bearer_token_env_var = \"ES_KEY\""), "{text}");
    // A local server added by hand with a bare `npx` is still recognised.
    let claude = find(&targets, Client::ClaudeCode, "default");
    install_file(
        claude,
        &Spec::stdio(
            "rea",
            "npx",
            vec!["-y".into(), "rea-agents@6.3.0".into(), "mcp".into()],
        ),
    )
    .unwrap();
    let resolved = Spec::stdio(
        "rea",
        "/opt/homebrew/bin/npx",
        vec!["-y".into(), "rea-agents@6.3.0".into(), "mcp".into()],
    );
    assert_eq!(status(claude, &resolved), Status::Installed);
}

#[test]
fn removing_a_server_keeps_the_others_in_order_and_the_files_indent() {
    let h = home();
    fs::create_dir_all(h.path().join(".cursor")).unwrap();
    let config = h.path().join(".cursor/mcp.json");
    fs::write(
            &config,
            "{\n    \"mcpServers\": {\n        \"a\": {\"command\": \"a\"},\n        \"b\": {\"command\": \"b\"},\n        \"c\": {\"command\": \"c\"}\n    }\n}\n",
        )
        .unwrap();
    let t = find(&discover(h.path(), None), Client::Cursor, "default").clone();
    uninstall_file(&t, "a").unwrap();
    let text = fs::read_to_string(&config).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let names: Vec<_> = v["mcpServers"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(names, ["b", "c"]);
    assert!(
        text.starts_with("{\n    \"mcpServers\": {\n        \"b\""),
        "{text}"
    );
    // The pre-rename entry goes the same way.
    fs::write(
        &config,
        r#"{"mcpServers": {"pgdesk": {"command": "/old/pgdesk-mcp"}, "x": {}, "y": {}}}"#,
    )
    .unwrap();
    install_file(&t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
    let v = json_at(&config);
    let names: Vec<_> = v["mcpServers"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(names, ["x", "y", "sawit"]);
    // A file with tabs keeps them.
    fs::write(&config, "{\n\t\"mcpServers\": {}\n}\n").unwrap();
    install_file(&t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
    let text = fs::read_to_string(&config).unwrap();
    assert!(
        text.starts_with("{\n\t\"mcpServers\": {\n\t\t\"sawit\""),
        "{text}"
    );
}

/// The MCP page and an agent's `install_mcp_server` (another process)
/// can change one config at once: neither loses the other's entry, and
/// the file never ends up half one write and half the other.
#[test]
fn two_writers_to_one_config_lose_nothing() {
    let h = home();
    fs::create_dir_all(h.path().join(".cursor")).unwrap();
    let t = find(&discover(h.path(), None), Client::Cursor, "default").clone();
    std::thread::scope(|scope| {
        for name in ["one", "two"] {
            let t = &t;
            scope.spawn(move || {
                for i in 0..150 {
                    let spec = Spec::stdio(name, format!("/bin/{name}-{i}"), vec![]);
                    install_file(t, &spec).unwrap_or_else(|e| panic!("{name} {i}: {e}"));
                }
            });
        }
    });
    let v = json_at(&t.config);
    assert_eq!(v["mcpServers"]["one"]["command"], "/bin/one-149");
    assert_eq!(v["mcpServers"]["two"]["command"], "/bin/two-149");
}
