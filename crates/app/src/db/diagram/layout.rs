//! Where the cards go and how big they are: the automatic layout, the positions saved per
//! schema once a table is moved, and each card's size.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::*;
use sawit_core::db::diagram;
use sawit_core::instances::AppPaths;

use super::{CARD_PAD, CHAR_W, DiagramView, HEADER_H, MAX_W, MIN_W, ROW_H};

impl DiagramView {
    pub(super) fn place(&mut self, saved: &HashMap<String, (f32, f32)>) {
        let sizes: Vec<_> = self.sizes.iter().map(|s| (s.width, s.height)).collect();
        let edges: Vec<_> = self.erd.edges.iter().map(|e| (e.from, e.to)).collect();
        let auto = diagram::layout(&sizes, &edges);
        let keys: Vec<String> = self.erd.tables.iter().map(|t| t.key()).collect();
        let below = keys
            .iter()
            .zip(&self.sizes)
            .filter_map(|(k, s)| saved.get(k).map(|p| p.1 + s.height))
            .fold(None, |m: Option<f32>, y| Some(m.map_or(y, |m| m.max(y))));
        // Unsaved tables keep their relative auto layout, shifted below.
        let unsaved_top = keys
            .iter()
            .zip(&auto)
            .filter(|(k, _)| !saved.contains_key(*k))
            .map(|(_, p)| p.1)
            .fold(f32::INFINITY, f32::min);
        self.pos = keys
            .iter()
            .zip(&auto)
            .map(|(k, a)| match (saved.get(k), below) {
                (Some(p), _) => point(p.0, p.1),
                (None, Some(b)) => point(a.0, a.1 - unsaved_top + b + 120.),
                (None, None) => point(a.0, a.1),
            })
            .collect();
    }

    pub(super) fn layout_key(&self) -> String {
        format!("{}/{}", self.profile, self.schema)
    }

    pub(super) fn save_layout(&self) {
        let key = self.layout_key();
        let positions: HashMap<String, (f32, f32)> = self
            .erd
            .tables
            .iter()
            .zip(&self.pos)
            .map(|(t, p)| (t.key(), (p.x.round(), p.y.round())))
            .collect();
        std::thread::spawn(move || {
            let mut all = load_layouts();
            all.insert(key, positions);
            if let Ok(json) = serde_json::to_string(&all) {
                let _ = std::fs::write(layouts_file(), json);
            }
        });
    }

    /// Forgets moved positions and lays everything out again.
    pub(super) fn auto_layout(&mut self, cx: &mut Context<Self>) {
        let key = self.layout_key();
        std::thread::spawn(move || {
            let mut all = load_layouts();
            if all.remove(&key).is_some()
                && let Ok(json) = serde_json::to_string(&all)
            {
                let _ = std::fs::write(layouts_file(), json);
            }
        });
        self.place(&HashMap::new());
        self.fit(cx);
    }
}

pub(super) fn card_size(t: &diagram::ErdTable) -> gpui_kit::Size<f32> {
    let header = (t.name.chars().count() + t.schema.chars().count()) as f32 * CHAR_W + 70.;
    let rows = t
        .columns
        .iter()
        .map(|c| {
            (c.name.chars().count() + short_type(&c.data_type).chars().count()) as f32 * CHAR_W
                + 64.
        })
        .fold(0., f32::max);
    size(
        header.max(rows).clamp(MIN_W, MAX_W),
        HEADER_H + CARD_PAD + ROW_H * t.columns.len() as f32,
    )
}

/// Shorter spellings of long Postgres type names for the cards.
pub(super) fn short_type(ty: &str) -> String {
    const SHORT: [(&str, &str); 6] = [
        ("character varying", "varchar"),
        ("timestamp with time zone", "timestamptz"),
        ("timestamp without time zone", "timestamp"),
        ("time with time zone", "timetz"),
        ("time without time zone", "time"),
        ("double precision", "float8"),
    ];
    for (long, short) in SHORT {
        if let Some(rest) = ty.strip_prefix(long) {
            return format!("{short}{rest}");
        }
    }
    ty.to_string()
}

fn layouts_file() -> PathBuf {
    AppPaths::default_macos().root.join("diagram-layouts.json")
}

/// Moved table positions, by `profile/schema` then `schema.table`.
pub(super) fn load_layouts() -> HashMap<String, HashMap<String, (f32, f32)>> {
    std::fs::read_to_string(layouts_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
