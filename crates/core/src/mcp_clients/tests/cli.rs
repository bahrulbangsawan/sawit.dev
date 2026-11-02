//! Changing a config through the client's CLI: the profile and `PATH` it runs with, and the
//! old entry put back when it fails.

use std::path::PathBuf;

use serde_json::json;

use super::*;
use crate::mcp_clients::cli::cli_call_with;

#[test]
fn cli_targets_the_right_profile() {
    let h = home();
    let targets = discover(h.path(), None);
    let server = Path::new("/bin/sawit-mcp");
    let default = find(&targets, Client::ClaudeCode, "default");
    let inv = cli_add(default, &Spec::sawit(server), Path::new("/usr/bin/claude")).unwrap();
    assert_eq!(
        inv.args,
        [
            "mcp",
            "add",
            "--scope",
            "user",
            "sawit",
            "--",
            "/bin/sawit-mcp"
        ]
    );
    // The default profile must not inherit the app's CLAUDE_CONFIG_DIR.
    assert_eq!(inv.env, vec![("CLAUDE_CONFIG_DIR".to_string(), None)]);
    let work = find(&targets, Client::ClaudeCode, "work");
    let inv = cli_add(work, &Spec::sawit(server), Path::new("/usr/bin/claude")).unwrap();
    assert_eq!(
        inv.env,
        vec![(
            "CLAUDE_CONFIG_DIR".to_string(),
            Some(h.path().join(".claude-work"))
        )]
    );
    let codex = find(&targets, Client::Codex, "default");
    let inv = cli_add(codex, &Spec::sawit(server), Path::new("/usr/bin/codex")).unwrap();
    assert_eq!(inv.args, ["mcp", "add", "sawit", "--", "/bin/sawit-mcp"]);
    assert_eq!(inv.env, vec![("CODEX_HOME".to_string(), None)]);
    // Clients without a CLI path aren't driven by one.
    let desktop = find(&targets, Client::ClaudeDesktop, "default");
    assert!(cli_add(desktop, &Spec::sawit(server), Path::new("/x")).is_none());
}

