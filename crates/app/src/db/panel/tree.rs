//! The connection tree as the lines the virtual list shows: connections, the D1 sections,
//! schemas, relation kinds and relations, and a relation's columns, indexes and constraints.

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::assets::IconName as Lucide;
use sawit_core::db::{ConnProfile, DbKind, RelKind, Relation, WriteRoute};

use crate::db::store::DbStore;

/// The sections a schema's relations are listed in, in this order.
const KIND_ORDER: [RelKind; 4] = [
    RelKind::Table,
    RelKind::View,
    RelKind::MaterializedView,
    RelKind::Foreign,
];

pub(super) fn kind_label(kind: RelKind) -> &'static str {
    match kind {
        RelKind::Table => "Tables",
        RelKind::View => "Views",
        RelKind::MaterializedView => "Materialized Views",
        RelKind::Foreign => "Foreign Tables",
    }
}

/// "table", "view", … for menus and prompts.
pub(super) fn kind_noun(kind: RelKind) -> &'static str {
    match kind {
        RelKind::Table => "table",
        RelKind::View => "view",
        RelKind::MaterializedView => "materialized view",
        RelKind::Foreign => "foreign table",
    }
}

pub(super) fn kind_icon(kind: RelKind) -> Lucide {
    match kind {
        RelKind::Table | RelKind::Foreign => Lucide::Table,
        RelKind::View => Lucide::Eye,
        RelKind::MaterializedView => Lucide::Layers,
    }
}

/// The collapsible groups under a table, after its columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    Indexes,
    Constraints,
}

impl Facet {
    pub(super) fn label(self) -> &'static str {
        match self {
            Facet::Indexes => "Indexes",
            Facet::Constraints => "Constraints",
        }
    }
}

/// One visible line of the connection tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Profile {
        /// The profile's key.
        name: String,
        /// What to show: the custom label, else `name` (a D1 binding's name).
        label: String,
        /// Beside the label, quieter: a D1 binding's database name.
        detail: Option<String>,
        /// Where it points, for the tooltip: the connection string, or the file.
        url: String,
        kind: DbKind,
        /// "PostgreSQL 16 · kyzn": the tooltip's first line and the row's accessible name.
        accessible: String,
        /// Saved (has settings), rather than found in a Workers project.
        saved: bool,
        /// Where its changes go, for SQLite and D1.
        route: Option<String>,
        connected: bool,
        /// Connecting now or waiting to retry.
        pending: bool,
        /// Status when not connected ("idle", "retrying in 4s").
        status: Option<String>,
    },
    Error {
        message: String,
    },
    /// A heading over found databases: "Cloudflare D1", "Unmatched local D1 databases".
    Section {
        key: String,
        title: String,
        count: usize,
        open: bool,
        /// Shows the Cloudflare logo.
        cloudflare: bool,
    },
    Schema {
        key: String,
        profile: String,
        name: String,
        open: bool,
        count: usize,
        /// The database's kind, for what its menu offers.
        db: DbKind,
    },
    /// "Tables", "Views", … under a schema; open unless collapsed.
    Kind {
        key: String,
        kind: RelKind,
        count: usize,
        open: bool,
    },
    Relation {
        key: String,
        profile: String,
        schema: String,
        name: String,
        kind: RelKind,
        open: bool,
        readable: bool,
        db: DbKind,
    },
    Column {
        key: String,
        name: String,
        detail: String,
        /// Part of the primary key.
        primary: bool,
    },
    Group {
        key: String,
        facet: Facet,
        count: usize,
        open: bool,
    },
    /// An index or constraint; `definition` is its full SQL.
    Item {
        key: String,
        facet: Facet,
        name: String,
        detail: String,
        definition: String,
    },
}

