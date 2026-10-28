//! MCP server exposing PG Desk to AI agents: list/start/stop local instances,
//! browse schemas, run read-only queries, and write a project's DATABASE_URL.

use std::path::Path;

use pgdesk_core::control::{self, Preflight, StartOutcome};
use pgdesk_core::db::{
    self, ConnProfile, Profiles, format_table, load_password, write_database_url,
};
use pgdesk_core::instances::{AppPaths, Instance, Registry};
use pgdesk_core::pgctl::PgCtlStatus;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock as Content};
use rmcp::{schemars, tool, tool_router};

/// Rows returned by `query` at most.
const MAX_ROWS: usize = 500;

#[derive(Debug, Clone)]
pub struct PgDesk {
    paths: AppPaths,
}

fn ok(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text.into())])
}

fn fail(text: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(text.into())])
}

pub fn instance_url(inst: &Instance) -> String {
    format!("postgres://postgres@localhost:{}/postgres", inst.port)
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InstanceArgs {
    #[schemars(description = "PG Desk instance name (see list_instances)")]
    pub name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetArgs {
    #[schemars(
        description = "PG Desk instance name or saved connection name (see list_connections)"
    )]
    pub target: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct QueryArgs {
    #[schemars(description = "PG Desk instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "SQL to run. Always executed read-only.")]
    pub sql: String,
    #[schemars(description = "Maximum rows to return (default and cap: 500)")]
    pub max_rows: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WriteEnvArgs {
    #[schemars(description = "PG Desk instance name")]
    pub instance: String,
    #[schemars(description = "Project folder whose .env gets DATABASE_URL")]
    pub project_dir: String,
}

impl PgDesk {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    fn registry(&self) -> Result<Registry, String> {
        Registry::load(&self.paths.instances_file()).map_err(|e| e.to_string())
    }

    fn instance(&self, name: &str) -> Result<Instance, String> {
        self.registry()?
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Instance \"{name}\" tidak ada. Lihat list_instances."))
    }

    /// Connection target: an instance (trust auth on localhost) or a saved profile.
    fn resolve(&self, target: &str) -> Result<(ConnProfile, Option<String>), String> {
        if let Ok(inst) = self.instance(target) {
            return Ok((
                ConnProfile::new("localhost", inst.port, "postgres", "postgres"),
                None,
            ));
        }
        let profiles =
            Profiles::load(&self.paths.root.join("profiles.toml")).map_err(|e| e.to_string())?;
        let profile = profiles
            .items
            .into_iter()
            .find(|p| p.name == target)
            .ok_or_else(|| format!("\"{target}\" bukan instance atau koneksi yang dikenal."))?;
        let password = load_password(&profile).ok().flatten();
        Ok((profile, password))
    }

    pub async fn list_instances_tool(&self) -> CallToolResult {
        let reg = match self.registry() {
            Ok(r) => r,
            Err(e) => return fail(e),
        };
        if reg.instances.is_empty() {
            return ok("Belum ada instance PG Desk.");
        }
        let mut lines = Vec::new();
        for inst in &reg.instances {
            let status = match control::status(inst).await {
                Ok(PgCtlStatus::Running { pid }) => format!("berjalan (PID {pid})"),
                Ok(PgCtlStatus::Stopped) => "berhenti".into(),
                Ok(other) => format!("{other:?}"),
                Err(e) => format!("error: {e}"),
            };
            lines.push(format!(
                "- {} · port {} · {status} · {}",
                inst.name,
                inst.port,
                instance_url(inst)
            ));
        }
        ok(lines.join("\n"))
    }

    pub async fn start_instance_tool(&self, name: &str) -> CallToolResult {
        let inst = match self.instance(name) {
            Ok(i) => i,
            Err(e) => return fail(e),
        };
        let managed = self.registry().map(|r| r.data_dirs()).unwrap_or_default();
        match control::start(&inst, &self.paths, managed, Preflight::Check).await {
            Ok(StartOutcome::Started) => ok(format!(
                "{name} berjalan. DATABASE_URL={}",
                instance_url(&inst)
            )),
            Ok(StartOutcome::AlreadyRunning { pid }) => {
                ok(format!("{name} sudah berjalan (PID {pid})."))
            }
            Ok(StartOutcome::Blocked(report)) => fail(format!(
                "Start ditahan karena konflik port. Selesaikan dulu atau minta pengguna memakai \"Tetap start\" di PG Desk.\n{}",
                report.explain()
            )),
            Err(e) => fail(e.to_string()),
        }
    }

    pub async fn stop_instance_tool(&self, name: &str) -> CallToolResult {
        match self.instance(name) {
            Ok(inst) => match control::stop(&inst).await {
                Ok(()) => ok(format!("{name} dihentikan.")),
                Err(e) => fail(e.to_string()),
            },
            Err(e) => fail(e),
        }
    }

    pub async fn list_connections_tool(&self) -> CallToolResult {
        let mut lines = Vec::new();
        if let Ok(reg) = self.registry() {
            lines.extend(
                reg.instances
                    .iter()
                    .map(|i| format!("- {} (instance PG Desk) · {}", i.name, instance_url(i))),
            );
        }
        if let Ok(p) = Profiles::load(&self.paths.root.join("profiles.toml")) {
            lines.extend(
                p.items
                    .iter()
                    .map(|p| format!("- {} · {}", p.name, p.url())),
            );
        }
        if lines.is_empty() {
            ok("Belum ada koneksi.")
        } else {
            ok(lines.join("\n"))
        }
    }

    pub async fn describe_schema_tool(&self, target: &str) -> CallToolResult {
        let (profile, password) = match self.resolve(target) {
            Ok(r) => r,
            Err(e) => return fail(e),
        };
        let client = match db::connect_read_only(&profile, password.as_deref()).await {
            Ok(c) => c,
            Err(e) => return fail(e.to_string()),
        };
        match db::load_schema(&client).await {
            Ok(schemas) => {
                let mut out = Vec::new();
                for s in schemas {
                    out.push(format!("schema {}", s.name));
                    for r in s.relations {
                        out.push(format!("  {:?} {}.{}", r.kind, s.name, r.name));
                        for c in r.columns {
                            out.push(format!(
                                "    {} {}{}",
                                c.name,
                                c.data_type,
                                if c.nullable { "" } else { " NOT NULL" }
                            ));
                        }
                    }
                }
                ok(out.join("\n"))
            }
            Err(e) => fail(e.to_string()),
        }
    }

    pub async fn query_tool(
        &self,
        target: &str,
        sql: &str,
        max_rows: Option<usize>,
    ) -> CallToolResult {
        let (profile, password) = match self.resolve(target) {
            Ok(r) => r,
            Err(e) => return fail(e),
        };
        let client = match db::connect_read_only(&profile, password.as_deref()).await {
            Ok(c) => c,
            Err(e) => return fail(e.to_string()),
        };
        let limit = max_rows.unwrap_or(MAX_ROWS).min(MAX_ROWS);
        match db::run_read_only(&client, sql, limit).await {
            Ok(outcome) => {
                let mut parts: Vec<String> = outcome.results.iter().map(format_table).collect();
                if parts.is_empty() {
                    parts.push(format!("OK ({} ms)", outcome.duration.as_millis()));
                }
                ok(parts.join("\n\n"))
            }
            Err(e) => fail(format!("{e}\n(query dijalankan read-only)")),
        }
    }

    pub async fn write_env_tool(&self, instance: &str, project_dir: &str) -> CallToolResult {
        let inst = match self.instance(instance) {
            Ok(i) => i,
            Err(e) => return fail(e),
        };
        match write_database_url(Path::new(project_dir), &instance_url(&inst)) {
            Ok(path) => ok(format!("DATABASE_URL ditulis ke {}", path.display())),
            Err(e) => fail(e),
        }
    }
}

