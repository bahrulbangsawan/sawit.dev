//! Connections: profiles, DATABASE_URL/.env import, Keychain passwords,
//! schema browsing, paged table reads and ad-hoc SQL.

mod profile;
mod query;

pub use profile::{
    ConnProfile, Profiles, delete_password, dotenv_upsert, dotenv_value, load_password,
    parse_database_url, read_dotenv_database_url, store_password,
};

pub use query::{
    ColumnInfo, QueryOutcome, RelKind, Relation, ResultSet, SchemaInfo, connect, connect_read_only,
    fetch_page, list_databases, load_schema, row_count, run_read_only, run_sql, sql_error,
};

/// Quotes an SQL identifier.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests;