/// The lines under an open relation: columns, then its indexes and
/// constraints as groups (tables and materialized views only).
fn relation_children(rkey: &str, rel: &Relation, expanded: &HashSet<String>, out: &mut Vec<Node>) {
    for (cix, col) in rel.columns.iter().enumerate() {
        out.push(Node::Column {
            key: format!("{rkey}\u{0}{cix}"),
            name: col.name.clone(),
            detail: format!(
                "{}{}",
                col.data_type,
                if col.nullable { "" } else { " not null" }
            ),
            primary: rel.primary_key.contains(&col.name),
        });
    }
    let facets: &[Facet] = match rel.kind {
        RelKind::Table => &[Facet::Indexes, Facet::Constraints],
        RelKind::MaterializedView => &[Facet::Indexes],
        RelKind::View | RelKind::Foreign => &[],
    };
    for &facet in facets {
        let gkey = format!("{rkey}\u{0}{}", facet.label());
        let count = match facet {
            Facet::Indexes => rel.indexes.len(),
            Facet::Constraints => rel.constraints.len(),
        };
        let open = count > 0 && expanded.contains(&gkey);
        out.push(Node::Group {
            key: gkey.clone(),
            facet,
            count,
            open,
        });
        if !open {
            continue;
        }
        match facet {
            Facet::Indexes => out.extend(rel.indexes.iter().map(|ix| Node::Item {
                key: format!("{gkey}\u{0}{}", ix.name),
                facet,
                name: ix.name.clone(),
                detail: match (ix.primary, ix.unique) {
                    (true, _) => format!("primary {}", ix.method),
                    (false, true) => format!("unique {}", ix.method),
                    _ => ix.method.clone(),
                },
                definition: ix.definition.clone(),
            })),
            Facet::Constraints => out.extend(rel.constraints.iter().map(|c| Node::Item {
                key: format!("{gkey}\u{0}{}", c.name),
                facet,
                name: c.name.clone(),
                detail: c.kind.label().into(),
                definition: c.definition.clone(),
            })),
        }
    }
}

/// Flattens the tree into the lines currently visible. `public` schemas
/// start open; toggling a key flips its default. A non-empty `query` keeps
/// only relations whose name contains it (any case), with their schemas
/// open.
pub fn flatten(store: &DbStore, expanded: &HashSet<String>, query: &str) -> Vec<Node> {
    let query = query.trim().to_lowercase();
    let mut out = Vec::new();
    for profile in &store.profiles.items {
        push_profile(store, profile, true, expanded, &query, &mut out);
    }
    let d1: Vec<&ConnProfile> = store
        .detected
        .local
        .iter()
        .chain(&store.detected.remote)
        .collect();
    let sections = [
        (D1_SECTION, "Cloudflare D1", d1, true),
        (
            UNMATCHED_SECTION,
            "Unmatched local D1 databases",
            store.detected.unmatched.iter().collect(),
            false,
        ),
    ];
    for (key, title, profiles, cloudflare) in sections {
        if profiles.is_empty() {
            continue;
        }
        // Open unless collapsed.
        let open = !expanded.contains(key);
        out.push(Node::Section {
            key: key.into(),
            title: title.into(),
            count: profiles.len(),
            open,
            cloudflare,
        });
        if open {
            for profile in profiles {
                push_profile(store, profile, false, expanded, &query, &mut out);
            }
        }
    }
    out
}

/// The lines last flattened and what they show, so a render that changes none of it (a hover, a
/// job's banner, typing in Connection Settings) reuses them instead of walking every schema
/// again.
#[derive(Default)]
pub(super) struct TreeCache {
    lines: Rc<Vec<Node>>,
    made_from: Option<TreeInputs>,
}

/// What the lines were made from: the filter, the open lines, and what in the store can change
/// without a notification.
#[derive(PartialEq)]
struct TreeInputs {
    schema_rev: u64,
    query: String,
    expanded: HashSet<String>,
    /// Each connection's liveness and status line: a link that drops, or a retry countdown,
    /// changes them between the store's notifications.
    live: Vec<(bool, Option<String>)>,
}

impl TreeCache {
    /// Forgets the lines, for a store that changed: the next call flattens again.
    pub(super) fn clear(&mut self) {
        self.made_from = None;
    }

