//! The schema tree from `information_schema`: databases (the tree's schemas), their tables and
//! views with columns, keys, constraints, indexes and the server's row estimates.

use std::collections::BTreeMap;

use super::{My, literal, quote, relation};
use crate::Result;
use crate::db::query::{
    ColumnInfo, Constraint, ConstraintKind, ForeignKey, IndexInfo, RelKind, Relation, SchemaInfo,
};

/// The server's own databases, left out of the tree.
pub const SYSTEM_DATABASES: [&str; 4] =
    ["mysql", "information_schema", "performance_schema", "sys"];

type Rows = Vec<Vec<Option<String>>>;

/// Schema, table and constraint or index name.
type Key = (String, String, String);

/// A constraint's type, its columns, and (a foreign key's) the table and columns it references.
type KeyColumns = (String, Vec<String>, Option<(String, String)>, Vec<String>);

fn cell(row: &[Option<String>], i: usize) -> String {
    row.get(i).cloned().flatten().unwrap_or_default()
}

impl My {
    /// The databases the tree shows: the profile's, else every one but the server's own.
    async fn schema_names(&self) -> Result<Vec<String>> {
        if !self.profile.database.is_empty() {
            return Ok(vec![self.profile.database.clone()]);
        }
        let system = SYSTEM_DATABASES
            .iter()
            .map(|d| literal(d))
            .collect::<Vec<_>>()
            .join(", ");
        let rows = self
            .rows(&format!(
                "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA \
                 WHERE SCHEMA_NAME NOT IN ({system}) ORDER BY SCHEMA_NAME"
            ))
            .await?;
        Ok(rows.iter().map(|r| cell(r, 0)).collect())
    }

    pub async fn load_schema(&self) -> Result<Vec<SchemaInfo>> {
        let names = self.schema_names().await?;
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let list = names
            .iter()
            .map(|n| literal(n))
            .collect::<Vec<_>>()
            .join(", ");
        let tables = self
            .rows(&format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, TABLE_ROWS \
                 FROM information_schema.TABLES WHERE TABLE_SCHEMA IN ({list})"
            ))
            .await?;
        let columns = self
            .rows(&format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE \
                 FROM information_schema.COLUMNS WHERE TABLE_SCHEMA IN ({list}) \
                 ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION"
            ))
            .await?;
        let keys = self
            .rows(&format!(
                "SELECT k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, c.CONSTRAINT_TYPE, \
                        k.COLUMN_NAME, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, \
                        k.REFERENCED_COLUMN_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE k \
                 JOIN information_schema.TABLE_CONSTRAINTS c \
                   ON c.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA \
                  AND c.TABLE_NAME = k.TABLE_NAME \
                  AND c.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
                 WHERE k.TABLE_SCHEMA IN ({list}) \
                 ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION"
            ))
            .await?;
        // MySQL 8.0.16+ and MariaDB 10.2+; older servers have no CHECK constraints to list.
        let checks = self
            .rows(&format!(
                "SELECT t.TABLE_SCHEMA, t.TABLE_NAME, t.CONSTRAINT_NAME, k.CHECK_CLAUSE \
                 FROM information_schema.TABLE_CONSTRAINTS t \
                 JOIN information_schema.CHECK_CONSTRAINTS k \
                   ON k.CONSTRAINT_SCHEMA = t.CONSTRAINT_SCHEMA \
                  AND k.CONSTRAINT_NAME = t.CONSTRAINT_NAME \
                 WHERE t.CONSTRAINT_TYPE = 'CHECK' AND t.TABLE_SCHEMA IN ({list})"
            ))
            .await
            .unwrap_or_default();
        let indexes = self
            .rows(&format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, NON_UNIQUE, INDEX_TYPE, \
                        COLUMN_NAME, SUB_PART \
                 FROM information_schema.STATISTICS WHERE TABLE_SCHEMA IN ({list}) \
                 ORDER BY TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX"
            ))
            .await?;
        Ok(assemble(&names, tables, columns, keys, checks, indexes))
    }

    /// The primary key's columns, in key order.
    pub async fn primary_key(&self, schema: &str, table: &str) -> Result<Vec<String>> {
        let rows = self
            .rows(&format!(
                "SELECT COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE \
                 WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} AND CONSTRAINT_NAME = 'PRIMARY' \
                 ORDER BY ORDINAL_POSITION",
                literal(schema),
                literal(table)
            ))
            .await?;
        Ok(rows.iter().map(|r| cell(r, 0)).collect())
    }
}

