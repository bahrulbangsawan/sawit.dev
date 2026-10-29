//! Whole-object database jobs, as in the database sidebar's menus: export a
//! table or a query to CSV/Excel, `pg_dump` backups, loading a CSV file into
//! a table, and dropping, emptying or refreshing relations.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::binaries::{SearchRoots, discover};
use sawit_core::db::{self, BackupScope, ExportSummary, RelKind};

use crate::db::check_confirm;
use crate::{Sawit, expand_home, fail, ok, tilde, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExportTableArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "Schema of the table (default: public, or the only match)")]
    pub schema: Option<String>,
    #[schemars(description = "Table or view to export")]
    pub table: String,
    #[schemars(
        description = "Absolute path of the file to write; ending in .csv or .xlsx picks the format"
    )]
    pub path: String,
    #[schemars(description = "Replace the file if it exists (default false)")]
    pub overwrite: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExportQueryArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "One read-only statement (SELECT …) whose rows are exported")]
    pub sql: String,
    #[schemars(
        description = "Absolute path of the file to write; ending in .csv or .xlsx picks the format"
    )]
    pub path: String,
    #[schemars(description = "Replace the file if it exists (default false)")]
    pub overwrite: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BackupArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Database to back up (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "Back up only this schema (or the schema of `table`)")]
    pub schema: Option<String>,
    #[schemars(description = "Back up only this table")]
    pub table: Option<String>,
    #[schemars(
        description = "Absolute path of the backup file: .dump or .backup writes pg_dump's custom format (for pg_restore), anything else plain SQL"
    )]
    pub path: String,
    #[schemars(description = "Replace the file if it exists (default false)")]
    pub overwrite: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ImportCsvArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "Schema of the table (default: public, or the only match)")]
    pub schema: Option<String>,
    #[schemars(description = "Existing table the rows go into")]
    pub table: String,
    #[schemars(
        description = "Absolute path of a CSV file whose first line names the table's columns"
    )]
    pub path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DropRelationArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "Schema (default: public, or the only match)")]
    pub schema: Option<String>,
    #[schemars(description = "Table, view, materialized view or foreign table to drop")]
    pub name: String,
    #[schemars(description = "Also drop what depends on it (default false)")]
    pub cascade: Option<bool>,
    #[schemars(description = "Must equal `name` exactly, to confirm")]
    pub confirm: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TruncateArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "Schema (default: public, or the only match)")]
    pub schema: Option<String>,
    #[schemars(description = "Table to empty")]
    pub table: String,
    #[schemars(description = "Must equal `table` exactly, to confirm")]
    pub confirm: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RefreshArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "Schema (default: public, or the only match)")]
    pub schema: Option<String>,
    #[schemars(description = "Materialized view to refresh")]
    pub name: String,
}

/// A file to write, given by an agent: absolute (`~` expanded), in an
/// existing folder, and new unless `overwrite`.
pub(crate) fn output_file(path: &str, overwrite: bool) -> Result<PathBuf, String> {
    let file = expand_home(path.trim());
    if !file.is_absolute() {
        return Err(format!("“{path}” must be an absolute path."));
    }
    if !file.parent().is_some_and(|d| d.is_dir()) {
        return Err(format!("The folder of {} does not exist.", file.display()));
    }
    if file.is_dir() {
        return Err(format!("{} is a folder; give a file name.", file.display()));
    }
    if file.exists() && !overwrite {
        return Err(format!(
            "{} already exists. Pass overwrite=true to replace it.",
            file.display()
        ));
    }
    Ok(file)
}

fn exported(summary: ExportSummary, path: &std::path::Path) -> String {
    let mut text = format!(
        "Exported {} {} to {}.",
        summary.rows,
        if summary.rows == 1 { "row" } else { "rows" },
        tilde(path)
    );
    if summary.truncated {
        text.push_str(" Truncated: Excel holds at most 1,048,576 rows; export to .csv for all.");
    }
    text
}

impl Sawit {
    /// The schema and relation `name` names on an admin connection.
    async fn relation(
        &self,
        client: &tokio_postgres::Client,
        schema: Option<&str>,
        name: &str,
    ) -> Result<(String, RelKind), String> {
        let schemas = db::load_schema(client).await.map_err(|e| e.to_string())?;
        let (schema, relation) =
            db::find_relation(&schemas, schema, name).map_err(|e| e.to_string())?;
        Ok((schema.to_string(), relation.kind))
    }

