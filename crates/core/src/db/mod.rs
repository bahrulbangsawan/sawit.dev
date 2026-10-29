//! Connections: profiles, DATABASE_URL/.env import, Keychain passwords,
//! schema browsing, paged table reads and ad-hoc SQL.

pub mod diagram;
mod profile;
mod query;
mod rows;
mod tls;

pub use profile::{
    ChannelBinding, ConnProfile, HostPort, Profiles, SessionAttrs, SslMode, delete_password,
    dotenv_upsert, dotenv_value, load_password, parse_database_url, pgpass_password,
    read_dotenv_database_url, resolve_password, store_password,
};

pub use rows::{FilterOp, RowFilter, RowQuery, SortDir, quote_literal};

pub use query::{
    ColumnInfo, Constraint, ConstraintKind, FailureKind, ForeignKey, IndexInfo, QueryOutcome, READ_ONLY_ROLE, RelKind, Relation, ResultSet, RowCount,
    SchemaInfo, cancel, connect, connect_all, failure_kind, connect_read_only, ensure_read_only_role, fetch_page, fetch_rows, row_count_matching,
    list_databases, load_schema, primary_key, row_count, row_count_estimated, row_count_within,
    run_read_only, run_sql, screen_read_only_sql, sql_error,
};

/// Quotes an SQL identifier.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests;

/// Sets DATABASE_URL in `<project>/.env` (creating it if needed).
pub fn write_database_url(
    project_dir: &std::path::Path,
    url: &str,
) -> Result<std::path::PathBuf, String> {
    if !project_dir.is_dir() {
        return Err(format!("{} is not a folder.", project_dir.display()));
    }
    let path = project_dir.join(".env");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    crate::files::write_text_atomic(&path, &dotenv_upsert(&current, "DATABASE_URL", url))
        .map_err(|e| e.to_string())?;
    Ok(path)
}

/// Plain-text table for terminals and MCP clients.
pub fn format_table(set: &ResultSet) -> String {
    let cell = |v: &Option<String>| v.clone().unwrap_or_else(|| "NULL".into());
    let mut widths: Vec<usize> = set.columns.iter().map(|c| c.chars().count()).collect();
    for row in &set.rows {
        for (w, v) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell(v).chars().count());
        }
    }
    let line = |values: Vec<String>| {
        let cols: Vec<String> = values
            .iter()
            .zip(&widths)
            .map(|(v, w)| format!("{v:<w$}"))
            .collect();
        cols.join(" | ").trim_end().to_string()
    };
    let mut out = vec![line(set.columns.clone())];
    out.push(
        widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("-+-"),
    );
    for row in &set.rows {
        out.push(line(row.iter().map(cell).collect()));
    }
    out.push(format!(
        "({} {}{})",
        set.rows.len(),
        if set.rows.len() == 1 { "row" } else { "rows" },
        if set.truncated { ", truncated" } else { "" }
    ));
    out.join("\n")
}
