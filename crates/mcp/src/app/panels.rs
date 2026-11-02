//! The tools that do what the sidebar's panels do: the Run panel's scripts, SSH sessions, a
//! database's tables, SQL and diagram, instances, the network diagnosis and prompts.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::{Command, InstanceAction};

use super::{INSTANCE_ACTIONS, WindowArgs, variant};
use crate::{Sawit, fail};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ScriptArgs {
    #[schemars(description = "Script or saved command name as app_run_list shows it")]
    pub name: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SshConnectArgs {
    #[schemars(description = "Saved server id or name, or a ~/.ssh/config alias")]
    pub host: String,
    #[schemars(description = "Open another session even if one is open")]
    #[serde(default)]
    pub new_session: bool,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenTableArgs {
    #[schemars(description = "Saved connection name or Sawit instance name")]
    pub connection: String,
    pub schema: String,
    pub table: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenSqlArgs {
    #[schemars(description = "Saved connection name or Sawit instance name")]
    pub connection: String,
    #[schemars(description = "SQL to put in the editor")]
    pub sql: Option<String>,
    #[schemars(
        description = "Run it right away (default false). It runs with the connection's own privileges, like execute_sql"
    )]
    pub run: Option<bool>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InstanceArgs {
    #[schemars(description = "Sawit instance name (see list_instances)")]
    pub name: String,
    #[schemars(description = "select, start, stop or restart")]
    pub action: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NetworkArgs {
    #[schemars(description = "Also ping this host or IP (4 pings)")]
    pub ping: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenDiagramArgs {
    #[schemars(description = "Saved connection name or Sawit instance name")]
    pub connection: String,
    #[schemars(description = "Schema to draw, e.g. public")]
    pub schema: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenPromptArgs {
    #[schemars(description = "Prompt id (see list_prompts)")]
    pub id: String,
    pub project: Option<String>,
}

#[tool_router(router = panels_router, vis = "pub(super)")]
impl Sawit {
    #[tool(
        description = "The Run panel's scripts (package.json, Make, cargo, deno, composer, workspaces) and saved commands for the window's project, with their exact names and whether each is running"
    )]
    async fn app_run_list(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::RunList).await
    }

    #[tool(
        description = "Start a script or saved command from the Run panel in its own terminal tab. Returns the terminal (tab) and pane it runs in, or each one's for a saved command that runs several: pass them to terminal_read for its output and to terminal_input to answer or stop it"
    )]
    async fn app_run_script(&self, Parameters(a): Parameters<ScriptArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::RunScript { name: a.name })
            .await
    }

    #[tool(description = "Stop (Ctrl-C) a running script or saved command from the Run panel")]
    async fn app_stop_script(&self, Parameters(a): Parameters<ScriptArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::StopScript { name: a.name })
            .await
    }

    #[tool(
        description = "Open an interactive SSH session to a saved server or ~/.ssh/config host in Sawit's terminal; returns its terminal and pane, to drive it with terminal_input / terminal_read"
    )]
    async fn app_ssh_connect(&self, Parameters(a): Parameters<SshConnectArgs>) -> CallToolResult {
        let command = Command::SshConnect {
            host: a.host,
            new_session: a.new_session,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(description = "Open a table in Sawit's data grid")]
    async fn app_open_table(&self, Parameters(a): Parameters<OpenTableArgs>) -> CallToolResult {
        let command = Command::OpenTable {
            connection: a.connection,
            schema: a.schema,
            table: a.table,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(
        description = "Open a SQL editor tab for a connection, optionally prefilled with SQL; run=true also runs it in the app, as ⌘↩ does (the results show in the tab; use query or execute_sql to get rows back as text)"
    )]
    async fn app_open_sql(&self, Parameters(a): Parameters<OpenSqlArgs>) -> CallToolResult {
        let command = Command::OpenSql {
            connection: a.connection,
            sql: a.sql,
            run: a.run.unwrap_or(false),
        };
        self.app_tool(a.project, command).await
    }

    #[tool(
        description = "Select a PostgreSQL instance in the app's Instances tab and start, stop or restart it there, as its buttons do (needs the app; start_instance/stop_instance work without it). A start whose port is taken is held and the Instance tab explains the conflict. Returns at once; read the outcome with list_instances or instance_logs"
    )]
    async fn app_instance(&self, Parameters(a): Parameters<InstanceArgs>) -> CallToolResult {
        match variant::<InstanceAction>(&a.action, INSTANCE_ACTIONS) {
            Ok(action) => {
                let command = Command::Instance {
                    name: a.name,
                    action,
                };
                self.app_tool(a.project, command).await
            }
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Run the app's network diagnosis (connection, router, DNS, internet, Wi-Fi sign-in) on its Network tab, optionally pinging a host too. Needs the app; diagnose_network and ping return the same results as text without it"
    )]
    async fn app_network_diagnose(&self, Parameters(a): Parameters<NetworkArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::NetworkDiagnose { ping: a.ping })
            .await
    }

    #[tool(
        description = "Open a schema's diagram (tables and foreign keys, drawn by Pele) as a tab"
    )]
    async fn app_open_diagram(&self, Parameters(a): Parameters<OpenDiagramArgs>) -> CallToolResult {
        let command = Command::OpenDiagram {
            connection: a.connection,
            schema: a.schema,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(description = "Open a saved prompt in its tab")]
    async fn app_open_prompt(&self, Parameters(a): Parameters<OpenPromptArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::OpenPrompt { id: a.id })
            .await
    }
}
