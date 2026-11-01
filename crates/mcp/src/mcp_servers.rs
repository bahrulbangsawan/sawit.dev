//! The MCP page: the servers Sawit offers (its own `sawit-mcp` and the
//! catalog), the AI clients found on this Mac, and installing or removing a
//! server in a client's config.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::dev_tools::find_program;
use sawit_core::mcp_catalog::{self, Auth, DEFAULT_AWS_REGION, Server};
use sawit_core::mcp_clients::{self, Spec, Status, Target};
use sawit_core::package_manager::{self, PackageManager, Use};

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
    #[schemars(
        description = "AWS Region for servers whose URL has one, like eu-west-1 (default: the one the client already has it in, else us-east-1)"
    )]
    pub region: Option<String>,
    #[schemars(
        description = "The package manager that runs a local server's package and the mcp-remote bridge: \"npm\" (npx -y pkg), \"pnpm\" (pnpm dlx pkg), \"bun\" (bunx pkg) or \"yarn\" (yarn dlx pkg, Yarn 2+). Written into the client's config. Default: the one picked last in Sawit, else npx"
    )]
    pub package_manager: Option<String>,
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

fn status_text(server: &Server, status: &Status) -> String {
    match status {
        Status::Installed => "installed".into(),
        Status::Outdated(sig) if let Some(region) = server.region_in(sig) => {
            format!("installed in region {region}")
        }
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

fn spec(server: &Server, region: &str) -> Spec {
    server.spec(&own_server(), region, find_program)
}

/// The Region to install a regional server in: the one given, which must
/// be an AWS Region (it goes into the URL's host), else the one the client
/// already has it in, else the default.
fn region_for(server: &Server, target: &Target, given: Option<&str>) -> Result<String, String> {
    match given {
        Some(region) if mcp_catalog::valid_region(region) => Ok(region.to_string()),
        Some(region) => Err(format!(
            "\"{region}\" isn't an AWS Region (like us-east-1 or eu-west-1)."
        )),
        None => Ok(
            match mcp_clients::status(target, &spec(server, DEFAULT_AWS_REGION)) {
                Status::Outdated(sig) => server.region_in(&sig),
                _ => None,
            }
            .unwrap_or_else(|| DEFAULT_AWS_REGION.to_string()),
        ),
    }
}

fn pick(
    server: &str,
    client: &str,
    profile: Option<&str>,
) -> Result<(&'static Server, Target), String> {
    let entry = mcp_catalog::get(server)
        .ok_or_else(|| format!("Unknown server \"{server}\". See list_mcp_servers."))?;
    let profile = profile.unwrap_or("default");
    let target = mcp_clients::targets()
        .into_iter()
        .find(|t| same_name(t.client.name(), client) && t.profile == profile)
        .ok_or_else(|| {
            format!("No client \"{client}\" with profile \"{profile}\" on this Mac. See list_mcp_servers.")
        })?;
    Ok((entry, target))
}

impl Sawit {
    /// The package manager that runs packages for an install: the one named
    /// when it's installed, else the one picked last in Sawit, else npm (npx,
    /// as before there was a choice).
    async fn package_manager_for(&self, named: Option<&str>) -> Result<PackageManager, String> {
        let wanted = match named {
            None => None,
            Some(name) => Some(PackageManager::parse(name).ok_or_else(|| {
                format!("\"{name}\" isn't a package manager: npm, pnpm, bun or yarn.")
            })?),
        };
        let available = package_manager::detect().await;
        let last = package_manager::remembered(&self.paths);
        match (
            wanted,
            package_manager::choose(&available, wanted, last, Use::Run),
        ) {
            (Some(w), Some(pm)) if w != pm => Err(format!(
                "{} isn't installed here (or, for Yarn 1, has no dlx). Installed: {}.",
                w.id(),
                package_manager::usable(&available, Use::Run)
                    .iter()
                    .map(|a| a.pm.id())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            (Some(w), None) => Err(format!(
                "{} isn't installed here, nor is any other package manager.",
                w.id()
            )),
            (_, pm) => Ok(pm.unwrap_or(PackageManager::Npm)),
        }
    }

    pub async fn list_mcp_servers_tool(&self) -> CallToolResult {
        let text = tri!(
            blocking(|| {
                let report = mcp_catalog::report(&own_server());
                let mut out = vec![format!(
                    "Clients found: {}",
                    report
                        .targets
                        .iter()
                        .map(target_label)
                        .collect::<Vec<_>>()
                        .join(", ")
                )];
                for listed in &report.servers {
                    let server = listed.server;
                    out.push(String::new());
                    out.push(format!(
                        "{} ({}) · {}",
                        server.id,
                        server.name,
                        server.address(DEFAULT_AWS_REGION)
                    ));
                    out.push(format!("  {}", server.about));
                    if server.package().is_some() {
                        let others: Vec<String> = [
                            PackageManager::Pnpm,
                            PackageManager::Bun,
                            PackageManager::Yarn,
                        ]
                        .into_iter()
                        .map(|pm| {
                            format!(
                                "{}: {}",
                                pm.id(),
                                server.address_with(DEFAULT_AWS_REGION, pm)
                            )
                        })
                        .collect();
                        out.push(format!("  package_manager: {}", others.join(", ")));
                    }
                    if let Some(auth) = auth_text(server.auth) {
                        out.push(format!("  auth: {auth}"));
                    }
                    if let Some(note) = server.note {
                        out.push(format!("  note: {note}"));
                    }
                    if let Some(program) = listed.missing_program {
                        out.push(format!("  missing program: {program} is not installed"));
                    }
                    let installed: Vec<String> = listed
                        .installed
                        .iter()
                        .map(|(ix, status)| {
                            let target = &report.targets[*ix];
                            format!("{} ({})", target_label(target), status_text(server, status))
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
        let region = tri!(region_for(server, &target, a.region.as_deref()));
        let pm = tri!(self.package_manager_for(a.package_manager.as_deref()).await);
        if server.is_remote()
            && mcp_clients::needs_bridge(target.client)
            && find_program(pm.runner()).is_none()
        {
            return fail(format!(
                "{} reaches remote servers only through the mcp-remote bridge, which `{}` runs, and it isn't installed. Install Node.js first (install_dev_tools with \"node\"), or pass another package_manager.",
                target.client.name(),
                pm.runner()
            ));
        }
        // A package run by a runner that isn't here would never start.
        if server.package().is_some()
            && let Some(program) = server.missing_program_with(find_program, pm)
        {
            return fail(format!(
                "{} runs with `{program}`, which isn't installed here: the client couldn't start it. Install it first, or pass another package_manager.",
                server.id
            ));
        }
        let spec = server.spec_with(&own_server(), &region, find_program, pm);
        if !target.client.editable() {
            return ok(format!(
                "{} keeps its config in a format Sawit doesn't edit. Add this to {}:\n\n{}",
                target.client.name(),
                target.config.display(),
                mcp_clients::snippet(&target, &spec)
            ));
        }
        let cli = target.client.cli().and_then(find_program);
        if let Err(e) = mcp_clients::install(&target, &spec, cli.as_deref()).await {
            return fail(e.to_string());
        }
        let mut text = format!(
            "Installed {} in {} ({}): {}.",
            server.id,
            target_label(&target),
            target.config.display(),
            status_text(server, &mcp_clients::status(&target, &spec))
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
        let cli = target.client.cli().and_then(find_program);
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

    /// An agent updating AWS without naming a Region keeps the one the
    /// client has; one that isn't a Region never reaches the URL.
    #[test]
    fn the_region_is_kept_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(
            &config,
            "[mcp_servers.aws]\nurl = \"https://aws-mcp.eu-west-1.api.aws/mcp\"\n",
        )
        .unwrap();
        let codex = Target {
            client: mcp_clients::Client::Codex,
            profile: "default".into(),
            config,
            profile_dir: None,
        };
        let aws = mcp_catalog::get("aws").unwrap();
        assert_eq!(region_for(aws, &codex, None).as_deref(), Ok("eu-west-1"));
        assert_eq!(
            region_for(aws, &codex, Some("ap-south-1")).as_deref(),
            Ok("ap-south-1")
        );
        assert!(region_for(aws, &codex, Some("x.example.com/#")).is_err());
        assert_eq!(
            status_text(
                aws,
                &mcp_clients::status(&codex, &spec(aws, DEFAULT_AWS_REGION))
            ),
            "installed in region eu-west-1"
        );
    }

    #[test]
    fn client_names_match_loosely() {
        assert!(same_name("Claude Code", "claude-code"));
        assert!(same_name("VS Code", "vscode"));
        assert!(!same_name("VS Code", "VS Code Insiders"));
    }

    #[test]
    fn every_catalog_server_has_an_id_and_known_auth_text() {
        for s in mcp_catalog::SERVERS {
            assert!(!s.id.is_empty());
            assert!(mcp_catalog::get(s.id).is_some());
            let _ = auth_text(s.auth);
        }
        assert!(auth_text(Auth::EnvKey("X_KEY")).unwrap().contains("X_KEY"));
    }
}
