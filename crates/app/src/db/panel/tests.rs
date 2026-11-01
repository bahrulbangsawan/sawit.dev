//! Tests of the connection tree's lines.

// Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
use super::tree::{D1_SECTION, Facet, Node, flatten};
use crate::db::lifecycle::ConnState;
use crate::db::store::DbStore;
use sawit_core::db::DbKind;
use sawit_core::db::FailureKind;
use sawit_core::db::{
    ColumnInfo, ConnProfile, Constraint, ConstraintKind, IndexInfo, Profiles, RelKind, Relation,
    SchemaInfo,
};
use std::collections::HashSet;
use std::time::{Duration, Instant};

fn rel(name: &str, readable: bool) -> Relation {
    Relation {
        name: name.into(),
        kind: RelKind::Table,
        columns: vec![ColumnInfo {
            name: "id".into(),
            data_type: "int".into(),
            nullable: false,
        }],
        readable,
        ..Relation::default()
    }
}

fn store() -> (DbStore, String) {
    let p = ConnProfile::new("h", 5432, "app", "u");
    let name = p.name.clone();
    let mut store = DbStore::detached(Profiles { items: vec![p] });
    store.schemas.insert(
        name.clone(),
        vec![
            SchemaInfo {
                name: "audit".into(),
                relations: vec![rel("log", true)],
            },
            SchemaInfo {
                name: "public".into(),
                relations: vec![rel("users", true), rel("secret", false)],
            },
        ],
    );
    (store, name)
}

fn names(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .map(|n| match n {
            Node::Profile { name, .. } => format!("p:{name}"),
            Node::Error { .. } => "error".into(),
            Node::Section { title, .. } => format!("section:{title}"),
            Node::Schema { name, .. } => format!("s:{name}"),
            Node::Kind { kind, .. } => format!("k:{}", super::tree::kind_label(*kind)),
            Node::Relation { name, .. } => format!("r:{name}"),
            Node::Column { name, .. } => format!("c:{name}"),
            Node::Group { facet, count, .. } => format!("g:{}:{count}", facet.label()),
            Node::Item { name, .. } => format!("i:{name}"),
        })
        .collect()
}

#[test]
fn collapsed_profile_shows_only_itself() {
    let (store, name) = store();
    assert_eq!(
        names(&flatten(&store, &HashSet::new(), "")),
        vec![format!("p:{name}")]
    );
}

#[test]
fn public_starts_open_other_schemas_closed() {
    let (store, name) = store();
    let expanded = HashSet::from([name.clone()]);
    assert_eq!(
        names(&flatten(&store, &expanded, "")),
        vec![
            format!("p:{name}"),
            "s:audit".into(),
            "s:public".into(),
            "k:Tables".into(),
            "r:users".into(),
            "r:secret".into(),
        ]
    );
}

#[test]
fn unreadable_relations_are_flagged_and_never_expand() {
    let (store, name) = store();
    let secret = format!("{name}\u{0}public\u{0}secret");
    let users = format!("{name}\u{0}public\u{0}users");
    let expanded = HashSet::from([name.clone(), secret, users]);
    let nodes = flatten(&store, &expanded, "");
    assert!(nodes.iter().any(|n| matches!(
        n,
        Node::Relation { name, readable: false, open: false, .. } if name == "secret"
    )));
    // Only `users` contributes a column line.
    assert_eq!(
        nodes
            .iter()
            .filter(|n| matches!(n, Node::Column { .. }))
            .count(),
        1
    );
}

#[test]
fn errors_get_their_own_line() {
    let (mut store, name) = store();
    store.set_state(
        &name,
        ConnState::Failed {
            error: "Check the user name and password.\nFATAL: no".into(),
            kind: FailureKind::Auth,
        },
    );
    assert_eq!(names(&flatten(&store, &HashSet::new(), ""))[1], "error");
    // Schema reload problems show the same way.
    store.set_state(&name, ConnState::Idle);
    store
        .notices
        .insert(name.clone(), "Unable to reload".into());
    assert_eq!(names(&flatten(&store, &HashSet::new(), ""))[1], "error");
}

