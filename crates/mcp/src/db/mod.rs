//! Databases: finding them, listing/creating/dropping, schemas, read-only
//! and read-write SQL, saved connections and `.env` files.
//!
//! SQLite files and Cloudflare D1 databases (local and remote) are targets
//! too, by the ids `detect_databases` gives (`d1-local:DB@/project`,
//! `d1-remote:shop@/project`, `sqlite:/path`). The tools that read and write
//! data work on them with the same read-only and confirm rules; the ones only
//! a PostgreSQL server has say so.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use sawit_core::db::{self, Conn, ConnProfile, Profiles, resolve_password};
use sawit_core::instances::{Instance, Registry};

use crate::{Sawit, blocking};

mod args;
mod connections;
mod describe;
mod detect;
mod sql;
#[cfg(test)]
mod tests;

pub use args::{
    CreateDbArgs, DetectArgs, DropDbArgs, ExecuteArgs, QueryArgs, RemoveConnectionArgs,
    SaveConnectionArgs, SchemaArgs, TableArgs, TargetArgs, WriteEnvArgs,
};

/// Rows returned per result set at most.
pub(crate) const MAX_ROWS: usize = 500;

/// Where a target lives and how to log in with full privileges.
pub(crate) enum Target {
    Instance(Instance),
    Profile(Box<ConnProfile>),
    /// A SQLite file or a D1 database.
    Lite(Box<ConnProfile>),
}

/// Why a PostgreSQL-only tool won't take a SQLite or D1 target.
pub(crate) fn postgres_only(p: &ConnProfile) -> String {
    format!(
        "Refused: “{}” is {} (SQLite), and this tool works on PostgreSQL only. On it, use query, execute_sql, describe_schema, describe_table, list_databases, export_table, export_query or schema_diagram.",
        p.name,
        p.kind.title()
    )
}

/// `confirm` must repeat `name` exactly before something is destroyed.
pub(crate) fn check_confirm(name: &str, confirm: &str, what: &str) -> Result<(), String> {
    if confirm == name {
        Ok(())
    } else {
        Err(format!(
            "Refused: to {what} “{name}”, pass confirm=\"{name}\" (got \"{confirm}\")."
        ))
    }
}

impl Sawit {
    pub(super) fn profiles_file(&self) -> std::path::PathBuf {
        self.paths.root.join("profiles.toml")
    }

    pub(crate) fn registry(&self) -> Result<Registry, String> {
        Registry::load(&self.paths.instances_file()).map_err(|e| e.to_string())
    }

    pub(crate) fn instance(&self, name: &str) -> Result<Instance, String> {
        self.registry()?
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Instance \"{name}\" does not exist. See list_instances."))
    }

    pub(super) fn profiles(&self) -> Result<Profiles, String> {
        Profiles::load(&self.profiles_file()).map_err(|e| e.to_string())
    }

    pub(crate) fn resolve_target(&self, target: &str) -> Result<Target, String> {
        if let Some(found) = db::d1::resolve(target.trim()) {
            return found.map(|p| Target::Lite(Box::new(p)));
        }
        if let Ok(inst) = self.instance(target) {
            return Ok(Target::Instance(inst));
        }
        self.profiles()?
            .items
            .into_iter()
            .find(|p| p.name == target)
            .map(|p| {
                if p.kind.is_lite() {
                    Target::Lite(Box::new(p))
                } else {
                    Target::Profile(Box::new(p))
                }
            })
            .ok_or_else(|| {
                format!("\"{target}\" is not a known instance or connection. See list_connections.")
            })
    }

    /// Full-privilege login for `target`, on `database` when given: the
    /// `postgres` superuser for instances, the saved credentials otherwise.
    pub(crate) async fn admin_login(
        &self,
        target: &str,
        database: Option<&str>,
    ) -> Result<(ConnProfile, Option<String>), String> {
        let database = database.map(str::trim).filter(|d| !d.is_empty());
        match self.resolve_target(target)? {
            Target::Instance(inst) => Ok((
                ConnProfile::new(
                    "localhost",
                    inst.port,
                    database.unwrap_or("postgres"),
                    "postgres",
                ),
                None,
            )),
            Target::Lite(profile) => Err(postgres_only(&profile)),
            Target::Profile(mut profile) => {
                // The Keychain entry belongs to the saved database.
                let saved = profile.clone();
                let password = blocking(move || resolve_password(&saved)).await?;
                if let Some(db) = database {
                    profile.database = db.to_string();
                }
                Ok((*profile, password))
            }
        }
    }

    pub(crate) async fn connect_admin(
        &self,
        target: &str,
        database: Option<&str>,
    ) -> Result<tokio_postgres::Client, String> {
        let (profile, password) = self.admin_login(target, database).await?;
        db::connect(&profile, password.as_deref())
            .await
            .map_err(|e| e.to_string())
    }

    /// Read-only connection for a target. Instances are reached as the
    /// non-superuser `sawit_ro` role (created on first use), so agents can't
    /// write, COPY to files or run server programs. Saved profiles use their
    /// own credentials inside read-only, single-statement transactions.
    pub(crate) async fn connect_target(
        &self,
        target: &str,
        database: Option<&str>,
    ) -> Result<tokio_postgres::Client, String> {
        let (profile, password) = self.admin_login(target, database).await?;
        if let Target::Instance(inst) = self.resolve_target(target)? {
            let admin = ConnProfile::new("localhost", inst.port, "postgres", "postgres");
            let client = db::connect(&admin, None).await.map_err(|e| e.to_string())?;
            db::ensure_read_only_role(&client)
                .await
                .map_err(|e| e.to_string())?;
            let ro = ConnProfile {
                user: db::READ_ONLY_ROLE.into(),
                ..profile
            };
            return db::connect_read_only(&ro, None)
                .await
                .map_err(|e| e.to_string());
        }
        db::connect_read_only(&profile, password.as_deref())
            .await
            .map_err(|e| e.to_string())
    }

    /// A handle on a SQLite or D1 target (`None` for PostgreSQL ones): read-only for the
    /// tools that only read.
    pub(crate) fn lite_conn(&self, target: &str, read_only: bool) -> Result<Option<Conn>, String> {
        match self.resolve_target(target)? {
            Target::Lite(p) => Ok(Some(Conn::lite(*p, read_only))),
            _ => Ok(None),
        }
    }
}

