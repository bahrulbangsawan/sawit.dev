//! The entry a server gets in each client's config: its JSON shape, how GitHub's server is
//! handed Sawit's sign-in ([`Handover`]), the `PATH` an app without the shell's is told, and
//! Codex's TOML table.

use std::path::Path;

use serde_json::{Value, json};

use super::{Client, Spec, Transport};

/// How a client gets Sawit's GitHub sign-in for a server that takes it ([`Spec::token_helper`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handover {
    /// The client runs `sawit github token --header` for the request headers each time it
    /// connects: Claude Code's `headersHelper`, Codex's `http_headers_helper`.
    Helper,
    /// The client starts `sawit github token --exec`, which runs the `mcp-remote` bridge with the
    /// header in its environment: clients that only run programs, and those whose remote support
    /// has no header helper (Cursor, Zed, …) and that GitHub doesn't offer its sign-in to.
    Bridge,
    /// The client signs in to GitHub itself, with its own GitHub account: VS Code (its built-in
    /// GitHub sign-in) and GitHub Copilot CLI, the hosts GitHub's server offers OAuth to.
    ClientSignIn,
}

/// How `client` gets the sign-in for a server that takes Sawit's ([`Handover`]).
pub fn handover(client: Client) -> Handover {
    match client {
        Client::ClaudeCode | Client::Codex => Handover::Helper,
        Client::VsCode | Client::VsCodeInsiders | Client::CopilotCli => Handover::ClientSignIn,
        _ => Handover::Bridge,
    }
}

/// Whether `client` reaches the remote server `spec` through the `mcp-remote` bridge: one that
/// only runs programs, or, for a server that takes Sawit's GitHub sign-in, one it is handed to
/// through the bridge ([`Handover::Bridge`]).
pub fn bridged(client: Client, spec: &Spec) -> bool {
    spec.is_remote()
        && match spec.token_helper {
            Some(_) => handover(client) == Handover::Bridge,
            None => needs_bridge(client),
        }
}

/// What Claude Code's and Codex's entries hold once Sawit hands them its GitHub sign-in, and the
/// bridge's arguments: an entry that holds it runs some `sawit`'s helper.
pub(super) const HELPER_MARK: &str = " github token ";

/// The key holding the servers in a JSON client config.
pub(super) fn servers_key(client: Client) -> &'static str {
    match client {
        Client::Zed => "context_servers",
        Client::OpenCode | Client::KiloCode => "mcp",
        Client::VsCode | Client::VsCodeInsiders => "servers",
        // A flat, dotted key in Amp's settings.
        Client::Amp => "amp.mcpServers",
        _ => "mcpServers",
    }
}

fn object(pairs: &[(String, String)]) -> Value {
    Value::Object(
        pairs
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
    )
}

/// A remote server in the client's own format, or `None` when the client
/// only runs local programs (it gets the `mcp-remote` bridge instead).
fn http_entry(client: Client, url: &str, headers: &[(String, String)]) -> Option<Value> {
    let mut v = match client {
        Client::ClaudeCode | Client::VsCode | Client::VsCodeInsiders => {
            json!({"type": "http", "url": url})
        }
        Client::Cursor | Client::Amp | Client::Zed => json!({"url": url}),
        Client::Gemini | Client::Qwen => json!({"httpUrl": url}),
        Client::Windsurf | Client::Antigravity => json!({"serverUrl": url}),
        Client::OpenCode | Client::KiloCode => {
            json!({"type": "remote", "url": url, "enabled": true})
        }
        Client::CopilotCli => json!({"type": "http", "url": url, "tools": ["*"]}),
        Client::FactoryDroid => json!({"type": "http", "url": url, "disabled": false}),
        Client::Kiro => json!({"url": url, "disabled": false}),
        Client::Cline => json!({"type": "streamableHttp", "url": url, "disabled": false}),
        Client::RooCode => json!({"type": "streamable-http", "url": url}),
        _ => return None,
    };
    if !headers.is_empty() {
        v["headers"] = object(headers);
    }
    Some(v)
}

/// Whether `client` reaches a remote server only through the `mcp-remote`
/// bridge, which `npx` runs: it only runs programs.
pub fn needs_bridge(client: Client) -> bool {
    client != Client::Codex && http_entry(client, "", &[]).is_none()
}

