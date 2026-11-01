//! The schema tree: schemas, their tables and views with columns, keys, constraints, indexes
//! and row estimates.

use tokio_postgres::types::Type;
use tokio_postgres::{Client, SimpleQueryMessage};

use super::failure::sql_error;
use crate::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RelKind {
    #[default]
    Table,
    View,
    MaterializedView,
    Foreign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

/// A foreign key from a relation's `columns` to `ref_columns` of another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

/// What a constraint enforces (`pg_constraint.contype`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    PrimaryKey,
    ForeignKey,
    Unique,
    Check,
    Exclusion,
    /// `NOT NULL` as a catalogued constraint (PostgreSQL 18+), or a kind
    /// a newer server adds.
    Other,
}

impl ConstraintKind {
    fn from_contype(contype: &str) -> ConstraintKind {
        match contype {
            "p" => ConstraintKind::PrimaryKey,
            "f" => ConstraintKind::ForeignKey,
            "u" => ConstraintKind::Unique,
            "c" => ConstraintKind::Check,
            "x" => ConstraintKind::Exclusion,
            _ => ConstraintKind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConstraintKind::PrimaryKey => "primary key",
            ConstraintKind::ForeignKey => "foreign key",
            ConstraintKind::Unique => "unique",
            ConstraintKind::Check => "check",
            ConstraintKind::Exclusion => "exclusion",
            ConstraintKind::Other => "constraint",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    pub name: String,
    pub kind: ConstraintKind,
    /// `pg_get_constraintdef`: `CHECK ((qty > 0))`, `FOREIGN KEY (…) REFERENCES …`.
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexInfo {
    pub name: String,
    /// Access method: btree, hash, gin, gist, brin, …
    pub method: String,
    pub unique: bool,
    pub primary: bool,
    /// `pg_get_indexdef`: the full `CREATE INDEX` statement.
    pub definition: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relation {
    pub name: String,
    pub kind: RelKind,
    pub columns: Vec<ColumnInfo>,
    /// The connected role may SELECT from it.
    pub readable: bool,
    /// Primary-key columns in key order (empty for views and keyless tables).
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
    /// Every constraint, by name (tables only).
    pub constraints: Vec<Constraint>,
    /// Indexes, by name (tables and materialized views).
    pub indexes: Vec<IndexInfo>,
    /// The planner's row estimate; `None` before the first ANALYZE.
    pub estimated_rows: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaInfo {
    pub name: String,
    pub relations: Vec<Relation>,
}

pub async fn list_databases(client: &Client) -> Result<Vec<String>> {
    let rows = client
        .query_typed(
            "SELECT datname FROM pg_database WHERE NOT datistemplate ORDER BY datname",
            &[],
        )
        .await
        .map_err(sql_error)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// System schemas, hidden from the tree (`n` is pg_namespace).
const USER_SCHEMAS: &str = "n.nspname NOT IN ('pg_catalog', 'information_schema')
      AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp%'";

/// User schemas with their tables/views, columns, constraints, indexes and
/// row estimates (system schemas hidden), in one round trip: four
/// statements sent as one simple query, which also works through
/// transaction-mode poolers.
pub async fn load_schema(client: &Client) -> Result<Vec<SchemaInfo>> {
    let sql = format!(
        "SELECT n.nspname FROM pg_namespace n WHERE {USER_SCHEMAS} ORDER BY 1;
        WITH rels AS (
          SELECT c.oid, n.nspname, c.relname, c.relkind, c.reltuples::bigint AS est,
                 has_table_privilege(c.oid, 'SELECT') AS readable
          FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
          WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND {USER_SCHEMAS})
        SELECT r.nspname, r.relname, r.relkind::text, r.est, r.readable,
               a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull
        FROM rels r
        LEFT JOIN pg_attribute a ON a.attrelid = r.oid AND a.attnum > 0 AND NOT a.attisdropped
        ORDER BY r.nspname, r.relname, a.attnum;
        SELECT n.nspname, c.relname, con.contype::text,
          (SELECT json_agg(a.attname ORDER BY k.o) FROM unnest(con.conkey) WITH ORDINALITY k(n, o)
             JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.n)::text,
          fn.nspname, fc.relname,
          (SELECT json_agg(a.attname ORDER BY k.o) FROM unnest(con.confkey) WITH ORDINALITY k(n, o)
             JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.n)::text,
          con.conname, pg_get_constraintdef(con.oid)
        FROM pg_constraint con
        JOIN pg_class c ON c.oid = con.conrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        LEFT JOIN pg_class fc ON fc.oid = con.confrelid
        LEFT JOIN pg_namespace fn ON fn.oid = fc.relnamespace
        WHERE {USER_SCHEMAS}
        ORDER BY n.nspname, c.relname, con.conname;
        SELECT n.nspname, c.relname, ic.relname, am.amname, i.indisunique, i.indisprimary,
               pg_get_indexdef(i.indexrelid)
        FROM pg_index i
        JOIN pg_class c ON c.oid = i.indrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        JOIN pg_class ic ON ic.oid = i.indexrelid
        JOIN pg_am am ON am.oid = ic.relam
        WHERE c.relkind IN ('r', 'p', 'm') AND {USER_SCHEMAS}
        ORDER BY n.nspname, c.relname, ic.relname"
    );
    let mut statement = 0;
    let mut schemas: Vec<SchemaInfo> = Vec::new();
    for msg in client.simple_query(&sql).await.map_err(sql_error)? {
        let row = match msg {
            SimpleQueryMessage::Row(row) => row,
            SimpleQueryMessage::CommandComplete(_) => {
                statement += 1;
                continue;
            }
            _ => continue,
        };
        let text = |i: usize| row.get(i).unwrap_or_default().to_string();
        let names = |i: usize| -> Vec<String> {
            row.get(i)
                .and_then(|v| serde_json::from_str(v).ok())
                .unwrap_or_default()
        };
        match statement {
            0 => schemas.push(SchemaInfo {
                name: text(0),
                relations: vec![],
            }),
            1 => {
                let (schema, name) = (text(0), text(1));
                let Some(s) = schemas.iter_mut().find(|s| s.name == schema) else {
                    continue;
                };
                if s.relations.last().is_none_or(|r| r.name != name) {
                    let kind = match row.get(2) {
                        Some("v") => RelKind::View,
                        Some("m") => RelKind::MaterializedView,
                        Some("f") => RelKind::Foreign,
                        _ => RelKind::Table,
                    };
                    s.relations.push(Relation {
                        name: name.clone(),
                        kind,
                        readable: row.get(4) == Some("t"),
                        estimated_rows: row
                            .get(3)
                            .and_then(|v| v.parse::<i64>().ok())
                            .filter(|n| *n >= 0),
                        ..Relation::default()
                    });
                }
                if let Some(col) = row.get(5) {
                    let rel = s.relations.last_mut().expect("pushed above");
                    rel.columns.push(ColumnInfo {
                        name: col.to_string(),
                        data_type: text(6),
                        nullable: row.get(7) == Some("t"),
                    });
                }
            }
            _ => {
                let (schema, name) = (text(0), text(1));
                let Some(rel) = schemas
                    .iter_mut()
                    .find(|s| s.name == schema)
                    .and_then(|s| s.relations.iter_mut().find(|r| r.name == name))
                else {
                    continue;
                };
                if statement == 3 {
                    rel.indexes.push(IndexInfo {
                        name: text(2),
                        method: text(3),
                        unique: row.get(4) == Some("t"),
                        primary: row.get(5) == Some("t"),
                        definition: text(6),
                    });
                    continue;
                }
                let kind = ConstraintKind::from_contype(row.get(2).unwrap_or_default());
                match kind {
                    ConstraintKind::PrimaryKey => rel.primary_key = names(3),
                    ConstraintKind::ForeignKey => rel.foreign_keys.push(ForeignKey {
                        columns: names(3),
                        ref_schema: text(4),
                        ref_table: text(5),
                        ref_columns: names(6),
                    }),
                    _ => {}
                }
                rel.constraints.push(Constraint {
                    name: text(7),
                    kind,
                    definition: text(8),
                });
            }
        }
    }
    Ok(schemas)
}

/// Primary-key columns of a table, in key order (empty for views and
/// tables without one).
pub async fn primary_key(client: &Client, schema: &str, table: &str) -> Result<Vec<String>> {
    let rows = client
        .query_typed(
            "SELECT a.attname::text
             FROM pg_index i
             JOIN pg_class c ON c.oid = i.indrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord)
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum
             WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2
             ORDER BY k.ord",
            &[(&schema, Type::TEXT), (&table, Type::TEXT)],
        )
        .await
        .map_err(sql_error)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}