#[tokio::test]
async fn cli_runs_with_the_profile_env_and_never_inherits_it() {
    use std::os::unix::fs::PermissionsExt;
    let h = home();
    let log = h.path().join("calls.log");
    let cli = h.path().join("claude");
    fs::write(
        &cli,
        format!(
            "#!/bin/sh\necho \"${{CLAUDE_CONFIG_DIR:-unset}} $*\" >> {}\n",
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    let targets = discover(h.path(), None);
    let server = Path::new("/bin/sawit-mcp");
    // SAFETY: test-only; mimics the app launched from a profile shell.
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", "/somewhere/else") };
    install(
        find(&targets, Client::ClaudeCode, "default"),
        &Spec::sawit(server),
        Some(&cli),
    )
    .await
    .unwrap();
    install(
        find(&targets, Client::ClaudeCode, "work"),
        &Spec::sawit(server),
        Some(&cli),
    )
    .await
    .unwrap();
    let calls = fs::read_to_string(&log).unwrap();
    let lines: Vec<_> = calls.lines().collect();
    assert_eq!(
        lines[0],
        "unset mcp add --scope user sawit -- /bin/sawit-mcp"
    );
    assert_eq!(
        lines[1],
        format!(
            "{} mcp add --scope user sawit -- /bin/sawit-mcp",
            h.path().join(".claude-work").display()
        )
    );
}

/// A stand-in for `claude` whose `mcp remove` writes `removed` to
/// `config` and whose `mcp add` runs `add`.
fn fake_cli(dir: &Path, config: &Path, removed: &str, add: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let cli = dir.join("claude");
    fs::write(
            &cli,
            format!(
                "#!/bin/sh\ncase \"$2\" in\n  remove) printf '%s' '{removed}' > '{}' ;;\n  add) {add} ;;\nesac\n",
                config.display()
            ),
        )
        .unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    cli
}

const CLAUDE_WITH_SAWIT: &str = r#"{"numStartups": 3, "mcpServers": {"sawit": {"type": "stdio", "command": "/old/sawit-mcp", "args": [], "env": {"SAWIT_HOME": "/x"}}}}"#;

#[tokio::test]
async fn a_failed_cli_update_puts_the_old_entry_back() {
    let h = home();
    let t = find(&discover(h.path(), None), Client::ClaudeCode, "work").clone();
    fs::write(&t.config, CLAUDE_WITH_SAWIT).unwrap();
    let cli = fake_cli(
        h.path(),
        &t.config,
        r#"{"numStartups": 3, "mcpServers": {}}"#,
        "echo boom >&2; exit 1",
    );
    let err = install(&t, &Spec::sawit(Path::new("/new/sawit-mcp")), Some(&cli))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("boom"), "{err}");
    assert_eq!(
        json_at(&t.config),
        serde_json::from_str::<Value>(CLAUDE_WITH_SAWIT).unwrap()
    );
}

#[tokio::test]
async fn a_cli_update_stopped_halfway_puts_the_old_entry_back() {
    let h = home();
    let t = find(&discover(h.path(), None), Client::ClaudeCode, "work").clone();
    fs::write(&t.config, CLAUDE_WITH_SAWIT).unwrap();
    let cli = fake_cli(
        h.path(),
        &t.config,
        r#"{"numStartups": 3, "mcpServers": {}}"#,
        "exec sleep 30",
    );
    let spec = Spec::sawit(Path::new("/new/sawit-mcp"));
    // Stop: the future is dropped while `mcp add` runs.
    let stopped = tokio::time::timeout(
        std::time::Duration::from_millis(1500),
        install(&t, &spec, Some(&cli)),
    )
    .await;
    assert!(stopped.is_err(), "the fake add didn't hang");
    assert_eq!(
        json_at(&t.config),
        serde_json::from_str::<Value>(CLAUDE_WITH_SAWIT).unwrap()
    );
}

#[tokio::test]
async fn a_cli_update_keeps_the_users_own_keys() {
    let h = home();
    let t = find(&discover(h.path(), None), Client::ClaudeCode, "work").clone();
    fs::write(&t.config, CLAUDE_WITH_SAWIT).unwrap();
    let added = r#"{"numStartups": 3, "mcpServers": {"sawit": {"type": "stdio", "command": "/new/sawit-mcp", "args": [], "env": {}}}}"#;
    let cli = fake_cli(
        h.path(),
        &t.config,
        r#"{"numStartups": 3, "mcpServers": {}}"#,
        &format!("printf '%s' '{added}' > '{}'", t.config.display()),
    );
    install(&t, &Spec::sawit(Path::new("/new/sawit-mcp")), Some(&cli))
        .await
        .unwrap();
    let entry = json_at(&t.config)["mcpServers"]["sawit"].clone();
    assert_eq!(entry["command"], "/new/sawit-mcp");
    assert_eq!(entry["env"], json!({"SAWIT_HOME": "/x"}));
}

/// A client CLI installed with npm (`#!/usr/bin/env node`) runs with
/// its own folder on `PATH`, where nvm keeps the `node` it needs.
#[test]
fn a_cli_runs_with_its_own_folder_on_path() {
    let h = home();
    let t = find(&discover(h.path(), None), Client::ClaudeCode, "default").clone();
    let cli = h.path().join(".nvm/versions/node/v22.11.0/bin/claude");
    let call = cli_add(&t, &Spec::sawit(Path::new("/bin/sawit-mcp")), &cli).unwrap();
    let path = call
        .env
        .iter()
        .find(|(k, _)| k == "PATH")
        .and_then(|(_, v)| v.clone())
        .expect("no PATH");
    assert!(
        path.to_string_lossy()
            .starts_with(&format!("{}:", cli.parent().unwrap().display())),
        "{}",
        path.display()
    );
}

/// CATALOG M3: a CLI found outside nvm's folders whose `#!/usr/bin/env node` needs nvm's
/// node (Claude Code's old `~/.claude/local/claude`, `~/.npm-global/bin`, `~/.bun/bin`) runs
/// with that node's folder on `PATH` too. With only its own folder there, every install
/// through it failed ("env: node: No such file or directory"), and so did its sign-in checks.
#[tokio::test]
async fn a_cli_finds_the_node_nvm_keeps_elsewhere() {
    use std::os::unix::fs::PermissionsExt;
    let h = home();
    let t = find(&discover(h.path(), None), Client::ClaudeCode, "default").clone();
    let program = |path: &Path, text: &str| {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    let cli = h.path().join(".claude/local/claude");
    program(&cli, "#!/usr/bin/env node\n");
    let node = h.path().join(".nvm/versions/node/v22.11.0/bin/node");
    program(&node, "#!/bin/sh\necho \"node ran $2 $3\"\n");
    // The PATH of an app opened from Finder: no nvm.
    let path = std::ffi::OsString::from("/usr/bin:/bin");
    let args = vec!["mcp".to_string(), "list".to_string()];
    let call = cli_call_with(&t, &cli, args, Some(&node), Some(&path)).unwrap();
    let out = run_cli(&call, std::time::Duration::from_secs(10))
        .await
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "node ran mcp list\n");
}

#[test]
fn remote_servers_go_through_the_cli() {
    let h = home();
    let targets = discover(h.path(), None);
    let spec = Spec::http("es", "https://es.dev/mcp").header("Authorization", "Bearer ${ES_KEY}");
    let claude = find(&targets, Client::ClaudeCode, "work");
    let call = cli_add(claude, &spec, Path::new("/bin/claude")).unwrap();
    assert_eq!(
        call.args,
        [
            "mcp",
            "add",
            "--scope",
            "user",
            "--transport",
            "http",
            "es",
            "https://es.dev/mcp",
            "--header",
            "Authorization: Bearer ${ES_KEY}"
        ]
    );
    let codex = find(&targets, Client::Codex, "default");
    let call = cli_add(codex, &spec, Path::new("/bin/codex")).unwrap();
    assert_eq!(
        call.args,
        [
            "mcp",
            "add",
            "es",
            "--url",
            "https://es.dev/mcp",
            "--bearer-token-env-var",
            "ES_KEY"
        ]
    );
    // Codex's CLI can't take other headers: the file is edited instead.
    let custom = Spec::http("x", "https://x.dev/mcp").header("X-Team", "a");
    assert!(cli_add(codex, &custom, Path::new("/bin/codex")).is_none());
}