/// Where the server entry lives in a JSON client config, and its value. A server that takes
/// Sawit's GitHub sign-in goes to Claude Code with a `headersHelper`, to the clients that sign in
/// to GitHub themselves as a bare URL, and to the others through the bridge ([`handover`]).
pub fn entry(client: Client, spec: &Spec) -> (&'static str, Value) {
    let key = servers_key(client);
    if let Transport::Http { url, headers } = &spec.transport {
        match &spec.token_helper {
            Some(sawit) if handover(client) == Handover::Helper => {
                if let Some(mut v) = http_entry(client, url, &[]) {
                    v["headersHelper"] = Value::String(crate::github_token::headers_helper(sawit));
                    return (key, v);
                }
            }
            Some(_) if handover(client) == Handover::ClientSignIn => {
                if let Some(v) = http_entry(client, url, &[]) {
                    return (key, v);
                }
            }
            Some(_) => {}
            None => {
                if let Some(v) = http_entry(client, url, headers) {
                    return (key, v);
                }
            }
        }
    }
    let (cmd, args, mut env) = spec.as_stdio();
    // An app started with launchd's PATH: npx's `#!/usr/bin/env node` needs
    // to be told where node is. Not the clients that have the login shell's
    // PATH: `env` goes over it, and would hide the rest of it from the server.
    if client.without_shell_path()
        && spec.starts_runner(&cmd)
        && let Some(path) = node_path(&spec.npx, spec.node_dir.as_deref())
        && !env.iter().any(|(k, _)| k == "PATH")
    {
        env.push(("PATH".into(), path));
    }
    let env = object(&env);
    let v = match client {
        Client::ClaudeCode => json!({"type": "stdio", "command": cmd, "args": args, "env": env}),
        Client::Zed => json!({"command": cmd, "args": args, "env": env}),
        Client::OpenCode | Client::KiloCode => {
            let mut command = vec![cmd];
            command.extend(args);
            json!({"type": "local", "command": command, "enabled": true})
        }
        Client::VsCode | Client::VsCodeInsiders => {
            json!({"type": "stdio", "command": cmd, "args": args})
        }
        Client::CopilotCli => {
            json!({"type": "local", "command": cmd, "args": args, "env": env, "tools": ["*"]})
        }
        Client::FactoryDroid => {
            json!({"type": "stdio", "command": cmd, "args": args, "disabled": false})
        }
        Client::Cursor => json!({"type": "stdio", "command": cmd, "args": args}),
        Client::Kiro => json!({"command": cmd, "args": args, "disabled": false}),
        Client::Cline => {
            json!({"command": cmd, "args": args, "disabled": false, "autoApprove": []})
        }
        Client::RooCode => {
            json!({"command": cmd, "args": args, "disabled": false, "alwaysAllow": []})
        }
        _ => json!({"command": cmd, "args": args}),
    };
    let mut v = v;
    if let (Some(obj), Value::Object(env)) = (v.as_object_mut(), &env)
        && !env.is_empty()
        && !obj.contains_key("env")
    {
        // opencode's and Kilo's local servers take theirs as `environment`.
        let name = if obj.contains_key("args") {
            "env"
        } else {
            "environment"
        };
        obj.insert(name.into(), Value::Object(env.clone()));
    }
    (key, v)
}

/// Folders launchd's `PATH` has, or Homebrew's.
const USUAL: [&str; 6] = [
    "/usr/local/bin",
    "/opt/homebrew/bin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];

/// The `PATH` an app started with launchd's ([`Client::without_shell_path`])
/// must give the runner at `program` (`npx`, `bunx`, `pnpm`), so that a
/// package's `#!/usr/bin/env node` finds a node: the runner's folder (nvm
/// keeps node beside npx), then `node_dir` when it is another one (nvm's node
/// for bunx), then the usual ones. `None` for a bare program, or one in a
/// folder launchd's PATH has.
pub(super) fn node_path(program: &str, node_dir: Option<&str>) -> Option<String> {
    let dir = Path::new(program).parent().filter(|d| d.is_absolute())?;
    let dir = dir.to_str()?;
    if matches!(dir, "/usr/bin" | "/bin") {
        return None;
    }
    let node = node_dir.filter(|n| Path::new(n).is_absolute() && *n != dir && !USUAL.contains(n));
    let mut parts = vec![dir];
    parts.extend(node);
    parts.extend(USUAL.iter().filter(|d| **d != dir));
    Some(parts.join(":"))
}

/// Whether `path` is one [`node_path`] works out for `program`, with or
/// without a node folder: Sawit's, not the user's.
pub(super) fn own_path(path: &str, program: &str) -> bool {
    let node = path.split(':').nth(1);
    node_path(program, None).as_deref() == Some(path)
        || node_path(program, node).as_deref() == Some(path)
}

fn toml_strings<'a>(items: impl IntoIterator<Item = &'a String>) -> toml_edit::Array {
    items.into_iter().map(|s| s.as_str()).collect()
}

fn toml_table(pairs: &[(String, String)]) -> toml_edit::InlineTable {
    pairs
        .iter()
        .map(|(k, v)| (k.as_str(), toml_edit::Value::from(v.as_str())))
        .collect()
}

/// Codex's `[mcp_servers.<name>]` table.
pub(super) fn codex_table(spec: &Spec) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    match &spec.transport {
        Transport::Stdio { command, args, env } => {
            table["command"] = toml_edit::value(command.as_str());
            table["args"] = toml_edit::value(toml_strings(args));
            if !env.is_empty() {
                table["env"] = toml_edit::value(toml_table(env));
            }
        }
        Transport::Http { url, headers } => {
            table["url"] = toml_edit::value(url.as_str());
            if let Some(sawit) = &spec.token_helper {
                // Codex runs it (`sh -c`) for the headers each time it connects.
                table["http_headers_helper"] =
                    toml_edit::value(crate::github_token::headers_helper(sawit));
            } else if let Some(var) = spec.bearer_env() {
                table["bearer_token_env_var"] = toml_edit::value(var);
            } else if !headers.is_empty() {
                table["http_headers"] = toml_edit::value(toml_table(headers));
            }
        }
    }
    table
}
