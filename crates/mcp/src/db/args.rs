//! The arguments of the database tools, as an agent sends them.

use rmcp::schemars;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetArgs {
    #[schemars(
        description = "Sawit instance name, saved connection name (see list_connections), or a D1/SQLite id from detect_databases (d1-local:…, d1-remote:…, sqlite:…)"
    )]
    pub target: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SchemaArgs {
    #[schemars(
        description = "Sawit instance name, saved connection name, or a D1/SQLite id from detect_databases (d1-local:…, d1-remote:…, sqlite:…)"
    )]
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
    #[schemars(
        description = "Sawit instance name, saved connection name, or a D1/SQLite id from detect_databases (d1-local:…, d1-remote:…, sqlite:…)"
    )]
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
    #[schemars(
        description = "Sawit instance name, saved connection name, or a D1/SQLite id from detect_databases (d1-local:…, d1-remote:…, sqlite:…)"
    )]
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
    #[schemars(
        description = "Sawit instance name, saved connection name, or a D1/SQLite id from detect_databases (d1-local:…, d1-remote:…, sqlite:…)"
    )]
    pub target: String,
    #[schemars(
        description = "SQL to execute with full privileges; may hold several statements separated by `;`"
    )]
    pub sql: String,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "Maximum rows per result set (default and cap: 500)")]
    pub max_rows: Option<usize>,
    #[schemars(
        description = "Needed (true) only for SQL that writes to a remote (production) D1 database (a d1-remote: target). Default false"
    )]
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DetectArgs {
    #[schemars(
        description = "Project folder whose .env (and the .env files above it) DATABASE_URL should be included, and whose Wrangler config's D1 databases (local and remote) are listed"
    )]
    pub project_dir: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CreateDbArgs {
    #[schemars(description = "Sawit instance name or saved connection name (PostgreSQL only)")]
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
    #[schemars(description = "Sawit instance name or saved connection name (PostgreSQL only)")]
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
pub struct RemoveConnectionArgs {
    #[schemars(description = "Saved connection name")]
    pub name: String,
    #[schemars(
        description = "Must equal `name` exactly, to confirm: the connection and its Keychain password are deleted for good"
    )]
    pub confirm: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WriteEnvArgs {
    #[schemars(description = "Sawit instance name")]
    pub instance: String,
    #[schemars(description = "Project folder whose .env gets DATABASE_URL (absolute path)")]
    pub project_dir: String,
    #[schemars(
        description = "Needed (true) only when the .env already sets a different DATABASE_URL, which this replaces. Default false"
    )]
    #[serde(default)]
    pub confirm: bool,
}
