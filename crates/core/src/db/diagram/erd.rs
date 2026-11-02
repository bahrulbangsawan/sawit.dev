//! The tables and foreign keys the ERD canvas draws, and searching them.

use std::collections::HashMap;

use super::tables;
use crate::db::query::{Relation, SchemaInfo};

/// A column as the ERD canvas draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct ErdColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub pk: bool,
    pub fk: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ErdTable {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ErdColumn>,
}

impl ErdTable {
    /// `schema.table`, the key positions are saved under.
    pub fn key(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }
}

/// A foreign key between two shown tables, by index into [`Erd::tables`]
/// and their `columns`.
#[derive(Clone, Debug, PartialEq)]
pub struct ErdEdge {
    pub from: usize,
    pub from_columns: Vec<usize>,
    pub to: usize,
    pub to_columns: Vec<usize>,
    /// Some referencing column is nullable: "zero or one" on the target.
    pub optional: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Erd {
    pub tables: Vec<ErdTable>,
    pub edges: Vec<ErdEdge>,
}

/// The tables and foreign keys of `schemas` (or only `only`) for the canvas.
pub fn erd(schemas: &[SchemaInfo], only: Option<&str>) -> Erd {
    let shown: Vec<_> = tables(schemas, only).collect();
    let index: HashMap<(&str, &str), usize> = shown
        .iter()
        .enumerate()
        .map(|(ix, (s, r))| ((*s, r.name.as_str()), ix))
        .collect();
    let col_ix = |rel: &Relation, name: &String| rel.columns.iter().position(|c| &c.name == name);
    let mut erd = Erd::default();
    for (from, (schema, rel)) in shown.iter().enumerate() {
        erd.tables.push(ErdTable {
            schema: schema.to_string(),
            name: rel.name.clone(),
            columns: rel
                .columns
                .iter()
                .map(|c| ErdColumn {
                    name: c.name.clone(),
                    data_type: c.data_type.clone(),
                    nullable: c.nullable,
                    pk: rel.primary_key.contains(&c.name),
                    fk: rel.foreign_keys.iter().any(|f| f.columns.contains(&c.name)),
                })
                .collect(),
        });
        for fk in &rel.foreign_keys {
            let Some(&to) = index.get(&(fk.ref_schema.as_str(), fk.ref_table.as_str())) else {
                continue;
            };
            let target = shown[to].1;
            erd.edges.push(ErdEdge {
                from,
                from_columns: fk.columns.iter().filter_map(|c| col_ix(rel, c)).collect(),
                to,
                to_columns: fk
                    .ref_columns
                    .iter()
                    .filter_map(|c| col_ix(target, c))
                    .collect(),
                optional: fk
                    .columns
                    .iter()
                    .any(|c| rel.columns.iter().any(|col| &col.name == c && col.nullable)),
            });
        }
    }
    erd
}

/// Tables and columns matching `query` (case-insensitive substring): first
/// `(table, None)` for each table-name hit, then `(table, Some(col))` for
/// each column hit, each in table order.
pub fn search(erd: &Erd, query: &str) -> Vec<(usize, Option<usize>)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let mut columns = Vec::new();
    for (t, table) in erd.tables.iter().enumerate() {
        if table.name.to_lowercase().contains(&q) || table.key().to_lowercase().contains(&q) {
            hits.push((t, None));
        }
        for (c, col) in table.columns.iter().enumerate() {
            if col.name.to_lowercase().contains(&q) {
                columns.push((t, Some(c)));
            }
        }
    }
    hits.extend(columns);
    hits
}