    pub async fn export_table_tool(&self, a: ExportTableArgs) -> CallToolResult {
        let path = tri!(output_file(&a.path, a.overwrite.unwrap_or(false)));
        let client = tri!(self.connect_target(&a.target, a.database.as_deref()).await);
        match db::export_table(&client, a.schema.as_deref(), &a.table, &path).await {
            Ok(summary) => ok(exported(summary, &path)),
            Err(e) => fail(e.to_string()),
        }
    }

    pub async fn export_query_tool(&self, a: ExportQueryArgs) -> CallToolResult {
        let path = tri!(output_file(&a.path, a.overwrite.unwrap_or(false)));
        let client = tri!(self.connect_target(&a.target, a.database.as_deref()).await);
        match db::export_query(&client, &a.sql, &path).await {
            Ok(summary) => ok(exported(summary, &path)),
            Err(e) => fail(format!("{e}\n(the query runs read-only)")),
        }
    }

    pub async fn backup_tool(&self, a: BackupArgs) -> CallToolResult {
        let path = tri!(output_file(&a.path, a.overwrite.unwrap_or(false)));
        let (profile, password) = tri!(self.admin_login(&a.target, a.database.as_deref()).await);
        let client = tri!(
            db::connect(&profile, password.as_deref())
                .await
                .map_err(|e| e.to_string())
        );
        let scope = match (a.schema, a.table) {
            (schema, Some(table)) => {
                let (schema, _) = tri!(self.relation(&client, schema.as_deref(), &table).await);
                BackupScope::Table { schema, table }
            }
            (Some(schema), None) => BackupScope::Schema(schema),
            (None, None) => BackupScope::Database,
        };
        let roots = SearchRoots::detect(self.paths.managed_bin_root()).await;
        let binaries = discover(&roots).await;
        match db::backup(
            &client,
            &profile,
            password.as_deref(),
            &binaries,
            &scope,
            &path,
        )
        .await
        {
            Ok(()) => {
                let what = match &scope {
                    BackupScope::Database => format!("database {}", profile.database),
                    BackupScope::Schema(s) => format!("schema {s} of {}", profile.database),
                    BackupScope::Table { schema, table } => {
                        format!("table {schema}.{table} of {}", profile.database)
                    }
                };
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                ok(format!(
                    "Backed up {what} to {} ({size} bytes, {}).",
                    tilde(&path),
                    match db::BackupFormat::for_path(&path) {
                        db::BackupFormat::Custom => "custom format, restore with pg_restore",
                        db::BackupFormat::Sql => "plain SQL",
                    }
                ))
            }
            Err(e) => fail(e.to_string()),
        }
    }

    pub async fn import_csv_tool(&self, a: ImportCsvArgs) -> CallToolResult {
        let path = expand_home(a.path.trim());
        if !path.is_absolute() || !path.is_file() {
            return fail(format!("{} is not a file (give an absolute path).", a.path));
        }
        let client = tri!(self.connect_admin(&a.target, a.database.as_deref()).await);
        let (schema, kind) = tri!(self.relation(&client, a.schema.as_deref(), &a.table).await);
        if kind != RelKind::Table {
            return fail(format!("{schema}.{} is not a table.", a.table));
        }
        match db::import_csv(&client, &schema, &a.table, &path).await {
            Ok(n) => ok(format!(
                "Imported {n} {} into {schema}.{}.",
                if n == 1 { "row" } else { "rows" },
                a.table
            )),
            Err(e) => fail(format!("{e}\nNothing was imported.")),
        }
    }

    pub async fn drop_relation_tool(&self, a: DropRelationArgs) -> CallToolResult {
        tri!(check_confirm(&a.name, &a.confirm, "drop"));
        let client = tri!(self.connect_admin(&a.target, a.database.as_deref()).await);
        let (schema, kind) = tri!(self.relation(&client, a.schema.as_deref(), &a.name).await);
        let cascade = a.cascade.unwrap_or(false);
        let sql = db::drop_sql(&schema, &a.name, kind, cascade);
        match db::run_bulk(&client, &sql).await {
            Ok(_) => ok(format!("Dropped {schema}.{}. ({sql})", a.name)),
            Err(e) if !cascade => fail(format!(
                "{e}\nIf other objects depend on it, pass cascade=true to drop them too."
            )),
            Err(e) => fail(e.to_string()),
        }
    }

    pub async fn truncate_table_tool(&self, a: TruncateArgs) -> CallToolResult {
        tri!(check_confirm(&a.table, &a.confirm, "empty table"));
        let client = tri!(self.connect_admin(&a.target, a.database.as_deref()).await);
        let (schema, kind) = tri!(self.relation(&client, a.schema.as_deref(), &a.table).await);
        if kind != RelKind::Table {
            return fail(format!("{schema}.{} is not a table.", a.table));
        }
        match db::run_bulk(&client, &db::truncate_sql(&schema, &a.table)).await {
            Ok(_) => ok(format!("Emptied {schema}.{} (every row removed).", a.table)),
            Err(e) => fail(e.to_string()),
        }
    }