    /// [`flatten`]'s lines: the last ones again while nothing they show changed.
    pub(super) fn lines(
        &mut self,
        store: &DbStore,
        expanded: &HashSet<String>,
        query: &str,
    ) -> Rc<Vec<Node>> {
        let now = Instant::now();
        let live: Vec<_> = store
            .profiles
            .items
            .iter()
            .chain(store.detected.all())
            .map(|p| (store.is_connected(&p.name), store.state(&p.name).label(now)))
            .collect();
        let unchanged = self.made_from.as_ref().is_some_and(|m| {
            m.schema_rev == store.schema_rev
                && m.query == query
                && m.expanded == *expanded
                && m.live == live
        });
        if !unchanged {
            self.lines = Rc::new(flatten(store, expanded, query));
            self.made_from = Some(TreeInputs {
                schema_rev: store.schema_rev,
                query: query.to_string(),
                expanded: expanded.clone(),
                live,
            });
        }
        self.lines.clone()
    }
}

/// The key of the "Cloudflare D1" section (collapsed when in `expanded`).
pub(super) const D1_SECTION: &str = "\u{2}d1";
const UNMATCHED_SECTION: &str = "\u{2}d1-unmatched";

/// One connection's line and, when open, its schemas.
fn push_profile(
    store: &DbStore,
    profile: &ConnProfile,
    saved: bool,
    expanded: &HashSet<String>,
    query: &str,
    out: &mut Vec<Node>,
) {
    {
        let name = profile.name.clone();
        let state = store.state(&name);
        // A D1 row: the binding, then its database's name; "remote" is a badge of its own.
        let (label, detail) = match (&profile.lite, profile.kind) {
            (Some(lite), DbKind::D1Local | DbKind::D1Remote) => (
                lite.binding
                    .clone()
                    .unwrap_or_else(|| profile.database.clone()),
                lite.database_name.clone(),
            ),
            _ => (profile.display_name().to_string(), None),
        };
        out.push(Node::Profile {
            name: name.clone(),
            label,
            detail,
            url: profile.location(),
            kind: profile.kind,
            accessible: store.accessible_name(&name),
            saved,
            route: store.route(&name).map(WriteRoute::describe),
            connected: store.is_connected(&name),
            pending: state.is_pending(),
            status: state.label(std::time::Instant::now()),
        });
        if let Some(e) = state
            .error()
            .or(store.notices.get(&name).map(String::as_str))
        {
            out.push(Node::Error {
                message: e.to_string(),
            });
        }
        if !expanded.contains(&name) {
            return;
        }
        let Some(schemas) = store.schemas.get(&name) else {
            return;
        };
        for schema in schemas {
            let skey = format!("{name}\u{0}{}", schema.name);
            let matching: Vec<&Relation> = schema
                .relations
                .iter()
                .filter(|r| query.is_empty() || r.name.to_lowercase().contains(query))
                .collect();
            if !query.is_empty() && matching.is_empty() {
                continue;
            }
            // `public`, or SQLite's `main`, starts open.
            let sopen = !query.is_empty()
                || (schema.name == profile.kind.default_schema()) != expanded.contains(&skey);
            out.push(Node::Schema {
                key: skey.clone(),
                profile: name.clone(),
                name: schema.name.clone(),
                open: sopen,
                count: matching.len(),
                db: profile.kind,
            });
            if !sopen {
                continue;
            }
            for kind in KIND_ORDER {
                let of_kind: Vec<&Relation> = matching
                    .iter()
                    .copied()
                    .filter(|r| r.kind == kind)
                    .collect();
                if of_kind.is_empty() {
                    continue;
                }
                // Sections start open; the key marks one collapsed.
                let kkey = format!("{skey}\u{0}\u{1}{}", kind_label(kind));
                let kopen = !query.is_empty() || !expanded.contains(&kkey);
                out.push(Node::Kind {
                    key: kkey,
                    kind,
                    count: of_kind.len(),
                    open: kopen,
                });
                if !kopen {
                    continue;
                }
                for rel in of_kind {
                    let rkey = format!("{skey}\u{0}{}", rel.name);
                    // Columns of a relation the role cannot read stay hidden too.
                    let ropen = rel.readable && expanded.contains(&rkey);
                    out.push(Node::Relation {
                        key: rkey.clone(),
                        profile: name.clone(),
                        schema: schema.name.clone(),
                        name: rel.name.clone(),
                        kind: rel.kind,
                        open: ropen,
                        readable: rel.readable,
                        db: profile.kind,
                    });
                    if ropen {
                        relation_children(&rkey, rel, expanded, out);
                    }
                }
            }
        }
    }
}