#[test]
fn each_connection_state_shows_its_status() {
    let (mut store, name) = store();
    let now = Instant::now();
    let profile_line = |store: &DbStore| match &flatten(store, &HashSet::new(), "")[0] {
        Node::Profile {
            connected,
            pending,
            status,
            ..
        } => (*connected, *pending, status.clone()),
        other => panic!("{other:?}"),
    };
    // Opening the app: every connection idle, nothing connecting.
    assert_eq!(profile_line(&store), (false, false, Some("idle".into())));
    store.set_state(&name, ConnState::Connecting { attempt: 0 });
    assert_eq!(
        profile_line(&store),
        (false, true, Some("connecting…".into()))
    );
    store.set_state(&name, ConnState::Connecting { attempt: 3 });
    assert_eq!(profile_line(&store).2.as_deref(), Some("reconnecting 3/8…"));
    store.set_state(
        &name,
        ConnState::Retrying {
            attempt: 2,
            retry_at: now + Duration::from_secs(4),
            error: "Connection lost".into(),
        },
    );
    let nodes = flatten(&store, &HashSet::new(), "");
    assert!(matches!(&nodes[0], Node::Profile { pending: true, .. }));
    assert!(matches!(&nodes[1], Node::Error { message } if message == "Connection lost"));
    store.set_state(&name, ConnState::Inactive);
    assert_eq!(
        profile_line(&store),
        (false, false, Some("inactive".into()))
    );
    store.set_state(&name, ConnState::Disconnected);
    assert_eq!(
        profile_line(&store),
        (false, false, Some("disconnected".into()))
    );
    // "Connected" with no live client (it just dropped) isn't shown as connected.
    store.set_state(&name, ConnState::Connected);
    assert_eq!(profile_line(&store), (false, false, None));
}

#[test]
fn filter_keeps_matching_relations_and_opens_their_schemas() {
    let (store, name) = store();
    let expanded = HashSet::from([name.clone()]);
    // `audit` starts closed but opens for a match; `public` has none.
    assert_eq!(
        names(&flatten(&store, &expanded, " LO ")),
        vec![
            format!("p:{name}"),
            "s:audit".into(),
            "k:Tables".into(),
            "r:log".into()
        ]
    );
    assert_eq!(
        names(&flatten(&store, &expanded, "zzz")),
        vec![format!("p:{name}")]
    );
}

#[test]
fn tables_list_indexes_and_constraints_after_columns() {
    let (mut store, name) = store();
    let users = &mut store.schemas.get_mut(&name).unwrap()[1].relations[0];
    users.primary_key = vec!["id".into()];
    users.indexes = vec![IndexInfo {
        name: "users_pkey".into(),
        method: "btree".into(),
        unique: true,
        primary: true,
        definition: "CREATE UNIQUE INDEX users_pkey ON public.users USING btree (id)".into(),
    }];
    users.constraints = vec![Constraint {
        name: "users_pkey".into(),
        kind: ConstraintKind::PrimaryKey,
        definition: "PRIMARY KEY (id)".into(),
    }];
    let rkey = format!("{name}\u{0}public\u{0}users");
    let mut expanded = HashSet::from([name.clone(), rkey.clone()]);
    let lines = names(&flatten(&store, &expanded, ""));
    assert_eq!(
        lines[4..8],
        ["r:users", "c:id", "g:Indexes:1", "g:Constraints:1"].map(String::from)
    );
    expanded.insert(format!("{rkey}\u{0}Indexes"));
    let nodes = flatten(&store, &expanded, "");
    assert!(nodes.iter().any(|n| matches!(
        n,
        Node::Item { facet: Facet::Indexes, detail, .. } if detail == "primary btree"
    )));
    assert!(
        nodes
            .iter()
            .any(|n| matches!(n, Node::Column { primary: true, .. }))
    );
}

#[test]
fn views_have_no_index_or_constraint_groups() {
    let (mut store, name) = store();
    store.schemas.get_mut(&name).unwrap()[1].relations[0].kind = RelKind::View;
    let rkey = format!("{name}\u{0}public\u{0}users");
    let expanded = HashSet::from([name.clone(), rkey]);
    assert!(
        !flatten(&store, &expanded, "")
            .iter()
            .any(|n| matches!(n, Node::Group { .. }))
    );
}

