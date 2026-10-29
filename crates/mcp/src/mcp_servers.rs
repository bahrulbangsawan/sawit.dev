//! The MCP page: the servers Sawit offers (its own `sawit-mcp` and the
//! catalog), the AI clients found on this Mac, and installing or removing a
//! server in a client's config.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::mcp_catalog::{self, Auth, DEFAULT_AWS_REGION, SERVERS, Server};
use sawit_core::mcp_clients::{self, Spec, Status, Target};
use sawit_core::probe::find_in_path;

use crate::{Sawit, blocking, fail, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ServerArgs {
    #[schemars(description = "Server id from list_mcp_servers, e.g. \"sawit\"")]
    pub server: String,
    #[schemars(
        description = "Client name from list_mcp_servers, e.g. \"Claude Code\" or \"cursor\""
    )]
    pub client: String,
    #[schemars(
        description = "Profile of Claude Code or Codex (default: \"default\"); see list_mcp_servers"
    )]
    pub profile: Option<String>,
    #[schemars(description = "AWS region for servers whose URL has one (default us-east-1)")]
    pub region: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UninstallArgs {
    #[schemars(description = "Server id to remove from the client's config")]
    pub server: String,
    #[schemars(description = "Client name from list_mcp_servers")]
    pub client: String,
    #[schemars(description = "Profile of Claude Code or Codex (default: \"default\")")]
    pub profile: Option<String>,
    #[schemars(description = "Must equal `server` exactly, to confirm")]
    pub confirm: String,
}

/// "Claude Code", "claude-code" and "claudecode" are the same client.
fn same_name(a: &str, b: &str) -> bool {
    let key = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    key(a) == key(b)
}

/// How a target is named to agents: the client, plus a non-default profile.
fn target_label(t: &Target) -> String {
    if t.profile == "default" {
        t.client.name().to_string()
    } else {
        format!("{} (profile {})", t.client.name(), t.profile)
    }
}

fn status_text(status: &Status) -> String {
    match status {
        Status::Installed => "installed".into(),
        Status::Outdated(_) => "installed, pointing elsewhere (install again to update)".into(),
        Status::NotInstalled => "not installed".into(),
        Status::Unreadable(e) => format!("config unreadable: {e}"),
        Status::Manual => "add by hand (YAML/script config)".into(),
    }
}

fn auth_text(auth: Auth) -> Option<String> {
    match auth {
        Auth::None => None,
        Auth::Browser => Some("signs in through the browser from the client".into()),
        Auth::EnvKey(var) => Some(format!("needs {var} set before starting the client")),
        Auth::Program => Some("uses the program's own sign-in".into()),
    }
}

/// This `sawit-mcp`: what the bundled entry points at.
fn own_server() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sawit-mcp"))
}

fn targets() -> Vec<Target> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    mcp_clients::discover(&home, claude_dir)
}

fn spec(server: &Server, region: &str) -> Spec {
    server.spec(&own_server(), region, find_in_path)
}

fn pick(
    server: &str,
    client: &str,
    profile: Option<&str>,
) -> Result<(&'static Server, Target), String> {
    let entry = mcp_catalog::get(server)
        .ok_or_else(|| format!("Unknown server \"{server}\". See list_mcp_servers."))?;
    let profile = profile.unwrap_or("default");
    let target = targets()
        .into_iter()
        .find(|t| same_name(t.client.name(), client) && t.profile == profile)
        .ok_or_else(|| {
            format!("No client \"{client}\" with profile \"{profile}\" on this Mac. See list_mcp_servers.")
        })?;
    Ok((entry, target))
}

impl Sawit {
    pub async fn list_mcp_servers_tool(&self) -> CallToolResult {
        let text = tri!(
            blocking(|| {
                let targets = targets();
                let mut out = vec![format!(
                    "Clients found: {}",
                    targets
                        .iter()
                        .map(target_label)
                        .collect::<Vec<_>>()
                        .join(", ")
                )];
                for server in SERVERS {
                    out.push(String::new());
                    out.push(format!(
                        "{} ({}) · {}",
                        server.id,
                        server.name,
                        server.address(DEFAULT_AWS_REGION)
                    ));
                    out.push(format!("  {}", server.about));
                    if let Some(auth) = auth_text(server.auth) {
                        out.push(format!("  auth: {auth}"));
                    }
                    if let Some(note) = server.note {
                        out.push(format!("  note: {note}"));
                    }
                    if let Some(program) = server.missing_program(find_in_path) {
                        out.push(format!("  missing program: {program} is not installed"));
                    }
                    let spec = spec(server, DEFAULT_AWS_REGION);
                    let installed: Vec<String> = targets
                        .iter()
                        .filter_map(|t| match mcp_clients::status(t, &spec) {
                            Status::NotInstalled => None,
                            s => Some(format!("{} ({})", target_label(t), status_text(&s))),
                        })
                        .collect();
                    out.push(format!(
                        "  in: {}",
                        if installed.is_empty() {
                            "no client yet".into()
                        } else {
                            installed.join(", ")
                        }
                    ));
                }
                out.join("\n")
            })
            .await
        );
        ok(text)
    }

