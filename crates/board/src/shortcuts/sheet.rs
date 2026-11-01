//! The keyboard-shortcuts sheet: the default keys by group, less the rows Sawit does not offer.

use figcraft_engine::keymap::{self, Group, Platform};

use super::{DefaultKeys, defaults, owned_by_sawit};
use crate::policy::is_hidden;

/// Most keys a row shows: a command's first bindings, as in FigCraft's own panel. The rest stay
/// as alternatives that work without being listed.
const MAX_KEYS: usize = 2;

/// Commands that have keys in the keymap and that the policy does not hide, but that the canvas has
/// no use for: Dev Mode, the switch between design and prototype and a live session's cursor chat
/// (all three are in the spec's Non-goals), F6's walk through the regions of FigCraft's own window
/// (toolbar, layers, canvas, properties), which Sawit's chrome does not have, and FigCraft's
/// keyboard box selection (`select.area`, ⌥Space or F8): it raises a `selectArea` request for a box
/// that the arrows move and Enter selects with, and the canvas never draws that box. So is the
/// Prototype group, Present (⌥⌘↩) and Preview (⇧Space): they ask for FigCraft's prototype player,
/// which the canvas doesn't show (CANVAS-UI-6). The sheet does not advertise them, they get no key
/// binding ([`key_bindings`](super::key_bindings)), and the keymap editor doesn't offer them to
/// bind ([`is_offered`]): only a binding written into the keymap file by hand reaches one.
pub(super) const NOT_OFFERED: &[&str] = &[
    "view.devMode",
    "view.designPrototype",
    "collab.cursorChat",
    "view.nextRegion",
    "view.previousRegion",
    "select.area",
    "proto.present",
    "proto.preview",
];

/// Whether the canvas offers command `id` to the keyboard: the policy doesn't hide it
/// ([`is_hidden`]) and the canvas has a use for it (it isn't one of `NOT_OFFERED`). The sheet, the
/// key bindings ([`key_bindings`](super::key_bindings)) and the keymap editor's rows for commands
/// with no key offer only these.
pub fn is_offered(id: &str) -> bool {
    !is_hidden(id) && !NOT_OFFERED.contains(&id)
}

/// One row of the sheet: a command and its keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutRow {
    pub id: &'static str,
    /// What the row calls the command: FigCraft's label for it in its own panel.
    pub label: String,
    /// The keys as a Mac menu writes them (`⌘K`). A command with two keys that read differently
    /// shows both, joined with ` or ` (`⌘K or ⌘/`).
    pub keys: String,
}

/// A group of rows: one of the tabs of FigCraft's own panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutGroup {
    pub name: String,
    pub rows: Vec<ShortcutRow>,
}

/// The sheet: the default keys ([`defaults`]) by group, FigCraft's in its keymap's order and then
/// Sawit's, without the rows described in the module's docs. A group with no row left is not
/// listed.
pub fn sheet() -> Vec<ShortcutGroup> {
    Group::ALL
        .into_iter()
        .filter(|group| *group != Group::Prototype)
        .filter_map(|group| {
            let rows: Vec<ShortcutRow> = defaults()
                .filter(|binding| binding.group == group)
                .filter_map(|binding| row(&binding))
                .collect();
            (!rows.is_empty()).then(|| ShortcutGroup {
                name: group.label().to_string(),
                rows,
            })
        })
        .collect()
}

/// The sheet's row for `binding`, if it has one.
fn row(binding: &DefaultKeys) -> Option<ShortcutRow> {
    if !is_offered(binding.id) {
        return None;
    }
    let mut keys: Vec<String> = Vec::new();
    for spec in binding.keys {
        if owned_by_sawit(spec) {
            continue;
        }
        // Alternatives that read the same (⌘+ for Cmd+= and for Shift+Cmd+=) show once.
        let shown = keymap::display(spec, Platform::Mac);
        if keys.len() < MAX_KEYS && !keys.contains(&shown) {
            keys.push(shown);
        }
    }
    if keys.is_empty() {
        return None;
    }
    Some(ShortcutRow {
        id: binding.id,
        label: binding.title.to_string(),
        keys: keys.join(" or "),
    })
}