/// The tree from `information_schema`'s rows (see [`My::load_schema`] for their columns).
pub(super) fn assemble(
    names: &[String],
    tables: Rows,
    columns: Rows,
    keys: Rows,
    checks: Rows,
    indexes: Rows,
) -> Vec<SchemaInfo> {
    let mut rels: BTreeMap<(String, String), Relation> = BTreeMap::new();
    for t in &tables {
        let (schema, name) = (cell(t, 0), cell(t, 1));
        let kind = match cell(t, 2).as_str() {
            "VIEW" | "SYSTEM VIEW" => RelKind::View,
            _ => RelKind::Table,
        };
        rels.insert(
            (schema, name.clone()),
            Relation {
                name,
                kind,
                readable: true,
                estimated_rows: (kind == RelKind::Table)
                    .then(|| t.get(3).cloned().flatten()?.parse().ok())
                    .flatten(),
                ..Relation::default()
            },
        );
    }
    for c in &columns {
        if let Some(rel) = rels.get_mut(&(cell(c, 0), cell(c, 1))) {
            rel.columns.push(ColumnInfo {
                name: cell(c, 2),
                data_type: cell(c, 3),
                nullable: cell(c, 4) == "YES",
            });
        }
    }
    // Key columns, grouped by constraint (rows come in key order).
    let mut constraints: BTreeMap<Key, KeyColumns> = BTreeMap::new();
    for k in &keys {
        let entry = constraints
            .entry((cell(k, 0), cell(k, 1), cell(k, 2)))
            .or_insert_with(|| (cell(k, 3), Vec::new(), None, Vec::new()));
        entry.1.push(cell(k, 4));
        if let (Some(rs), Some(rt)) = (k.get(5).cloned().flatten(), k.get(6).cloned().flatten()) {
            entry.2 = Some((rs, rt));
            entry.3.push(cell(k, 7));
        }
    }
    for ((schema, table, name), (kind, cols, refs, ref_cols)) in constraints {
        let Some(rel) = rels.get_mut(&(schema.clone(), table)) else {
            continue;
        };
        let list = |cols: &[String]| cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
        let (kind, definition) = match (kind.as_str(), &refs) {
            ("PRIMARY KEY", _) => {
                rel.primary_key = cols.clone();
                (
                    ConstraintKind::PrimaryKey,
                    format!("PRIMARY KEY ({})", list(&cols)),
                )
            }
            ("FOREIGN KEY", Some((ref_schema, ref_table))) => {
                rel.foreign_keys.push(ForeignKey {
                    columns: cols.clone(),
                    ref_schema: ref_schema.clone(),
                    ref_table: ref_table.clone(),
                    ref_columns: ref_cols.clone(),
                });
                let target = if *ref_schema == schema {
                    quote(ref_table)
                } else {
                    relation(ref_schema, ref_table)
                };
                (
                    ConstraintKind::ForeignKey,
                    format!(
                        "FOREIGN KEY ({}) REFERENCES {target} ({})",
                        list(&cols),
                        list(&ref_cols)
                    ),
                )
            }
            ("UNIQUE", _) => (ConstraintKind::Unique, format!("UNIQUE ({})", list(&cols))),
            _ => (ConstraintKind::Other, list(&cols)),
        };
        rel.constraints.push(Constraint {
            name,
            kind,
            definition,
        });
    }
    for c in &checks {
        if let Some(rel) = rels.get_mut(&(cell(c, 0), cell(c, 1))) {
            rel.constraints.push(Constraint {
                name: cell(c, 2),
                kind: ConstraintKind::Check,
                definition: format!("CHECK ({})", cell(c, 3)),
            });
        }
    }
    // Index columns, grouped by index (rows come in column order).
    // Unique, method, columns.
    let mut index_cols: BTreeMap<Key, (bool, String, Vec<String>)> = BTreeMap::new();
    for i in &indexes {
        let entry = index_cols
            .entry((cell(i, 0), cell(i, 1), cell(i, 2)))
            .or_insert_with(|| (cell(i, 3) == "0", cell(i, 4), Vec::new()));
        let col = match i.get(5).cloned().flatten() {
            Some(name) => match i.get(6).cloned().flatten() {
                Some(len) => format!("{}({len})", quote(&name)),
                None => quote(&name),
            },
            // A functional index's part (MySQL 8): its expression isn't in this view.
            None => "(expression)".into(),
        };
        entry.2.push(col);
    }
    for ((schema, table, name), (unique, method, cols)) in index_cols {
        let Some(rel) = rels.get_mut(&(schema.clone(), table.clone())) else {
            continue;
        };
        let primary = name == "PRIMARY";
        let definition = if primary {
            format!("PRIMARY KEY ({}) USING {method}", cols.join(", "))
        } else {
            format!(
                "CREATE {}INDEX {} ON {} ({}) USING {method}",
                if unique { "UNIQUE " } else { "" },
                quote(&name),
                quote(&table),
                cols.join(", ")
            )
        };
        rel.indexes.push(IndexInfo {
            name,
            method: method.to_lowercase(),
            unique,
            primary,
            definition,
        });
    }
    names
        .iter()
        .map(|schema| SchemaInfo {
            name: schema.clone(),
            relations: rels
                .iter()
                .filter(|((s, _), _)| s == schema)
                .map(|(_, r)| r.clone())
                .collect(),
        })
        .collect()
}
