//! Describing a database: its schemas with their tables and views (`describe_schema`), and one
//! table or view in full (`describe_table`).

use rmcp::model::CallToolResult;
use sawit_core::db::{self, Conn};
use tokio_postgres::types::ToSql;

use super::TableArgs;
use crate::{Sawit, fail, ok, tri};

/// A table of a SQLite or D1 database, as `describe_table` tells it.
async fn describe_lite_table(conn: &Conn, table: &str) -> Result<String, String> {
    let schemas = conn.load_schema().await.map_err(|e| e.to_string())?;
    let (_, rel) = db::find_relation(&schemas, None, table).map_err(|e| e.to_string())?;
    let rows = conn
        .row_count_matching(db::sqlite::MAIN, table, &Default::default())
        .await
        .ok()
        .flatten()
        .map(|c| format!(" · {} rows", c.rows))
        .unwrap_or_default();
    let kind = match rel.kind {
        db::RelKind::View => "view",
        _ => "table",
    };
    let mut out = vec![format!(
        "{kind} {}{rows} ({})",
        rel.name,
        conn.kind().title()
    )];
    out.push(String::new());
    out.push("Columns:".into());
    for c in &rel.columns {
        out.push(format!(
            "  {} {}{}{}",
            c.name,
            c.data_type,
            if c.nullable { "" } else { " NOT NULL" },
            if rel.primary_key.contains(&c.name) {
                " PK"
            } else {
                ""
            }
        ));
    }
    if !rel.constraints.is_empty() {
        out.push(String::new());
        out.push("Constraints:".into());
        out.extend(
            rel.constraints
                .iter()
                .map(|c| format!("  {}: {}", c.kind.label().to_uppercase(), c.definition)),
        );
    }
    if !rel.indexes.is_empty() {
        out.push(String::new());
        out.push("Indexes:".into());
        out.extend(rel.indexes.iter().map(|i| format!("  {}", i.definition)));
    }
    let referenced: Vec<String> = schemas
        .iter()
        .flat_map(|s| &s.relations)
        .flat_map(|r| {
            r.foreign_keys
                .iter()
                .filter(|fk| fk.ref_table == rel.name)
                .map(move |fk| {
                    format!(
                        "  {} ({}) → {}({})",
                        r.name,
                        fk.columns.join(", "),
                        fk.ref_table,
                        fk.ref_columns.join(", ")
                    )
                })
        })
        .collect();
    if !referenced.is_empty() {
        out.push(String::new());
        out.push("Referenced by:".into());
        out.extend(referenced);
    }
    Ok(out.join("\n"))
}

impl Sawit {
    pub async fn describe_schema_tool(
        &self,
        target: &str,
        schema: Option<&str>,
        database: Option<&str>,
    ) -> CallToolResult {
        let schemas = match tri!(self.lite_conn(target, true)) {
            Some(conn) => tri!(conn.load_schema().await.map_err(|e| e.to_string())),
            None => {
                let client = tri!(self.connect_target(target, database).await);
                tri!(db::load_schema(&client).await.map_err(|e| e.to_string()))
            }
        };
        let mut out = Vec::new();
        for s in schemas
            .iter()
            .filter(|s| schema.is_none_or(|o| o == s.name))
        {
            out.push(format!("schema {}", s.name));
            for r in &s.relations {
                let rows = r
                    .estimated_rows
                    .map(|n| format!(" (~{n} rows)"))
                    .unwrap_or_default();
                out.push(format!("  {:?} {}.{}{rows}", r.kind, s.name, r.name));
                for c in &r.columns {
                    let pk = if r.primary_key.contains(&c.name) {
                        " PK"
                    } else {
                        ""
                    };
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
                Some(s) => fail(format!(
                    "No schema named \"{s}\" (or it has nothing visible)."
                )),
                None => ok("No user schemas."),
            };
        }
        ok(out.join("\n"))
    }

    pub async fn describe_table_tool(&self, a: TableArgs) -> CallToolResult {
        if let Some(conn) = tri!(self.lite_conn(&a.target, true)) {
            return match describe_lite_table(&conn, &a.table).await {
                Ok(text) => ok(text),
                Err(e) => fail(e),
            };
        }
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
            [] => {
                return fail(format!(
                    "No table or view named \"{}\". See describe_schema.",
                    a.table
                ));
            }
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
        let columns_sql = format!(
            "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), a.attnotnull,
                            pg_get_expr(d.adbin, d.adrelid), col_description(a.attrelid, a.attnum)
                     FROM pg_attribute a
                     LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
                     WHERE a.attrelid = {rel} AND a.attnum > 0 AND NOT a.attisdropped
                     ORDER BY a.attnum"
        );
        let constraints_sql = format!(
            "SELECT conname::text, contype::text, pg_get_constraintdef(oid)
                     FROM pg_constraint WHERE conrelid = {rel}
                     ORDER BY array_position(ARRAY['p','f','u','c','x'], contype::text), conname"
        );
        let indexes_sql = format!(
            "SELECT i.relname::text, pg_get_indexdef(x.indexrelid)
                     FROM pg_index x JOIN pg_class i ON i.oid = x.indexrelid
                     WHERE x.indrelid = {rel} ORDER BY x.indisprimary DESC, i.relname"
        );
        let referenced_sql = format!(
            "SELECT conname::text, conrelid::regclass::text, pg_get_constraintdef(oid)
                     FROM pg_constraint WHERE confrelid = {rel} AND contype = 'f' ORDER BY 2, 1"
        );
        let on: [&(dyn ToSql + Sync); 2] = [&schema, &a.table];
        // The four go to the server together, in this order, rather than each one after the
        // answer to the one before, so their round trips (a prepare and an execute each) overlap.
        let (columns, constraints, indexes, referenced) = tri!(
            tokio::try_join!(
                client.query(&columns_sql, &on),
                client.query(&constraints_sql, &on),
                client.query(&indexes_sql, &on),
                client.query(&referenced_sql, &on),
            )
            .map_err(err)
        );

        let mut out = vec![format!(
            "{kind} {schema}.{}{}",
            a.table,
            estimate
                .map(|n| format!(" · ~{n} rows (estimate)"))
                .unwrap_or_default()
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
}
