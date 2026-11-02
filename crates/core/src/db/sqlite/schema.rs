//! The schema tree of a SQLite database: its tables and views with their columns, keys,
//! foreign keys and indexes, as one schema.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use super::{MAIN, lite_error};
use crate::Result;
use crate::db::query::{
    ColumnInfo, Constraint, ConstraintKind, ForeignKey, IndexInfo, RelKind, Relation, SchemaInfo,
};

/// Tables Sawit hides: SQLite's own, and D1's (`_cf_KV`, `_cf_METADATA`), which D1 refuses
/// to touch anyway.
fn is_internal(name: &str) -> bool {
    name.starts_with("sqlite_") || name.starts_with("_cf_")
}

/// The tables and views of the database, as one schema, `main`, with their columns, keys,
/// foreign keys and indexes. Row counts are exact (see [`row_count`](super::row_count)), so no estimate.
pub fn load_schema(conn: &Connection) -> Result<Vec<SchemaInfo>> {
    // The indexes' CREATE statements are read once for all the tables, so the schema must not
    // change while the tables are read: another process may change it (a local D1 file is
    // wrangler's). The schema version says when it did, and the tables are read again.
    let version = |conn: &Connection| -> Result<i64> {
        conn.query_row("PRAGMA schema_version", [], |r| r.get(0))
            .map_err(lite_error)
    };
    let mut tries = 0;
    let mut relations = loop {
        let before = version(conn)?;
        let relations = read_relations(conn)?;
        tries += 1;
        if version(conn)? == before || tries == 3 {
            break relations;
        }
    };
    // A foreign key to the parent's primary key names no columns: fill them in.
    let keys: HashMap<String, Vec<String>> = relations
        .iter()
        .map(|r| (r.name.clone(), r.primary_key.clone()))
        .collect();
    for rel in &mut relations {
        for fk in &mut rel.foreign_keys {
            if fk.ref_columns.iter().all(String::is_empty) {
                fk.ref_columns = keys.get(&fk.ref_table).cloned().unwrap_or_default();
            }
        }
    }
    Ok(vec![SchemaInfo {
        name: MAIN.into(),
        relations,
    }])
}