#[tool_router(server_handler)]
impl PgDesk {
    #[tool(
        description = "List local PostgreSQL instances managed by PG Desk with status, port and DATABASE_URL"
    )]
    async fn list_instances(&self) -> CallToolResult {
        self.list_instances_tool().await
    }

    #[tool(
        description = "Start a PG Desk instance. Refuses (and explains) when another process already listens on its port, e.g. a Docker container"
    )]
    async fn start_instance(&self, Parameters(a): Parameters<InstanceArgs>) -> CallToolResult {
        self.start_instance_tool(&a.name).await
    }

    #[tool(description = "Stop a PG Desk instance (fast shutdown)")]
    async fn stop_instance(&self, Parameters(a): Parameters<InstanceArgs>) -> CallToolResult {
        self.stop_instance_tool(&a.name).await
    }

    #[tool(
        description = "List instances and saved connections that describe_schema and query accept"
    )]
    async fn list_connections(&self) -> CallToolResult {
        self.list_connections_tool().await
    }

    #[tool(description = "Describe schemas, tables, views and columns of a database")]
    async fn describe_schema(&self, Parameters(a): Parameters<TargetArgs>) -> CallToolResult {
        self.describe_schema_tool(&a.target).await
    }

    #[tool(
        description = "Run SQL read-only (writes are rejected) and return up to 500 rows as a text table"
    )]
    async fn query(&self, Parameters(a): Parameters<QueryArgs>) -> CallToolResult {
        self.query_tool(&a.target, &a.sql, a.max_rows).await
    }

    #[tool(
        description = "Write DATABASE_URL for a PG Desk instance into <project_dir>/.env, e.g. for a TanStack Start app"
    )]
    async fn write_env(&self, Parameters(a): Parameters<WriteEnvArgs>) -> CallToolResult {
        self.write_env_tool(&a.instance, &a.project_dir).await
    }
}
