//! Databases: finding them, listing/creating/dropping, schemas, read-only
//! and read-write SQL, saved connections and `.env` files.

use std::path::Path;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::Store;
use sawit_core::db::{
    self, ConnProfile, Profiles, QueryOutcome, format_table, parse_database_url, quote_ident,
    resolve_password, write_database_url,
};
use sawit_core::instances::{Instance, Registry};
use sawit_core::kebun::{self, Kebun};

use crate::{Sawit, blocking, fail, instance_url, ok, tilde, tri};

/// Rows returned per result set at most.
pub(crate) const MAX_ROWS: usize = 500;

/// Where a target lives and how to log in with full privileges.
pub(crate) enum Target {
    Instance(Instance),
    Profile(Box<ConnProfile>),
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetArgs {
    #[schemars(
        description = "Sawit instance name or saved connection name (see list_connections)"
    )]
    pub target: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SchemaArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Only this schema (default: every user schema)")]
    pub schema: Option<String>,
    #[schemars(
        description = "Database to connect to (default: `postgres` for instances, the connection's own database otherwise)"
    )]
    pub database: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TableArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Schema of the table (default: public, or the only match)")]
    pub schema: Option<String>,
    #[schemars(description = "Table or view name")]
    pub table: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct QueryArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "SQL to run. Always executed read-only; one statement.")]
    pub sql: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "Maximum rows to return (default and cap: 500)")]
    pub max_rows: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExecuteArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(
        description = "SQL to execute with full privileges; may hold several statements separated by `;`"
    )]
    pub sql: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "Maximum rows per result set (default and cap: 500)")]
    pub max_rows: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DetectArgs {
    #[schemars(
        description = "Project folder whose .env (and the .env files above it) DATABASE_URL should be included"
    )]
    pub project_dir: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CreateDbArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Name of the new database")]
    pub name: String,
    #[schemars(description = "Owner role (default: the connecting user)")]
    pub owner: Option<String>,
    #[schemars(description = "Template database (default: template1)")]
    pub template: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DropDbArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Database to drop")]
    pub name: String,
    #[schemars(description = "Must equal `name` exactly, to confirm the drop")]
    pub confirm: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SaveConnectionArgs {
    #[schemars(description = "Name for the connection")]
    pub name: String,
    #[schemars(
        description = "postgres:// URL. A password in it goes to the macOS Keychain, never to profiles.toml"
    )]
    pub url: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NameArgs {
    #[schemars(description = "Saved connection name")]
    pub name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WriteEnvArgs {
    #[schemars(description = "Sawit instance name")]
    pub instance: String,
    #[schemars(description = "Project folder whose .env gets DATABASE_URL")]
    pub project_dir: String,
}

/// Query results as text: each result set as a table, then the row counts
/// of statements that returned none.
pub(crate) fn format_outcome(outcome: &QueryOutcome) -> String {
    let mut parts: Vec<String> = outcome.results.iter().map(format_table).collect();
    match outcome.affected.as_slice() {
        [] => {}
        [n] => parts.push(format!("{n} {} affected", plural(*n, "row"))),
        many => parts.push(
            many.iter()
                .enumerate()
                .map(|(i, n)| format!("statement {}: {n} {} affected", i + 1, plural(*n, "row")))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    }
    parts.push(format!("({} ms)", outcome.duration.as_millis()));
    parts.join("\n\n")
}

fn plural(n: u64, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
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

/// `CREATE DATABASE` with quoted identifiers.
pub(crate) fn create_database_sql(name: &str, owner: Option<&str>, template: Option<&str>) -> String {
    let mut sql = format!("CREATE DATABASE {}", quote_ident(name));
    if let Some(owner) = owner.filter(|o| !o.trim().is_empty()) {
        sql.push_str(&format!(" OWNER {}", quote_ident(owner.trim())));
    }
    if let Some(template) = template.filter(|t| !t.trim().is_empty()) {
        sql.push_str(&format!(" TEMPLATE {}", quote_ident(template.trim())));
    }
    sql
}

/// One detected database as a line.
fn kebun_line(k: &Kebun) -> String {
    let p = &k.profile;
    format!(
        "- {} · {}:{} · db {} · user {} · via {}",
        p.name, p.host, p.port, p.database, p.user, k.via
    )
}

/// The `via` labels of `sawit_core::kebun` in English.
fn english_via(via: &str) -> String {
    if let Some(name) = via.strip_prefix("koneksi tersimpan ") {
        format!("saved connection {name}")
    } else if let Some(dir) = via.strip_prefix(".env di ") {
        format!(".env in {dir}")
    } else {
        via.to_string()
    }
}

impl Sawit {
    fn profiles_file(&self) -> std::path::PathBuf {
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

    fn profiles(&self) -> Result<Profiles, String> {
        Profiles::load(&self.profiles_file()).map_err(|e| e.to_string())
    }

    pub(crate) fn resolve_target(&self, target: &str) -> Result<Target, String> {
        if let Ok(inst) = self.instance(target) {
            return Ok(Target::Instance(inst));
        }
        self.profiles()?
            .items
            .into_iter()
            .find(|p| p.name == target)
            .map(|p| Target::Profile(Box::new(p)))
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
                ConnProfile::new("localhost", inst.port, database.unwrap_or("postgres"), "postgres"),
                None,
            )),
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

    pub async fn list_connections_tool(&self) -> CallToolResult {
        let mut lines = Vec::new();
        if let Ok(reg) = self.registry() {
            lines.extend(
                reg.instances
                    .iter()
                    .map(|i| format!("- {} (Sawit instance) · {}", i.name, instance_url(i))),
            );
        }
        if let Ok(p) = self.profiles() {
            lines.extend(p.items.iter().map(|p| format!("- {} · {}", p.name, p.url())));
        }
        if lines.is_empty() {
            ok("No connections yet.")
        } else {
            ok(lines.join("\n"))
        }
    }

    pub async fn describe_schema_tool(
        &self,
        target: &str,
        schema: Option<&str>,
        database: Option<&str>,
    ) -> CallToolResult {
        let client = tri!(self.connect_target(target, database).await);
        let schemas = tri!(db::load_schema(&client).await.map_err(|e| e.to_string()));
        let mut out = Vec::new();
        for s in schemas.iter().filter(|s| schema.is_none_or(|o| o == s.name)) {
            out.push(format!("schema {}", s.name));
            for r in &s.relations {
                let rows = r
                    .estimated_rows
                    .map(|n| format!(" (~{n} rows)"))
                    .unwrap_or_default();
                out.push(format!("  {:?} {}.{}{rows}", r.kind, s.name, r.name));
                for c in &r.columns {
                    let pk = if r.primary_key.contains(&c.name) { " PK" } else { "" };
                    out.push(format!(
                        "    {} {}{}{pk}",
                        c.name,
                        c.data_type,
                        if c.nullable { "" } else { " NOT NULL" }
                    ));
                }
                for fk in &r.foreign_keys {
                    out.push(format!(
                        "    FK ({}) → {}.{}({})",
                        fk.columns.join(", "),
                        fk.ref_schema,
                        fk.ref_table,
                        fk.ref_columns.join(", ")
                    ));
                }
            }
        }
        if out.is_empty() {
            return match schema {
                Some(s) => fail(format!("No schema named \"{s}\" (or it has nothing visible).")),
                None => ok("No user schemas."),
            };
        }
        ok(out.join("\n"))
    }

    pub async fn query_tool(
        &self,
        target: &str,
        sql: &str,
        database: Option<&str>,
        max_rows: Option<usize>,
    ) -> CallToolResult {
        let client = tri!(self.connect_target(target, database).await);
        let limit = max_rows.unwrap_or(MAX_ROWS).clamp(1, MAX_ROWS);
        match db::run_read_only(&client, sql, limit).await {
            Ok(outcome) => ok(format_outcome(&outcome)),
            Err(e) => fail(format!(
                "{e}\n(query runs read-only; use execute_sql to change data)"
            )),
        }
    }

    pub async fn execute_sql_tool(&self, a: ExecuteArgs) -> CallToolResult {
        let client = tri!(self.connect_admin(&a.target, a.database.as_deref()).await);
        let limit = a.max_rows.unwrap_or(MAX_ROWS).clamp(1, MAX_ROWS);
        match db::run_sql(&client, &a.sql, limit).await {
            Ok(outcome) => ok(format_outcome(&outcome)),
            Err(e) => fail(e.to_string()),
        }
    }

    pub async fn detect_databases_tool(&self, project_dir: Option<&str>) -> CallToolResult {
        let registry = self.registry().unwrap_or_default();
        let mut out = Vec::new();
        if !registry.instances.is_empty() {
            out.push("Sawit instances:".to_string());
            for inst in &registry.instances {
                out.push(format!(
                    "- {} · localhost:{} · {}",
                    inst.name,
                    inst.port,
                    instance_url(inst)
                ));
            }
        }
        let paths = self.paths.clone();
        let mut found: Vec<Kebun> = tri!(blocking(move || kebun::saved_profiles(&paths)).await);
        if let Some(dir) = project_dir.filter(|d| !d.trim().is_empty()) {
            let dir = tri!(crate::existing_dir(dir));
            found.extend(tri!(blocking(move || kebun::dotenv_kebuns(&dir)).await));
        }
        found.extend(kebun::local_kebuns(&registry).await);
        let mut unique: Vec<Kebun> = Vec::new();
        for mut k in found {
            let p = &k.profile;
            if unique.iter().any(|u| {
                u.profile.host == p.host && u.profile.port == p.port && u.profile.database == p.database
            }) {
                continue;
            }
            k.via = english_via(&k.via);
            unique.push(k);
        }
        if !unique.is_empty() {
            out.push("Databases (running servers list every database):".to_string());
            out.extend(unique.iter().map(kebun_line));
        }
        if out.is_empty() {
            return ok("No databases found: no Sawit instances, saved connections, .env DATABASE_URL or running local Postgres.");
        }
        ok(out.join("\n"))
    }

    pub async fn list_databases_tool(&self, target: &str) -> CallToolResult {
        let client = tri!(self.connect_admin(target, None).await);
        match db::list_databases(&client).await {
            Ok(names) if names.is_empty() => ok("No databases."),
            Ok(names) => ok(names
                .iter()
                .map(|n| format!("- {n}"))
                .collect::<Vec<_>>()
                .join("\n")),
            Err(e) => fail(e.to_string()),
        }
    }

    pub async fn create_database_tool(&self, a: CreateDbArgs) -> CallToolResult {
        let name = a.name.trim();
        if name.is_empty() {
            return fail("Give the new database a name.");
        }
        let client = tri!(self.connect_admin(&a.target, None).await);
        let sql = create_database_sql(name, a.owner.as_deref(), a.template.as_deref());
        match client.batch_execute(&sql).await {
            Ok(()) => ok(format!("Created database {name} on {}.", a.target)),
            Err(e) => fail(db::sql_error(e).to_string()),
        }
    }

    pub async fn drop_database_tool(&self, a: DropDbArgs) -> CallToolResult {
        tri!(check_confirm(&a.name, &a.confirm, "drop database"));
        // Not connected to the one being dropped.
        let (profile, _) = tri!(self.admin_login(&a.target, None).await);
        let via = (profile.database == a.name).then_some("postgres");
        let client = tri!(self.connect_admin(&a.target, via).await);
        match client
            .batch_execute(&format!("DROP DATABASE {}", quote_ident(&a.name)))
            .await
        {
            Ok(()) => ok(format!("Dropped database {} on {}.", a.name, a.target)),
            Err(e) => fail(db::sql_error(e).to_string()),
        }
    }

    pub async fn describe_table_tool(&self, a: TableArgs) -> CallToolResult {
        let client = tri!(self.connect_target(&a.target, a.database.as_deref()).await);
        let err = |e: tokio_postgres::Error| db::sql_error(e).to_string();
        let found = tri!(client
            .query(
                "SELECT n.nspname, CASE c.relkind WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized view'
                          WHEN 'f' THEN 'foreign table' WHEN 'p' THEN 'partitioned table' ELSE 'table' END,
                        CASE WHEN c.reltuples < 0 THEN NULL ELSE c.reltuples::bigint END,
                        obj_description(c.oid, 'pg_class')
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE c.relname = $1 AND c.relkind IN ('r','p','v','m','f')
                   AND ($2::text IS NULL OR n.nspname = $2)
                   AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                 ORDER BY n.nspname <> 'public', n.nspname",
                &[&a.table, &a.schema],
            )
            .await
            .map_err(err));
        let row = match found.as_slice() {
            [] => return fail(format!("No table or view named \"{}\". See describe_schema.", a.table)),
            [one] => one,
            [first, ..] if first.get::<_, String>(0) == "public" && a.schema.is_none() => first,
            many => {
                let schemas: Vec<String> = many.iter().map(|r| r.get(0)).collect();
                return fail(format!(
                    "\"{}\" exists in several schemas ({}); pass `schema`.",
                    a.table,
                    schemas.join(", ")
                ));
            }
        };
        let schema: String = row.get(0);
        let kind: String = row.get(1);
        let estimate: Option<i64> = row.get(2);
        let comment: Option<String> = row.get(3);
        let rel = "format('%I.%I', $1::text, $2::text)::regclass";
        let columns = tri!(client
            .query(
                &format!(
                    "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), a.attnotnull,
                            pg_get_expr(d.adbin, d.adrelid), col_description(a.attrelid, a.attnum)
                     FROM pg_attribute a
                     LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
                     WHERE a.attrelid = {rel} AND a.attnum > 0 AND NOT a.attisdropped
                     ORDER BY a.attnum"
                ),
                &[&schema, &a.table],
            )
            .await
            .map_err(err));
        let constraints = tri!(client
            .query(
                &format!(
                    "SELECT conname::text, contype::text, pg_get_constraintdef(oid)
                     FROM pg_constraint WHERE conrelid = {rel}
                     ORDER BY array_position(ARRAY['p','f','u','c','x'], contype::text), conname"
                ),
                &[&schema, &a.table],
            )
            .await
            .map_err(err));
        let indexes = tri!(client
            .query(
                &format!(
                    "SELECT i.relname::text, pg_get_indexdef(x.indexrelid)
                     FROM pg_index x JOIN pg_class i ON i.oid = x.indexrelid
                     WHERE x.indrelid = {rel} ORDER BY x.indisprimary DESC, i.relname"
                ),
                &[&schema, &a.table],
            )
            .await
            .map_err(err));
        let referenced = tri!(client
            .query(
                &format!(
                    "SELECT conname::text, conrelid::regclass::text, pg_get_constraintdef(oid)
                     FROM pg_constraint WHERE confrelid = {rel} AND contype = 'f' ORDER BY 2, 1"
                ),
                &[&schema, &a.table],
            )
            .await
            .map_err(err));

        let mut out = vec![format!(
            "{kind} {schema}.{}{}",
            a.table,
            estimate.map(|n| format!(" · ~{n} rows (estimate)")).unwrap_or_default()
        )];
        if let Some(c) = comment {
            out.push(format!("comment: {c}"));
        }
        out.push(String::new());
        out.push("Columns:".into());
        for c in &columns {
            let name: String = c.get(0);
            let ty: String = c.get(1);
            let not_null: bool = c.get(2);
            let default: Option<String> = c.get(3);
            let comment: Option<String> = c.get(4);
            out.push(format!(
                "  {name} {ty}{}{}{}",
                if not_null { " NOT NULL" } else { "" },
                default.map(|d| format!(" DEFAULT {d}")).unwrap_or_default(),
                comment.map(|c| format!("  -- {c}")).unwrap_or_default()
            ));
        }
        let kind_name = |t: &str| match t {
            "p" => "PRIMARY KEY",
            "f" => "FOREIGN KEY",
            "u" => "UNIQUE",
            "c" => "CHECK",
            "x" => "EXCLUDE",
            _ => "CONSTRAINT",
        };
        if !constraints.is_empty() {
            out.push(String::new());
            out.push("Constraints:".into());
            for c in &constraints {
                let name: String = c.get(0);
                let t: String = c.get(1);
                let def: String = c.get(2);
                out.push(format!("  {} {name}: {def}", kind_name(&t)));
            }
        }
        if !indexes.is_empty() {
            out.push(String::new());
            out.push("Indexes:".into());
            for i in &indexes {
                let def: String = i.get(1);
                out.push(format!("  {def}"));
            }
        }
        if !referenced.is_empty() {
            out.push(String::new());
            out.push("Referenced by:".into());
            for r in &referenced {
                let name: String = r.get(0);
                let from: String = r.get(1);
                let def: String = r.get(2);
                out.push(format!("  {from} ({name}): {def}"));
            }
        }
        ok(out.join("\n"))
    }

    pub async fn save_connection_tool(&self, a: SaveConnectionArgs) -> CallToolResult {
        let name = a.name.trim().to_string();
        if name.is_empty() {
            return fail("Give the connection a name.");
        }
        if self.instance(&name).is_ok() {
            return fail(format!(
                "\"{name}\" is already a Sawit instance name; choose another so targets stay unambiguous."
            ));
        }
        let (mut profile, password) = tri!(parse_database_url(&a.url).map_err(|e| e.to_string()));
        profile.name = name.clone();
        let mut profiles = tri!(self.profiles());
        let replaced = profiles.items.iter().any(|p| p.name == name);
        if let Some(password) = password {
            let p = profile.clone();
            tri!(blocking(move || db::store_password(&p, &password))
                .await
                .and_then(|r| r.map_err(|e| {
                    format!("Unable to save the password in the Keychain. {e}")
                })));
        }
        profiles.upsert(profile.clone());
        tri!(profiles.save(&self.profiles_file()).map_err(|e| e.to_string()));
        self.notify(Store::Connections).await;
        ok(format!(
            "{} connection {name} · {}",
            if replaced { "Updated" } else { "Saved" },
            profile.url()
        ))
    }

    pub async fn remove_connection_tool(&self, name: &str) -> CallToolResult {
        let mut profiles = tri!(self.profiles());
        let Some(profile) = profiles.items.iter().find(|p| p.name == name).cloned() else {
            return fail(format!("No saved connection named \"{name}\". See list_connections."));
        };
        profiles.remove(name);
        tri!(profiles.save(&self.profiles_file()).map_err(|e| e.to_string()));
        // The password is shared by profiles with the same user, host and database.
        let shared = profiles
            .items
            .iter()
            .any(|p| p.keychain_account() == profile.keychain_account());
        if !shared {
            let _ = blocking(move || db::delete_password(&profile)).await;
        }
        self.notify(Store::Connections).await;
        ok(format!("Removed connection {name}."))
    }

    pub async fn write_env_tool(&self, instance: &str, project_dir: &str) -> CallToolResult {
        let inst = tri!(self.instance(instance));
        match write_database_url(Path::new(project_dir), &instance_url(&inst)) {
            Ok(path) => ok(format!("DATABASE_URL written to {}", tilde(&path))),
            Err(e) => fail(e),
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
        description = "Find every database reachable from this Mac: Sawit instances, saved connections, DATABASE_URL in a project's .env (optional project_dir), and every database of any running local PostgreSQL (Homebrew, Postgres.app, Sawit). Shows name, host, port, database, user and where it was found"
    )]
    async fn detect_databases(&self, Parameters(a): Parameters<DetectArgs>) -> CallToolResult {
        self.detect_databases_tool(a.project_dir.as_deref()).await
    }

    #[tool(description = "List the databases on a server (instance or saved connection)")]
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
        description = "Run one SQL statement READ-ONLY (writes are rejected) and return up to 500 rows as a text table. Use this for SELECTs; it cannot modify anything"
    )]
    async fn query(&self, Parameters(a): Parameters<QueryArgs>) -> CallToolResult {
        self.query_tool(&a.target, &a.sql, a.database.as_deref(), a.max_rows)
            .await
    }

    #[tool(
        description = "Execute SQL READ-WRITE with full privileges (postgres superuser on instances, the saved credentials on connections). CAN MODIFY OR DELETE DATA AND SCHEMA (INSERT/UPDATE/DELETE/DDL); several statements allowed. Prefer `query` for reads"
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

    #[tool(description = "Remove a saved connection (and its Keychain password)")]
    async fn remove_connection(&self, Parameters(a): Parameters<NameArgs>) -> CallToolResult {
        self.remove_connection_tool(&a.name).await
    }

    #[tool(
        description = "Write DATABASE_URL for a Sawit instance into <project_dir>/.env, e.g. for a TanStack Start app"
    )]
    async fn write_env(&self, Parameters(a): Parameters<WriteEnvArgs>) -> CallToolResult {
        self.write_env_tool(&a.instance, &a.project_dir).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawit_core::db::ResultSet;
    use std::time::Duration;

    #[test]
    fn confirm_must_match_exactly() {
        assert!(check_confirm("shop", "shop", "drop database").is_ok());
        let e = check_confirm("shop", "Shop", "drop database").unwrap_err();
        assert!(e.contains("confirm=\"shop\""), "{e}");
        assert!(check_confirm("shop", "", "drop database").is_err());
    }

    #[test]
    fn create_database_quotes_identifiers() {
        assert_eq!(create_database_sql("shop", None, None), "CREATE DATABASE \"shop\"");
        assert_eq!(
            create_database_sql("a\"b", Some("app"), Some("template0")),
            "CREATE DATABASE \"a\"\"b\" OWNER \"app\" TEMPLATE \"template0\""
        );
        assert_eq!(
            create_database_sql("x", Some(" "), None),
            "CREATE DATABASE \"x\""
        );
    }

    #[test]
    fn outcome_lists_tables_then_affected_rows() {
        let outcome = QueryOutcome {
            results: vec![ResultSet {
                columns: vec!["id".into()],
                rows: vec![vec![Some("1".into())]],
                truncated: false,
            }],
            affected: vec![3],
            duration: Duration::from_millis(7),
        };
        let text = format_outcome(&outcome);
        assert!(text.contains("id\n--\n1\n(1 row)"), "{text}");
        assert!(text.contains("3 rows affected"));
        assert!(text.ends_with("(7 ms)"));
        let many = QueryOutcome {
            affected: vec![1, 0],
            ..QueryOutcome::default()
        };
        let text = format_outcome(&many);
        assert!(text.contains("statement 1: 1 row affected\nstatement 2: 0 rows affected"));
    }

    #[test]
    fn via_labels_in_english() {
        assert_eq!(english_via("koneksi tersimpan prod"), "saved connection prod");
        assert_eq!(english_via(".env di ~/code/app"), ".env in ~/code/app");
        assert_eq!(english_via("instance shop"), "instance shop");
    }
}
