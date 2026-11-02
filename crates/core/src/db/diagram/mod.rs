//! Schema diagrams from a loaded schema tree: DBML text (dbdiagram.io,
//! dbdocs) to export, and the same tables and references as a Mermaid
//! `erDiagram` for Pele (`crate::mermaid`) to draw. Only tables are shown;
//! views have no keys to relate.

mod erd;
mod layout;
mod text;

use super::query::{RelKind, Relation, SchemaInfo};

pub use erd::{Erd, ErdColumn, ErdEdge, ErdTable, erd, search};
pub use layout::layout;
pub use text::{to_dbml, to_mermaid};

/// Tables of `schemas` (all of them, or only the one named `only`).
fn tables<'a>(
    schemas: &'a [SchemaInfo],
    only: Option<&'a str>,
) -> impl Iterator<Item = (&'a str, &'a Relation)> {
    schemas
        .iter()
        .filter(move |s| only.is_none_or(|o| o == s.name))
        .flat_map(|s| {
            s.relations
                .iter()
                .filter(|r| matches!(r.kind, RelKind::Table | RelKind::Foreign))
                .map(move |r| (s.name.as_str(), r))
        })
}

/// Number of tables a diagram of `schemas` (or only `only`) shows.
pub fn table_count(schemas: &[SchemaInfo], only: Option<&str>) -> usize {
    tables(schemas, only).count()
}

#[cfg(test)]
mod tests;