#[test]
fn relations_are_listed_by_kind_in_collapsible_sections() {
    let (mut store, name) = store();
    let public = &mut store.schemas.get_mut(&name).unwrap()[1];
    public.relations[1].kind = RelKind::View;
    public.relations.push(Relation {
        kind: RelKind::MaterializedView,
        ..rel("totals", true)
    });
    public.relations.push(rel("accounts", true));
    let mut expanded = HashSet::from([name.clone()]);
    let lines = names(&flatten(&store, &expanded, ""));
    assert_eq!(
        lines[3..],
        [
            "k:Tables",
            "r:users",
            "r:accounts",
            "k:Views",
            "r:secret",
            "k:Materialized Views",
            "r:totals"
        ]
        .map(String::from)
    );
    // Collapsing a section hides its relations only.
    expanded.insert(format!("{name}\u{0}public\u{0}\u{1}Tables"));
    let lines = names(&flatten(&store, &expanded, ""));
    assert_eq!(
        lines[3..6],
        ["k:Tables", "k:Views", "r:secret"].map(String::from)
    );
    // A filter opens every section with a match.
    let lines = names(&flatten(&store, &expanded, "acc"));
    assert_eq!(
        lines[1..],
        ["s:public", "k:Tables", "r:accounts"].map(String::from)
    );
}

/// A Workers project's D1 databases: `DB`'s local file (with a `main` schema loaded), the
/// same database on Cloudflare, and a local file no binding names.
fn with_d1(store: &mut DbStore) -> (String, String, String) {
    use sawit_core::db::d1::{Detected, LocalD1};
    let local = ConnProfile::d1_local(&LocalD1 {
        project: "/work/api".into(),
        worker: Some("api".into()),
        binding: "DB".into(),
        database_name: Some("shop".into()),
        miniflare_id: "5d0c".into(),
        file: "/work/api/.wrangler/state/v3/d1/x/abc.sqlite".into(),
        exists: true,
    });
    let remote = ConnProfile::d1_remote(std::path::Path::new("/work/api"), "DB", "shop");
    let stray = ConnProfile::sqlite_file(std::path::Path::new("/work/api/stray.sqlite"));
    let names = (local.name.clone(), remote.name.clone(), stray.name.clone());
    store.schemas.insert(
        local.name.clone(),
        vec![SchemaInfo {
            name: "main".into(),
            relations: vec![rel("products", true)],
        }],
    );
    store.detected = Detected {
        local: vec![local],
        remote: vec![remote],
        unmatched: vec![stray],
    };
    names
}

#[test]
fn d1_databases_follow_saved_connections_local_first() {
    let (mut store, name) = store();
    let (local, remote, stray) = with_d1(&mut store);
    let mut expanded = HashSet::new();
    assert_eq!(
        names(&flatten(&store, &expanded, "")),
        vec![
            format!("p:{name}"),
            "section:Cloudflare D1".into(),
            format!("p:{local}"),
            format!("p:{remote}"),
            "section:Unmatched local D1 databases".into(),
            format!("p:{stray}"),
        ]
    );
    // SQLite's `main` starts open, as `public` does.
    expanded.insert(local.clone());
    let lines = names(&flatten(&store, &expanded, ""));
    assert_eq!(
        lines[2..6],
        [
            format!("p:{local}"),
            "s:main".into(),
            "k:Tables".into(),
            "r:products".into()
        ]
    );
    // Collapsing the section hides its databases.
    expanded.insert(D1_SECTION.to_string());
    let lines = names(&flatten(&store, &expanded, ""));
    assert_eq!(
        lines[1..3],
        [
            "section:Cloudflare D1".to_string(),
            "section:Unmatched local D1 databases".into()
        ]
    );
}

#[test]
fn rows_name_their_kind_beside_the_logo() {
    let (mut store, name) = store();
    let (local, remote, _) = with_d1(&mut store);
    store.versions.insert(name.clone(), 16);
    let nodes = flatten(&store, &HashSet::new(), "");
    let row = |key: &str| {
        nodes
            .iter()
            .find_map(|n| match n {
                Node::Profile {
                    name,
                    label,
                    detail,
                    kind,
                    accessible,
                    saved,
                    ..
                } if name == key => Some((
                    label.clone(),
                    detail.clone(),
                    *kind,
                    accessible.clone(),
                    *saved,
                )),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        row(&name),
        (
            name.clone(),
            None,
            DbKind::Postgres,
            format!("PostgreSQL 16 · {name}"),
            true
        )
    );
    assert_eq!(
        row(&local),
        (
            "DB".into(),
            Some("shop".into()),
            DbKind::D1Local,
            "Cloudflare D1 (local) · DB".into(),
            false
        )
    );
    assert_eq!(row(&remote).3, "Cloudflare D1 (remote) · DB");
    assert_eq!(
        super::super::logos::logo_path(row(&local).2).0,
        "db_logos/d1.svg"
    );
}
