//! DBML, Mermaid, the ERD model, its search and its layout, on a small shop schema.

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
