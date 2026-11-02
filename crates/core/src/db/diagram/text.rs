//! The schema as text: DBML (dbdiagram.io, dbdocs) to export, and a Mermaid `erDiagram` for
//! Pele to draw.

use std::collections::HashMap;

use super::tables;
use crate::db::query::SchemaInfo;

/// A DBML name: bare when it is a plain identifier, else double-quoted.
fn dbml_name(name: &str) -> String {
    let plain = name
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if plain {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn dbml_columns(columns: &[String]) -> String {
    match columns {
        [one] => dbml_name(one),
        many => format!(
            "({})",
            many.iter()
                .map(|c| dbml_name(c))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The schema as DBML: one `Table` per table with its columns, primary key
/// and NOT NULLs, then a `Ref` per foreign key between shown tables.
pub fn to_dbml(schemas: &[SchemaInfo], only: Option<&str>) -> String {
    let shown: Vec<_> = tables(schemas, only).collect();
    let is_shown =
        |schema: &str, table: &str| shown.iter().any(|(s, r)| *s == schema && r.name == table);
    let mut out = String::new();
    let mut refs = Vec::new();
    for (schema, rel) in &shown {
        let table = format!("{}.{}", dbml_name(schema), dbml_name(&rel.name));
        out.push_str(&format!("Table {table} {{\n"));
        let single_pk = (rel.primary_key.len() == 1).then(|| rel.primary_key[0].as_str());
        for col in &rel.columns {
            let mut settings = Vec::new();
            if single_pk == Some(col.name.as_str()) {
                settings.push("pk");
            } else if !col.nullable {
                settings.push("not null");
            }
            let ty = if col.data_type.contains(' ') {
                format!("\"{}\"", col.data_type)
            } else {
                col.data_type.clone()
            };
            out.push_str(&format!("  {} {ty}", dbml_name(&col.name)));
            if !settings.is_empty() {
                out.push_str(&format!(" [{}]", settings.join(", ")));
            }
            out.push('\n');
        }
        if rel.primary_key.len() > 1 {
            out.push_str(&format!(
                "\n  Indexes {{\n    {} [pk]\n  }}\n",
                dbml_columns(&rel.primary_key)
            ));
        }
        out.push_str("}\n\n");
        for fk in &rel.foreign_keys {
            if is_shown(&fk.ref_schema, &fk.ref_table) {
                refs.push(format!(
                    "Ref: {table}.{} > {}.{}.{}",
                    dbml_columns(&fk.columns),
                    dbml_name(&fk.ref_schema),
                    dbml_name(&fk.ref_table),
                    dbml_columns(&fk.ref_columns)
                ));
            }
        }
    }
    for r in refs {
        out.push_str(&r);
        out.push('\n');
    }
    out
}

/// A token Pele's erDiagram grammar accepts for names and types: letters,
/// digits, `_` and `-`, plus `()[],` inside types.
fn mermaid_token(s: &str, keep: &[char]) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || keep.contains(&c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    match t.chars().next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => t,
        _ => format!("_{t}"),
    }
}

/// The tables and foreign keys as a Mermaid `erDiagram`. Entities are
/// labelled `schema.table`; a nullable foreign key is drawn "zero or one".
pub fn to_mermaid(schemas: &[SchemaInfo], only: Option<&str>) -> String {
    let shown: Vec<_> = tables(schemas, only).collect();
    let mut ids: HashMap<(&str, &str), String> = HashMap::new();
    for (ix, (schema, rel)) in shown.iter().enumerate() {
        let id = format!("{}_{ix}", mermaid_token(&rel.name, &[]));
        ids.insert((schema, rel.name.as_str()), id);
    }
    let mut out = String::from("erDiagram\n");
    let mut refs = Vec::new();
    for (schema, rel) in &shown {
        let id = &ids[&(*schema, rel.name.as_str())];
        let label = format!("{schema}.{}", rel.name).replace('"', "");
        out.push_str(&format!("  {id}[\"{label}\"] {{\n"));
        for col in &rel.columns {
            let pk = rel.primary_key.contains(&col.name);
            let fk = rel
                .foreign_keys
                .iter()
                .any(|f| f.columns.contains(&col.name));
            let keys = match (pk, fk) {
                (true, true) => " PK, FK",
                (true, false) => " PK",
                (false, true) => " FK",
                (false, false) => "",
            };
            out.push_str(&format!(
                "    {} {}{keys}\n",
                mermaid_token(&col.data_type, &['(', ')', '[', ']', ',']),
                mermaid_token(&col.name, &[])
            ));
        }
        out.push_str("  }\n");
        for fk in &rel.foreign_keys {
            let Some(target) = ids.get(&(fk.ref_schema.as_str(), fk.ref_table.as_str())) else {
                continue;
            };
            let optional = fk
                .columns
                .iter()
                .any(|c| rel.columns.iter().any(|col| &col.name == c && col.nullable));
            let to = if optional { "o|" } else { "||" };
            let label = fk.columns.join(", ").replace('"', "");
            refs.push(format!("  {id} }}o--{to} {target} : \"{label}\"\n"));
        }
    }
    for r in refs {
        out.push_str(&r);
    }
    out
}
