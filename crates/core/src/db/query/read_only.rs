//! Read-only runs: SQL tried inside a read-only transaction, COPY and read-only switches
//! screened out, and the read-only role agents log in as.

use tokio_postgres::Client;

use super::failure::sql_error;
use super::run::{QueryOutcome, run_statements};
use crate::{CoreError, Result};

/// Login role for agent (MCP) access to Sawit instances: reads everything,
/// cannot write, cannot run server programs or write server files.
pub const READ_ONLY_ROLE: &str = "sawit_ro";

/// Rejects SQL that the read-only mode must never run: COPY (server files /
/// programs) and attempts to switch read-only mode off.
pub fn screen_read_only_sql(sql: &str) -> Result<()> {
    let lowered = sql.to_lowercase();
    if lowered.contains("transaction_read_only") || lowered.contains("characteristics") {
        return Err(CoreError::Sql("Read-only mode can't be changed.".into()));
    }
    screen_copy(&lowered)
}

/// Rejects COPY, which reaches the server's files and programs even inside a read-only
/// transaction. `lowered` is the SQL in lower case.
fn screen_copy(lowered: &str) -> Result<()> {
    // First keyword after whitespace, comments and opening parentheses.
    let mut rest = lowered;
    loop {
        let trimmed = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '(');
        if let Some(after) = trimmed.strip_prefix("--") {
            rest = after.split_once('\n').map(|(_, r)| r).unwrap_or("");
        } else if let Some(after) = trimmed.strip_prefix("/*") {
            rest = after.split_once("*/").map(|(_, r)| r).unwrap_or("");
        } else {
            rest = trimmed;
            break;
        }
    }
    if rest.starts_with("copy") {
        return Err(CoreError::Sql(
            "COPY is not allowed in read-only mode.".into(),
        ));
    }
    Ok(())
}

/// Creates (idempotently) the non-superuser `READ_ONLY_ROLE` with
/// `pg_read_all_data` (PostgreSQL 14+). Run on an admin connection.
pub async fn ensure_read_only_role(admin: &Client) -> Result<()> {
    admin
        .batch_execute(&format!(
            "DO $$ BEGIN
               IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{READ_ONLY_ROLE}') THEN
                 CREATE ROLE {READ_ONLY_ROLE} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;
               END IF;
             END $$;
             GRANT pg_read_all_data TO {READ_ONLY_ROLE};"
        ))
        .await
        .map_err(sql_error)
}

/// Runs one SQL statement on a `connect_read_only` connection inside a
/// read-only transaction. Multiple statements are refused (the server's
/// Parse step rejects them), as are COPY and read-only switches. Use a
/// non-superuser role (see `ensure_read_only_role`) for full protection.
pub async fn run_read_only(client: &Client, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    screen_read_only_sql(sql)?;
    match run_if_read_only(client, sql, max_rows).await? {
        ReadOnlyTry::Read(outcome) => Ok(outcome),
        ReadOnlyTry::Writes(why) | ReadOnlyTry::Several(why) => Err(CoreError::Sql(why)),
    }
}

/// What trying SQL read-only found ([`run_if_read_only`]).
#[derive(Debug)]
pub enum ReadOnlyTry {
    /// It only reads: what it returned.
    Read(QueryOutcome),
    /// It writes (the server refused it in a read-only transaction), must run outside any
    /// transaction (`DROP DATABASE`, `VACUUM`, a procedure that commits), or is `COPY`, which
    /// read-only mode never runs: the refusal, in the server's or Sawit's words. Nothing of it
    /// ran.
    Writes(String),
    /// Several statements, which a read-only run can't take (a `COMMIT` among them would end its
    /// transaction): the server's refusal. None of them ran.
    Several(String),
}

/// Runs `sql` as [`run_read_only`] does, and tells SQL that only reads (its results) from what
/// the read-only run refuses for what it is: a write, or several statements. Any other error, a
/// typo or a missing table, is the statement's own.
///
/// Only `COPY` is screened out by its text. A switch of the read-only mode runs, alone, inside
/// the read-only transaction, which is rolled back, so it can't outlive the try; screening for
/// its words also caught reads that only name them, such as a `product_characteristics` table
/// or `SHOW transaction_read_only`, which `sawit <db>` then asked about as writes.
/// [`run_read_only`] still refuses those words.
pub async fn run_if_read_only(client: &Client, sql: &str, max_rows: usize) -> Result<ReadOnlyTry> {
    use tokio_postgres::error::SqlState;

    if let Err(e) = screen_copy(&sql.to_lowercase()) {
        return Ok(ReadOnlyTry::Writes(e.to_string()));
    }
    if let Err(e) = client.prepare(sql).await {
        // The server answers several statements as it answers a typo: only text with a `;`
        // before its end can be several.
        if e.code() == Some(&SqlState::SYNTAX_ERROR) && several_statements(sql) {
            return Ok(ReadOnlyTry::Several(sql_error(e).to_string()));
        }
        return Err(sql_error(e));
    }
    client
        .batch_execute("BEGIN TRANSACTION READ ONLY")
        .await
        .map_err(sql_error)?;
    let result = run_statements(client, sql, max_rows).await;
    let _ = client.batch_execute("ROLLBACK").await;
    match result {
        Ok(outcome) => Ok(ReadOnlyTry::Read(outcome)),
        // It writes (25006), must run outside a transaction (25001), or ends the transaction
        // it runs in, as a procedure that commits does (2D000).
        Err(e)
            if e.code() == Some(&SqlState::READ_ONLY_SQL_TRANSACTION)
                || e.code() == Some(&SqlState::ACTIVE_SQL_TRANSACTION)
                || e.code() == Some(&SqlState::INVALID_TRANSACTION_TERMINATION) =>
        {
            Ok(ReadOnlyTry::Writes(sql_error(e).to_string()))
        }
        Err(e) => Err(sql_error(e)),
    }
}

/// Whether `sql` may be several statements: a `;` before the end of its text (`;` at the end ends
/// the one statement). One in a string or a comment counts too; it only says why the server
/// couldn't parse it.
pub(in crate::db) fn several_statements(sql: &str) -> bool {
    sql.trim_end().trim_end_matches(';').contains(';')
}
