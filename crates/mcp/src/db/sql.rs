//! Running SQL: one read-only statement (`query`), read-write SQL (`execute_sql`), and creating and
//! dropping databases.

use rmcp::model::CallToolResult;
use sawit_core::db::{self, Conn, DbKind, QueryOutcome, ReadOnlyTry, format_table, quote_ident};

use super::{CreateDbArgs, DropDbArgs, ExecuteArgs, MAX_ROWS, check_confirm};
use crate::{Sawit, fail, ok, tri};

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

/// `CREATE DATABASE` with quoted identifiers.
pub(crate) fn create_database_sql(
    name: &str,
    owner: Option<&str>,
    template: Option<&str>,
) -> String {
    let mut sql = format!("CREATE DATABASE {}", quote_ident(name));
    if let Some(owner) = owner.filter(|o| !o.trim().is_empty()) {
        sql.push_str(&format!(" OWNER {}", quote_ident(owner.trim())));
    }
    if let Some(template) = template.filter(|t| !t.trim().is_empty()) {
        sql.push_str(&format!(" TEMPLATE {}", quote_ident(template.trim())));
    }
    sql
}

impl Sawit {
    pub async fn query_tool(
        &self,
        target: &str,
        sql: &str,
        database: Option<&str>,
        max_rows: Option<usize>,
    ) -> CallToolResult {
        let limit = max_rows.unwrap_or(MAX_ROWS).clamp(1, MAX_ROWS);
        if let Some(conn) = tri!(self.lite_conn(target, true)) {
            return match conn.run_read_only(sql, limit).await {
                Ok(outcome) => ok(format_outcome(&outcome)),
                Err(e) => fail(format!(
                    "{e}\n(query runs read-only; use execute_sql to change data)"
                )),
            };
        }
        let client = tri!(self.connect_target(target, database).await);
        match db::run_read_only(&client, sql, limit).await {
            Ok(outcome) => ok(format_outcome(&outcome)),
            Err(e) => fail(format!(
                "{e}\n(query runs read-only; use execute_sql to change data)"
            )),
        }
    }

    pub async fn execute_sql_tool(&self, a: ExecuteArgs) -> CallToolResult {
        let limit = a.max_rows.unwrap_or(MAX_ROWS).clamp(1, MAX_ROWS);
        if let Some(Conn::Lite(lite)) = tri!(self.lite_conn(&a.target, false)) {
            // Reads run read-only on the file (or remotely); only writes take a write route.
            match lite.run_if_read_only(&a.sql, limit).await {
                Ok(ReadOnlyTry::Read(outcome)) => return ok(format_outcome(&outcome)),
                Ok(_) => {}
                Err(e) => return fail(e.to_string()),
            }
            if lite.profile.kind == DbKind::D1Remote && !a.confirm {
                return fail(format!(
                    "Refused: this SQL writes to {}, a remote (production) D1 database on Cloudflare. Nothing was run. Pass confirm=true to run it.",
                    lite.profile.database
                ));
            }
            return match lite.write(&a.sql, limit).await {
                Ok((outcome, route)) => ok(format!(
                    "{}\n\n{}",
                    format_outcome(&outcome),
                    route.describe()
                )),
                Err(e) => fail(format!("{e}\nNothing was changed.")),
            };
        }
        let client = tri!(self.connect_admin(&a.target, a.database.as_deref()).await);
        match db::run_sql(&client, &a.sql, limit).await {
            Ok(outcome) => ok(format_outcome(&outcome)),
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
}
