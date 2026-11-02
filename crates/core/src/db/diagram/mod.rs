//! Schema diagrams from a loaded schema tree: DBML text (dbdiagram.io,
//! dbdocs) to export, and the same tables and references as a Mermaid
//! `erDiagram` for Pele (`crate::mermaid`) to draw. Only tables are shown;
//! views have no keys to relate.

use std::collections::HashMap;

use super::query::{RelKind, Relation, SchemaInfo};

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

/// Number of tables a diagram of `schemas` (or only `only`) shows.
pub fn table_count(schemas: &[SchemaInfo], only: Option<&str>) -> usize {
    tables(schemas, only).count()
}

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

const RANK_GAP: f32 = 120.;
const NODE_GAP: f32 = 40.;
const GROUP_GAP: f32 = 120.;

/// Top-left positions for boxes of `sizes` (width, height) so that each
/// group of related tables reads left to right: a referenced table sits in
/// a column left of the tables pointing at it (`edges` are `(from, to)`
/// foreign keys). Groups, then lone tables, are packed into rows.
pub fn layout(sizes: &[(f32, f32)], edges: &[(usize, usize)]) -> Vec<(f32, f32)> {
    let n = sizes.len();
    let mut parents = vec![Vec::new(); n];
    let mut children = vec![Vec::new(); n];
    for &(from, to) in edges {
        if from != to && from < n && to < n && !parents[from].contains(&to) {
            parents[from].push(to);
            children[to].push(from);
        }
    }
    // Connected groups, in first-table order.
    let mut group = vec![usize::MAX; n];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for start in 0..n {
        if group[start] != usize::MAX {
            continue;
        }
        let g = groups.len();
        let mut members = vec![start];
        group[start] = g;
        let mut i = 0;
        while i < members.len() {
            let t = members[i];
            for &o in parents[t].iter().chain(&children[t]) {
                if group[o] == usize::MAX {
                    group[o] = g;
                    members.push(o);
                }
            }
            i += 1;
        }
        members.sort_unstable();
        groups.push(members);
    }
    // Rank = longest chain of references below a table; cycles are cut
    // where the depth-first walk meets a table it is still inside.
    let mut rank = vec![usize::MAX; n];
    fn rank_of(
        t: usize,
        parents: &[Vec<usize>],
        rank: &mut [usize],
        on_path: &mut [bool],
    ) -> usize {
        if rank[t] != usize::MAX {
            return rank[t];
        }
        on_path[t] = true;
        let mut r = 0;
        for &p in &parents[t] {
            if !on_path[p] {
                r = r.max(rank_of(p, parents, rank, on_path) + 1);
            }
        }
        on_path[t] = false;
        rank[t] = r;
        r
    }
    let mut on_path = vec![false; n];
    for t in 0..n {
        rank_of(t, &parents, &mut rank, &mut on_path);
    }

    // Lay out each group on its own, at (0, 0).
    let mut local = vec![(0f32, 0f32); n];
    let mut boxes: Vec<(usize, f32, f32)> = Vec::new(); // (group, width, height)
    for (g, members) in groups.iter().enumerate() {
        let ranks = members.iter().map(|&t| rank[t]).max().unwrap_or(0) + 1;
        let mut cols: Vec<Vec<usize>> = vec![Vec::new(); ranks];
        for &t in members {
            cols[rank[t]].push(t);
        }
        // Order each column by the average slot of its neighbours, a few
        // sweeps each way, so edges cross less.
        let mut slot = vec![0f32; n];
        let renumber = |cols: &Vec<Vec<usize>>, slot: &mut Vec<f32>| {
            for col in cols {
                for (i, &t) in col.iter().enumerate() {
                    slot[t] = i as f32;
                }
            }
        };
        renumber(&cols, &mut slot);
        for sweep in 0..4 {
            let order: Vec<usize> = if sweep % 2 == 0 {
                (1..ranks).collect()
            } else {
                (0..ranks.saturating_sub(1)).rev().collect()
            };
            for r in order {
                let mut keyed: Vec<(f32, usize)> = cols[r]
                    .iter()
                    .map(|&t| {
                        let near: Vec<f32> = parents[t]
                            .iter()
                            .chain(&children[t])
                            .filter(|&&o| rank[o] + 1 == r || rank[o] == r + 1)
                            .map(|&o| slot[o])
                            .collect();
                        let key = if near.is_empty() {
                            slot[t]
                        } else {
                            near.iter().sum::<f32>() / near.len() as f32
                        };
                        (key, t)
                    })
                    .collect();
                keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                cols[r] = keyed.into_iter().map(|(_, t)| t).collect();
                renumber(&cols, &mut slot);
            }
        }
        let heights: Vec<f32> = cols
            .iter()
            .map(|c| {
                c.iter().map(|&t| sizes[t].1).sum::<f32>()
                    + NODE_GAP * c.len().saturating_sub(1) as f32
            })
            .collect();
        let tallest = heights.iter().cloned().fold(0., f32::max);
        let mut x = 0.;
        for (r, col) in cols.iter().enumerate() {
            let width = col.iter().map(|&t| sizes[t].0).fold(0., f32::max);
            let mut y = (tallest - heights[r]) / 2.;
            for &t in col {
                local[t] = (x, y);
                y += sizes[t].1 + NODE_GAP;
            }
            x += width + RANK_GAP;
        }
        boxes.push((g, (x - RANK_GAP).max(0.), tallest));
    }

    // Pack groups into rows: bigger groups first, lone tables last.
    let area: f32 = boxes
        .iter()
        .map(|b| (b.1 + GROUP_GAP) * (b.2 + GROUP_GAP))
        .sum();
    let widest = boxes.iter().map(|b| b.1).fold(0., f32::max);
    let row_width = widest.max(area.sqrt() * 1.6);
    boxes.sort_by(|a, b| {
        let lone = |g: usize| groups[g].len() == 1;
        lone(a.0)
            .cmp(&lone(b.0))
            .then(b.2.total_cmp(&a.2))
            .then(a.0.cmp(&b.0))
    });
    let mut out = vec![(0f32, 0f32); n];
    let (mut x, mut y, mut row_h) = (0f32, 0f32, 0f32);
    for (g, w, h) in boxes {
        if x > 0. && x + w > row_width {
            x = 0.;
            y += row_h + GROUP_GAP;
            row_h = 0.;
        }
        for &t in &groups[g] {
            out[t] = (x + local[t].0, y + local[t].1);
        }
        x += w + GROUP_GAP;
        row_h = row_h.max(h);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ColumnInfo, ForeignKey};

    fn col(name: &str, ty: &str, nullable: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            data_type: ty.into(),
            nullable,
        }
    }

    fn shop() -> Vec<SchemaInfo> {
        vec![SchemaInfo {
            name: "public".into(),
            relations: vec![
                Relation {
                    name: "users".into(),
                    columns: vec![
                        col("id", "bigint", false),
                        col("email", "character varying(255)", false),
                    ],
                    readable: true,
                    primary_key: vec!["id".into()],
                    ..Relation::default()
                },
                Relation {
                    name: "orders".into(),
                    columns: vec![
                        col("id", "bigint", false),
                        col("user_id", "bigint", true),
                        col("total", "numeric(10,2)", false),
                    ],
                    readable: true,
                    primary_key: vec!["id".into()],
                    foreign_keys: vec![ForeignKey {
                        columns: vec!["user_id".into()],
                        ref_schema: "public".into(),
                        ref_table: "users".into(),
                        ref_columns: vec!["id".into()],
                    }],
                    ..Relation::default()
                },
                Relation {
                    name: "recent".into(),
                    kind: RelKind::View,
                    columns: vec![col("id", "bigint", true)],
                    ..Relation::default()
                },
            ],
        }]
    }

    #[test]
    fn dbml_has_tables_keys_and_refs() {
        let dbml = to_dbml(&shop(), None);
        assert!(dbml.contains("Table public.users {\n  id bigint [pk]\n  email \"character varying(255)\" [not null]\n}"), "{dbml}");
        assert!(dbml.contains("  user_id bigint\n"), "{dbml}");
        assert!(
            dbml.contains("Ref: public.orders.user_id > public.users.id"),
            "{dbml}"
        );
        assert!(!dbml.contains("recent"), "views are left out: {dbml}");
    }

    #[test]
    fn dbml_quotes_odd_names_and_composite_keys() {
        let schemas = vec![SchemaInfo {
            name: "My Schema".into(),
            relations: vec![Relation {
                name: "line".into(),
                columns: vec![col("a", "int", false), col("b", "int", false)],
                primary_key: vec!["a".into(), "b".into()],
                ..Relation::default()
            }],
        }];
        let dbml = to_dbml(&schemas, None);
        assert!(dbml.starts_with("Table \"My Schema\".line {"), "{dbml}");
        assert!(dbml.contains("Indexes {\n    (a, b) [pk]\n  }"), "{dbml}");
    }

    #[test]
    fn mermaid_uses_safe_tokens_and_cardinality() {
        let er = to_mermaid(&shop(), Some("public"));
        assert!(er.starts_with("erDiagram\n"));
        assert!(er.contains("users_0[\"public.users\"] {"), "{er}");
        assert!(er.contains("    character_varying(255) email\n"), "{er}");
        assert!(er.contains("    numeric(10,2) total\n"), "{er}");
        assert!(er.contains("    bigint user_id FK\n"), "{er}");
        // Nullable user_id: an order may have no user.
        assert!(er.contains("orders_1 }o--o| users_0 : \"user_id\""), "{er}");
        assert_eq!(table_count(&shop(), None), 2);
        assert_eq!(table_count(&shop(), Some("other")), 0);
    }

    #[test]
    fn erd_has_tables_columns_and_edges() {
        let erd = erd(&shop(), None);
        assert_eq!(erd.tables.len(), 2, "views are left out");
        assert_eq!(erd.tables[1].key(), "public.orders");
        let user_id = &erd.tables[1].columns[1];
        assert!(user_id.fk && !user_id.pk && user_id.nullable);
        assert!(erd.tables[0].columns[0].pk);
        assert_eq!(
            erd.edges,
            vec![ErdEdge {
                from: 1,
                from_columns: vec![1],
                to: 0,
                to_columns: vec![0],
                optional: true,
            }]
        );
    }

    #[test]
    fn search_finds_tables_and_columns() {
        let erd = erd(&shop(), None);
        assert_eq!(search(&erd, "USER"), vec![(0, None), (1, Some(1))]);
        assert_eq!(search(&erd, "public.ord"), vec![(1, None)]);
        // Table names come before columns, whatever the table order.
        assert_eq!(
            search(&erd, "id"),
            vec![(0, Some(0)), (1, Some(0)), (1, Some(1))]
        );
        assert_eq!(search(&erd, "r"), vec![(0, None), (1, None), (1, Some(1))]);
        assert!(search(&erd, "  ").is_empty());
    }

    #[test]
    fn layout_puts_referenced_tables_left_and_never_overlaps() {
        // 0 <- 1 <- 2, 0 <- 3, a cycle 4 <-> 5, and lone 6.
        let sizes = vec![
            (200., 100.),
            (220., 160.),
            (180., 80.),
            (200., 60.),
            (150., 90.),
            (150., 90.),
            (120., 40.),
        ];
        let edges = vec![(1, 0), (2, 1), (3, 0), (4, 5), (5, 4), (2, 2)];
        let pos = layout(&sizes, &edges);
        assert!(pos[0].0 < pos[1].0 && pos[1].0 < pos[2].0);
        assert!(pos[0].0 < pos[3].0);
        for a in 0..sizes.len() {
            for b in a + 1..sizes.len() {
                let apart = pos[a].0 + sizes[a].0 <= pos[b].0
                    || pos[b].0 + sizes[b].0 <= pos[a].0
                    || pos[a].1 + sizes[a].1 <= pos[b].1
                    || pos[b].1 + sizes[b].1 <= pos[a].1;
                assert!(apart, "{a} and {b} overlap: {pos:?}");
            }
        }
        assert_eq!(layout(&sizes, &edges), pos, "deterministic");
        assert!(layout(&[], &[]).is_empty());
    }
}