/// The tables and views, read with every index's CREATE statement looked up in one query.
fn read_relations(conn: &Connection) -> Result<Vec<Relation>> {
    let mut stmt = conn
        .prepare(
            "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name",
        )
        .map_err(lite_error)?;
    let names: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(lite_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(lite_error)?;
    // Each index's CREATE statement by name (none for those SQLite makes), read once.
    let index_sql: HashMap<String, Option<String>> = conn
        .prepare("SELECT name, sql FROM sqlite_master WHERE type = 'index'")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .map_err(lite_error)?;
    let mut relations = Vec::new();
    for (name, kind) in names.into_iter().filter(|(n, _)| !is_internal(n)) {
        relations.push(relation(conn, &name, kind == "view", &index_sql)?);
    }
    Ok(relations)
}

pub(super) fn relation(
    conn: &Connection,
    name: &str,
    view: bool,
    index_sql: &HashMap<String, Option<String>>,
) -> Result<Relation> {
    let mut rel = Relation {
        name: name.into(),
        kind: if view { RelKind::View } else { RelKind::Table },
        readable: true,
        ..Relation::default()
    };
    let mut pk: Vec<(i64, String)> = Vec::new();
    let mut stmt = conn
        .prepare_cached(r#"SELECT name, type, "notnull", pk, hidden FROM pragma_table_xinfo(?1)"#)
        .map_err(lite_error)?;
    let mut rows = stmt.query([name]).map_err(lite_error)?;
    while let Some(row) = rows.next().map_err(lite_error)? {
        let hidden: i64 = row.get(4).map_err(lite_error)?;
        if hidden == 1 {
            continue; // a virtual table's hidden column
        }
        let column: String = row.get(0).map_err(lite_error)?;
        let key: i64 = row.get(3).map_err(lite_error)?;
        if key > 0 {
            pk.push((key, column.clone()));
        }
        rel.columns.push(ColumnInfo {
            name: column,
            data_type: row.get::<_, String>(1).map_err(lite_error)?,
            nullable: row.get::<_, i64>(2).map_err(lite_error)? == 0,
        });
    }
    if view {
        return Ok(rel);
    }
    pk.sort();
    rel.primary_key = pk.into_iter().map(|(_, c)| c).collect();
    if !rel.primary_key.is_empty() {
        rel.constraints.push(Constraint {
            name: "primary key".into(),
            kind: ConstraintKind::PrimaryKey,
            definition: format!("PRIMARY KEY ({})", rel.primary_key.join(", ")),
        });
    }
    let mut stmt = conn
        .prepare_cached(
            r#"SELECT id, "table", "from", "to" FROM pragma_foreign_key_list(?1) ORDER BY id, seq"#,
        )
        .map_err(lite_error)?;
    let mut rows = stmt.query([name]).map_err(lite_error)?;
    let mut current: Option<i64> = None;
    while let Some(row) = rows.next().map_err(lite_error)? {
        let id: i64 = row.get(0).map_err(lite_error)?;
        let from: String = row.get(2).map_err(lite_error)?;
        let to: Option<String> = row.get(3).map_err(lite_error)?;
        if current != Some(id) {
            current = Some(id);
            rel.foreign_keys.push(ForeignKey {
                columns: vec![],
                ref_schema: MAIN.into(),
                ref_table: row.get(1).map_err(lite_error)?,
                ref_columns: vec![],
            });
        }
        let fk = rel.foreign_keys.last_mut().expect("pushed above");
        fk.columns.push(from);
        fk.ref_columns.push(to.unwrap_or_default());
    }
    for fk in &rel.foreign_keys {
        let to = if fk.ref_columns.iter().all(String::is_empty) {
            String::new()
        } else {
            format!("({})", fk.ref_columns.join(", "))
        };
        rel.constraints.push(Constraint {
            name: format!("{} → {}", fk.columns.join(", "), fk.ref_table),
            kind: ConstraintKind::ForeignKey,
            definition: format!(
                "FOREIGN KEY ({}) REFERENCES {}{to}",
                fk.columns.join(", "),
                fk.ref_table
            ),
        });
    }
    let mut stmt = conn
        .prepare_cached(r#"SELECT name, "unique", origin FROM pragma_index_list(?1) ORDER BY name"#)
        .map_err(lite_error)?;
    let indexes: Vec<(String, bool, String, Option<String>)> = stmt
        .query_map([name], |r| {
            let index: String = r.get(0)?;
            // One made after the map was read is looked up on its own.
            let sql = match index_sql.get(&index) {
                Some(sql) => sql.clone(),
                None => conn
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                        [&index],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten(),
            };
            Ok((index, r.get::<_, i64>(1)? != 0, r.get(2)?, sql))
        })
        .map_err(lite_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(lite_error)?;
    for (index, unique, origin, sql) in indexes {
        let columns: Vec<String> = conn
            .prepare_cached("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
            .and_then(|mut s| {
                s.query_map([&index], |r| r.get::<_, Option<String>>(0))?
                    .map(|c| c.map(Option::unwrap_or_default))
                    .collect()
            })
            .map_err(lite_error)?;
        let cols = columns.join(", ");
        if origin == "u" {
            rel.constraints.push(Constraint {
                name: index.clone(),
                kind: ConstraintKind::Unique,
                definition: format!("UNIQUE ({cols})"),
            });
        }
        rel.indexes.push(IndexInfo {
            definition: sql.unwrap_or_else(|| {
                format!(
                    "{} ({cols}), made by SQLite for {}",
                    if unique { "UNIQUE INDEX" } else { "INDEX" },
                    if origin == "pk" {
                        "the primary key"
                    } else {
                        "a UNIQUE constraint"
                    }
                )
            }),
            name: index,
            method: "btree".into(),
            unique,
            primary: origin == "pk",
        });
    }
    Ok(rel)
}

/// Primary-key columns of `table`, in key order (empty for views and keyless tables).
pub fn primary_key(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk")
        .map_err(lite_error)?;
    stmt.query_map([table], |r| r.get(0))
        .map_err(lite_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(lite_error)
}

pub(super) fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_xinfo(?1) WHERE hidden <> 1")
        .map_err(lite_error)?;
    stmt.query_map([table], |r| r.get(0))
        .map_err(lite_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(lite_error)
}