#[tool_router(router = db_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List instances and saved connections that the database tools accept as `target`"
    )]
    async fn list_connections(&self) -> CallToolResult {
        self.list_connections_tool().await
    }

    #[tool(
        description = "Find every database reachable from this Mac: Sawit instances, saved connections, DATABASE_URL in a project's .env (optional project_dir), every database of any running local PostgreSQL (Homebrew, Postgres.app, Sawit), and with project_dir the Cloudflare D1 databases its Wrangler config binds (local SQLite files and remote databases). Each line starts with the id to pass as `target` and its kind: postgres, d1-local, d1-remote or sqlite"
    )]
    async fn detect_databases(&self, Parameters(a): Parameters<DetectArgs>) -> CallToolResult {
        self.detect_databases_tool(a.project_dir.as_deref()).await
    }

    #[tool(
        description = "List the databases on a server (instance or saved connection); for a D1 target, the project's D1 databases (d1-local) or the Wrangler account's (d1-remote)"
    )]
    async fn list_databases(&self, Parameters(a): Parameters<TargetArgs>) -> CallToolResult {
        self.list_databases_tool(&a.target).await
    }

    #[tool(
        description = "Create a database on an instance (as the postgres superuser) or a saved connection (with its own credentials)"
    )]
    async fn create_database(&self, Parameters(a): Parameters<CreateDbArgs>) -> CallToolResult {
        self.create_database_tool(a).await
    }

    #[tool(
        description = "Drop (permanently delete) a database. `confirm` must equal `name`, otherwise nothing happens"
    )]
    async fn drop_database(&self, Parameters(a): Parameters<DropDbArgs>) -> CallToolResult {
        self.drop_database_tool(a).await
    }

    #[tool(
        description = "Describe schemas, tables, views, columns, primary and foreign keys of a database (optionally one schema)"
    )]
    async fn describe_schema(&self, Parameters(a): Parameters<SchemaArgs>) -> CallToolResult {
        self.describe_schema_tool(&a.target, a.schema.as_deref(), a.database.as_deref())
            .await
    }

    #[tool(
        description = "Describe one table or view: columns with types, defaults and NOT NULL, primary key, foreign keys, unique/check constraints, indexes, referencing tables and the estimated row count"
    )]
    async fn describe_table(&self, Parameters(a): Parameters<TableArgs>) -> CallToolResult {
        self.describe_table_tool(a).await
    }

    #[tool(
        description = "Run one SQL statement READ-ONLY (writes are rejected) and return up to 500 rows as a text table. Use this for SELECTs; it cannot modify anything. On a local D1 or SQLite target the file is opened read-only"
    )]
    async fn query(&self, Parameters(a): Parameters<QueryArgs>) -> CallToolResult {
        self.query_tool(&a.target, &a.sql, a.database.as_deref(), a.max_rows)
            .await
    }

    #[tool(
        description = "Execute SQL READ-WRITE with full privileges (postgres superuser on instances, the saved credentials on connections). CAN MODIFY OR DELETE DATA AND SCHEMA (INSERT/UPDATE/DELETE/DDL); several statements allowed. Prefer `query` for reads. On a local D1 target, writes go through the running dev server's Local Explorer, else straight to the file (the answer says which); on a remote (production) D1 target they need confirm=true"
    )]
    async fn execute_sql(&self, Parameters(a): Parameters<ExecuteArgs>) -> CallToolResult {
        self.execute_sql_tool(a).await
    }

    #[tool(
        description = "Save a connection from a postgres:// URL under a name (password goes to the macOS Keychain). Replaces a connection with the same name"
    )]
    async fn save_connection(
        &self,
        Parameters(a): Parameters<SaveConnectionArgs>,
    ) -> CallToolResult {
        self.save_connection_tool(a).await
    }

    #[tool(
        description = "Remove a saved connection and its Keychain password (kept while another saved connection uses the same one). DESTRUCTIVE, can't be undone: `confirm` must equal `name`, otherwise nothing happens"
    )]
    async fn remove_connection(
        &self,
        Parameters(a): Parameters<RemoveConnectionArgs>,
    ) -> CallToolResult {
        self.remove_connection_tool(&a.name, &a.confirm).await
    }

    #[tool(
        description = "Write DATABASE_URL for a Sawit instance into <project_dir>/.env, e.g. for a TanStack Start app; the rest of the file stays. DESTRUCTIVE when the .env already sets a different DATABASE_URL: replacing it needs confirm=true (the refusal shows the current one, without its password)"
    )]
    async fn write_env(&self, Parameters(a): Parameters<WriteEnvArgs>) -> CallToolResult {
        self.write_env_tool(&a.instance, &a.project_dir, a.confirm)
            .await
    }
}