    pub async fn refresh_matview_tool(&self, a: RefreshArgs) -> CallToolResult {
        let client = tri!(self.connect_admin(&a.target, a.database.as_deref()).await);
        let (schema, kind) = tri!(self.relation(&client, a.schema.as_deref(), &a.name).await);
        if kind != RelKind::MaterializedView {
            return fail(format!("{schema}.{} is not a materialized view.", a.name));
        }
        match db::run_bulk(&client, &db::refresh_matview_sql(&schema, &a.name)).await {
            Ok(_) => ok(format!("Refreshed {schema}.{}.", a.name)),
            Err(e) => fail(e.to_string()),
        }
    }
}

#[tool_router(router = db_admin_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Export every row of a table or view to a CSV or Excel (.xlsx) file, in primary-key order, like the table's Export menu. Reads only; returns the row count and path. Works without the app"
    )]
    async fn export_table(&self, Parameters(a): Parameters<ExportTableArgs>) -> CallToolResult {
        self.export_table_tool(a).await
    }

    #[tool(
        description = "Run one read-only SELECT and write its rows to a CSV or Excel (.xlsx) file. Returns the row count and path. Works without the app"
    )]
    async fn export_query(&self, Parameters(a): Parameters<ExportQueryArgs>) -> CallToolResult {
        self.export_query_tool(a).await
    }

    #[tool(
        description = "Back up a database, one schema or one table with pg_dump (the newest installed PostgreSQL that can dump this server). .dump/.backup files are custom format for pg_restore, others plain SQL. Returns the file size. Works without the app"
    )]
    async fn backup_database(&self, Parameters(a): Parameters<BackupArgs>) -> CallToolResult {
        self.backup_tool(a).await
    }

    #[tool(
        description = "Load a CSV file into an existing table with COPY (header row names the columns; empty unquoted fields are NULL). WRITES DATA: adds rows, all or none. Returns the rows added. Works without the app"
    )]
    async fn import_csv(&self, Parameters(a): Parameters<ImportCsvArgs>) -> CallToolResult {
        self.import_csv_tool(a).await
    }

    #[tool(
        description = "DROP (permanently delete) a table, view, materialized view or foreign table; cascade=true also drops what depends on it. DESTRUCTIVE: `confirm` must equal `name`, otherwise nothing happens"
    )]
    async fn drop_relation(&self, Parameters(a): Parameters<DropRelationArgs>) -> CallToolResult {
        self.drop_relation_tool(a).await
    }

    #[tool(
        description = "TRUNCATE a table: permanently remove every row. DESTRUCTIVE: `confirm` must equal `table`, otherwise nothing happens"
    )]
    async fn truncate_table(&self, Parameters(a): Parameters<TruncateArgs>) -> CallToolResult {
        self.truncate_table_tool(a).await
    }

    #[tool(description = "REFRESH a materialized view so it shows current data")]
    async fn refresh_materialized_view(
        &self,
        Parameters(a): Parameters<RefreshArgs>,
    ) -> CallToolResult {
        self.refresh_matview_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_file_must_be_absolute_new_and_in_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.csv");
        assert_eq!(output_file(file.to_str().unwrap(), false).unwrap(), file);
        assert!(
            output_file("a.csv", false)
                .unwrap_err()
                .contains("absolute")
        );
        let missing = dir.path().join("nope/a.csv");
        assert!(
            output_file(missing.to_str().unwrap(), false)
                .unwrap_err()
                .contains("does not exist")
        );
        std::fs::write(&file, "x").unwrap();
        assert!(
            output_file(file.to_str().unwrap(), false)
                .unwrap_err()
                .contains("overwrite=true")
        );
        assert!(output_file(file.to_str().unwrap(), true).is_ok());
        assert!(
            output_file(dir.path().to_str().unwrap(), true)
                .unwrap_err()
                .contains("folder")
        );
    }

    #[test]
    fn export_summary_mentions_truncation() {
        let path = std::path::Path::new("/x/a.xlsx");
        assert_eq!(
            exported(
                ExportSummary {
                    rows: 1,
                    truncated: false
                },
                path
            ),
            "Exported 1 row to /x/a.xlsx."
        );
        assert!(
            exported(
                ExportSummary {
                    rows: 9,
                    truncated: true
                },
                path
            )
            .contains("Truncated")
        );
    }
}