    pub async fn install_mcp_server_tool(&self, a: ServerArgs) -> CallToolResult {
        let (server, target) = tri!(pick(&a.server, &a.client, a.profile.as_deref()));
        let region = a.region.as_deref().unwrap_or(DEFAULT_AWS_REGION);
        let spec = spec(server, region);
        if !target.client.editable() {
            return ok(format!(
                "{} keeps its config in a format Sawit doesn't edit. Add this to {}:\n\n{}",
                target.client.name(),
                target.config.display(),
                mcp_clients::snippet(&target, &spec)
            ));
        }
        let cli = target.client.cli().and_then(find_in_path);
        if let Err(e) = mcp_clients::install(&target, &spec, cli.as_deref()).await {
            return fail(e.to_string());
        }
        let mut text = format!(
            "Installed {} in {} ({}): {}.",
            server.id,
            target_label(&target),
            target.config.display(),
            status_text(&mcp_clients::status(&target, &spec))
        );
        if let Some(auth) = auth_text(server.auth) {
            text.push_str(&format!(" It {auth}."));
        }
        text.push_str(" Restart the client (or reload its MCP servers) to use it.");
        ok(text)
    }

    pub async fn uninstall_mcp_server_tool(&self, a: UninstallArgs) -> CallToolResult {
        if a.confirm != a.server {
            return fail(format!(
                "Refused: to remove “{}”, pass confirm=\"{}\".",
                a.server, a.server
            ));
        }
        let (server, target) = tri!(pick(&a.server, &a.client, a.profile.as_deref()));
        if mcp_clients::status(&target, &spec(server, DEFAULT_AWS_REGION)) == Status::NotInstalled {
            return ok(format!(
                "{} is not installed in {}; nothing to remove.",
                server.id,
                target_label(&target)
            ));
        }
        let cli = target.client.cli().and_then(find_in_path);
        match mcp_clients::uninstall(&target, server.id, cli.as_deref()).await {
            Ok(()) => ok(format!(
                "Removed {} from {}.",
                server.id,
                target_label(&target)
            )),
            Err(e) => fail(e.to_string()),
        }
    }
}

#[tool_router(router = mcp_servers_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List the MCP servers Sawit's MCP page offers (Sawit itself and the catalog: URL or command, sign-in, notes) and, for each, the AI clients on this Mac (Claude Code and Codex per profile, Cursor, VS Code, …) that have it installed. Works without the app"
    )]
    async fn list_mcp_servers(&self) -> CallToolResult {
        self.list_mcp_servers_tool().await
    }

    #[tool(
        description = "Install (or update) an MCP server from list_mcp_servers in one AI client's config, through the client's own CLI when it has one. Edits that client's config file (a backup is kept). Works without the app"
    )]
    async fn install_mcp_server(&self, Parameters(a): Parameters<ServerArgs>) -> CallToolResult {
        self.install_mcp_server_tool(a).await
    }

    #[tool(
        description = "Remove an MCP server from one AI client's config. DESTRUCTIVE for that client's setup: `confirm` must equal `server`. Works without the app"
    )]
    async fn uninstall_mcp_server(
        &self,
        Parameters(a): Parameters<UninstallArgs>,
    ) -> CallToolResult {
        self.uninstall_mcp_server_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_names_match_loosely() {
        assert!(same_name("Claude Code", "claude-code"));
        assert!(same_name("VS Code", "vscode"));
        assert!(!same_name("VS Code", "VS Code Insiders"));
    }

    #[test]
    fn every_catalog_server_has_an_id_and_known_auth_text() {
        for s in SERVERS {
            assert!(!s.id.is_empty());
            assert!(mcp_catalog::get(s.id).is_some());
            let _ = auth_text(s.auth);
        }
        assert!(auth_text(Auth::EnvKey("X_KEY")).unwrap().contains("X_KEY"));
    }
}
