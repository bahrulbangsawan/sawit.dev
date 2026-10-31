//! The keyboard-shortcuts sheet's model: FigCraft's keymap, grouped as FigCraft's own panel groups
//! it, less the rows Sawit does not offer. The view in `crates/app/src/board/menus.rs` draws it
//! (⌃⇧?, or Help ▸ Keyboard shortcuts).
//!
//! A row is left out when:
//! - its command is hidden ([`crate::policy::is_hidden`]), because the keyboard cannot reach it;
//! - its command has nothing to do in the canvas, although the keyboard reaches it: Dev Mode, a
//!   live session's cursor chat, F6's walk through FigCraft's window regions, and the keyboard box
//!   selection (⌥Space, F8), whose box the canvas never draws;
//! - it belongs to the Prototype group, which is all Present and Preview.
//!
//! A key is left out when Sawit keeps it ([`crate::policy::SAWIT_WINS`]: ⌘W, ⌘J, ⌃⌘M…). A row
//! with no key left goes too, so it shows nothing for a key the canvas never receives. The keys
//! Sawit keeps only while the canvas is not editing text (⌘B, ⌥⌘T, ⇧⌘↵) stay: they work then.

use figcraft_engine::keymap::{self, Binding, Group, KEYS, Platform};

use crate::input::{TEXT_CLIPBOARD, gpui_keystroke};
use crate::policy::{SAWIT_WINS, SAWIT_WINS_UNLESS_EDITING, is_hidden};

/// Most keys a row shows: a command's first bindings, as in FigCraft's own panel. The rest stay
/// as alternatives that work without being listed.
const MAX_KEYS: usize = 2;

/// Commands that have keys in the keymap and that the policy does not hide, but that the canvas
/// has no use for: Dev Mode, the switch between design and prototype and a live session's cursor
/// chat (all three are in the spec's Non-goals), F6's walk through the regions of FigCraft's own
/// window (toolbar, layers, canvas, properties), which Sawit's chrome does not have, and
/// FigCraft's keyboard box selection (`select.area`, ⌥Space or F8): it raises a `selectArea`
/// request for a box that the arrows move and Enter selects with, and the canvas never draws that
/// box. The keyboard still reaches them; the sheet does not advertise them.
const NOT_OFFERED: &[&str] = &[
    "view.devMode",
    "view.designPrototype",
    "collab.cursorChat",
    "view.nextRegion",
    "view.previousRegion",
    "select.area",
];

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

/// The sheet: FigCraft's keymap by group, in the keymap's order, without the rows described in
/// the module's docs. A group with no row left is not listed.
pub fn sheet() -> Vec<ShortcutGroup> {
    Group::ALL
        .into_iter()
        .filter(|group| *group != Group::Prototype)
        .filter_map(|group| {
            let rows: Vec<ShortcutRow> = KEYS
                .iter()
                .filter(|binding| binding.group == group)
                .filter_map(row)
                .collect();
            (!rows.is_empty()).then(|| ShortcutGroup {
                name: group.label().to_string(),
                rows,
            })
        })
        .collect()
}

/// The sheet's row for `binding`, if it has one.
fn row(binding: &Binding) -> Option<ShortcutRow> {
    if is_hidden(binding.id) || NOT_OFFERED.contains(&binding.id) {
        return None;
    }
    let mut keys: Vec<String> = Vec::new();
    for spec in binding.keys(Platform::Mac) {
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
        label: binding.title().to_string(),
        keys: keys.join(" or "),
    })
}

/// The app's key context of the canvas while it doesn't edit text, for its bindings.
pub const CANVAS_CONTEXT: &str = "Board && !BoardText";
/// The app's key context of the canvas while it edits text.
pub const TEXT_CONTEXT: &str = "BoardText";

/// One of FigCraft's shortcuts as an app key binding: its GPUI keystroke, its command and the key
/// context it is live in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    pub keystroke: String,
    pub id: &'static str,
    pub context: &'static str,
}

/// FigCraft's shortcuts as the app binds them, which the keymap editor shows and changes. Each
/// command's keys, but those of the commands the sheet leaves out (hidden, or of no use in the
/// canvas) and the keys Sawit keeps ([`SAWIT_WINS`]), in FigCraft's order:
///
/// - On the canvas ([`CANVAS_CONTEXT`]), the keys of the commands live there, but those Sawit
///   keeps while no text is edited ([`SAWIT_WINS_UNLESS_EDITING`]).
/// - While text is edited ([`TEXT_CONTEXT`]), the keys with ⌘, ⌥ or ⌃ of the commands live
///   there, and of copy and paste, which act on the text: plain keys type.
pub fn key_bindings() -> Vec<KeyBinding> {
    let mut out = Vec::new();
    for binding in KEYS.iter() {
        if is_hidden(binding.id) || NOT_OFFERED.contains(&binding.id) {
            continue;
        }
        for spec in binding.keys(Platform::Mac) {
            let Some(keystroke) = gpui_keystroke(spec) else {
                continue;
            };
            if SAWIT_WINS.contains(&keystroke.as_str()) {
                continue;
            }
            let Some(bound) = binding_keystroke(spec) else {
                continue;
            };
            let modified = ["ctrl-", "alt-", "cmd-"].iter().any(|m| bound.contains(m));
            let mut push = |context| {
                let key = KeyBinding {
                    keystroke: bound.clone(),
                    id: binding.id,
                    context,
                };
                if !out.contains(&key) {
                    out.push(key);
                }
            };
            if binding.context != keymap::Context::Text
                && !SAWIT_WINS_UNLESS_EDITING.contains(&keystroke.as_str())
            {
                push(CANVAS_CONTEXT);
            }
            let in_text =
                binding.context != keymap::Context::Canvas || TEXT_CLIPBOARD.contains(&binding.id);
            if in_text && modified {
                push(TEXT_CONTEXT);
            }
        }
    }
    out
}

/// The US keyboard's shifted symbols, each with its key.
const SHIFTED: &[(char, char)] = &[
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    ('|', '\\'),
    (':', ';'),
    ('"', '\''),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
    ('~', '`'),
];

/// The keystroke a binding for `spec` matches, which is [`gpui_keystroke`]'s but for a shifted
/// symbol: GPUI reports ⇧1 as `!` and ⌃⇧/ as `ctrl-?`, Shift off, so the binding is the symbol
/// without Shift (`Shift+1` is `!`, `Ctrl+Shift+?` is `ctrl-?`). A letter keeps its Shift.
pub fn binding_keystroke(spec: &str) -> Option<String> {
    let keystroke = gpui_keystroke(spec)?;
    // The modifiers end at the last `-` but one when the key is `-` itself (`cmd--`).
    let split = if keystroke.ends_with("--") {
        keystroke.len() - 2
    } else {
        keystroke.rfind('-').unwrap_or(0)
    };
    let (modifiers, key) = if keystroke[split..].starts_with('-') {
        (&keystroke[..split], &keystroke[split + 1..])
    } else {
        ("", keystroke.as_str())
    };
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return Some(keystroke);
    };
    let shifted = modifiers.split('-').any(|m| m == "shift");
    if !shifted || c.is_ascii_alphabetic() {
        return Some(keystroke);
    }
    let Some(&(symbol, _)) = SHIFTED
        .iter()
        .find(|(symbol, key)| *symbol == c || *key == c)
    else {
        return Some(keystroke);
    };
    let mut out: String = modifiers
        .split('-')
        .filter(|m| !m.is_empty() && *m != "shift")
        .map(|m| format!("{m}-"))
        .collect();
    out.push(symbol);
    Some(out)
}

/// Whether Sawit keeps `spec`, a FigCraft binding (`Cmd+W`), when the canvas has the keyboard.
/// The policy writes its keys as GPUI does, so `spec` is converted to compare.
pub(crate) fn owned_by_sawit(spec: &str) -> bool {
    gpui_keystroke(spec).is_some_and(|keystroke| SAWIT_WINS.contains(&keystroke.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row of the sheet, group by group.
    fn rows() -> Vec<ShortcutRow> {
        sheet().into_iter().flat_map(|group| group.rows).collect()
    }

    /// The row for command `id`.
    fn row_of(id: &str) -> Option<ShortcutRow> {
        rows().into_iter().find(|row| row.id == id)
    }

    /// The keys of `row`, one by one.
    fn keys(row: &ShortcutRow) -> Vec<&str> {
        row.keys.split(" or ").collect()
    }

    /// Whether any row of the sheet shows `key` (`⌘J`).
    fn has_key(key: &str) -> bool {
        rows().iter().any(|row| keys(row).contains(&key))
    }

    #[test]
    fn sheet_has_a_group_for_each_keymap_group_but_prototype() {
        let sheet = sheet();
        let names: Vec<&str> = sheet.iter().map(|group| group.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Essential",
                "Tools",
                "View",
                "Zoom",
                "Text",
                "Shape",
                "Selection",
                "Edit",
                "Transform",
                "Arrange",
                "Components",
            ]
        );
        assert!(sheet.iter().all(|group| !group.rows.is_empty()));
        assert!(rows().iter().all(|row| !row.keys.is_empty()));
        // The keymap's order within a group, less the rows that are gone.
        let essential: Vec<&str> = sheet[0].rows.iter().map(|row| row.id).collect();
        assert_eq!(
            essential,
            [
                "app.actions",
                "help.shortcuts",
                "view.toggleUi",
                "file.save",
                "file.saveAs"
            ]
        );
    }

    #[test]
    fn sheet_drops_the_keys_sawit_keeps() {
        // The rest of those commands' group is on the sheet, so an empty sheet cannot pass this.
        assert!(row_of("vector.flatten").is_some());
        // ⌃⌘M (Mask) and ⌘J (Join selection) are no hidden commands: only the key filter takes
        // them out. If it were missing, these rows would show.
        for id in ["object.mask", "vector.join"] {
            assert!(!is_hidden(id), "{id} is a hidden command");
            assert!(KEYS.iter().any(|b| b.id == id), "{id} has no keys to drop");
            assert!(row_of(id).is_none(), "{id} is on the sheet");
        }
        // Every key of the policy's list as the keymap binds it, worked out from the list itself.
        let kept: Vec<String> = KEYS
            .iter()
            .flat_map(|binding| binding.keys(Platform::Mac))
            .filter(|spec| owned_by_sawit(spec))
            .map(|spec| keymap::display(spec, Platform::Mac))
            .collect();
        assert_eq!(kept.len(), SAWIT_WINS.len(), "{kept:?}");
        // Those are the keys written out: no typing slip leaves the check below empty.
        for key in ["⌘W", "⌘O", "⌃⇥", "⌃⇧⇥", "⌃⌘M", "⌥⌘O", "⌘J", "⇧⌘J"]
        {
            assert!(kept.iter().any(|k| k == key), "{key} is not in {kept:?}");
        }
        for key in &kept {
            assert!(!has_key(key), "the sheet shows {key}");
        }
        // Outline stroke (⌥⌘O) and Smooth join (⇧⌘J) have no other key, so their rows go too.
        for id in ["vector.outlineStroke", "vector.smoothJoin"] {
            assert!(row_of(id).is_none(), "{id} is on the sheet");
        }
    }

    #[test]
    fn sheet_keeps_the_keys_the_canvas_has_while_text_is_edited() {
        // ⌘B, ⌥⌘T and ⇧⌘↵ are Sawit's unless text is edited, and they work then, so they show.
        let shown = |id| row_of(id).map(|row| row.keys);
        assert_eq!(shown("text.bold").as_deref(), Some("⌘B"));
        assert_eq!(shown("text.alignCenter").as_deref(), Some("⌥⌘T"));
        assert_eq!(shown("board.nextSticky").as_deref(), Some("⇧⌘↵"));
    }

    #[test]
    fn sheet_skips_what_sawit_does_not_have() {
        for id in [
            "proto.present",
            "proto.preview",
            "view.devMode",
            "view.designPrototype",
            "collab.cursorChat",
            "view.nextRegion",
            "view.previousRegion",
            "select.area",
        ] {
            assert!(
                KEYS.iter().any(|b| b.id == id),
                "{id} has no keys, so the test is empty"
            );
            assert!(row_of(id).is_none(), "{id} is on the sheet");
        }
        // The context menu has ⇧F10, which the view opens at the selection.
        assert_eq!(
            row_of("view.contextMenu").map(|row| row.keys).as_deref(),
            Some("⇧F10")
        );
        // The left sidebar has ⇧⌘\, which the view answers by toggling the layers panel.
        assert_eq!(
            row_of("view.leftSidebar").map(|row| row.keys).as_deref(),
            Some("⇧⌘\\")
        );
    }

    #[test]
    fn sheet_shows_each_alternative_once_and_at_most_two() {
        let shown = |id| row_of(id).map(|row| row.keys);
        assert_eq!(shown("app.actions").as_deref(), Some("⌘K or ⌘/"));
        assert_eq!(shown("tool.frame").as_deref(), Some("F or A"));
        // Cmd+=, Shift+=, Shift+Cmd+= and = are written ⌘+, +, ⌘+ and +.
        assert_eq!(shown("view.zoomIn").as_deref(), Some("⌘+ or +"));
        // Zoom out has four keys that read differently. The first two show.
        assert_eq!(shown("view.zoomOut").as_deref(), Some("⌘- or -"));
        for row in rows() {
            let keys = keys(&row);
            assert!(keys.len() <= 2, "{row:?}");
            assert!(keys.iter().all(|key| !key.is_empty()), "{row:?}");
        }
    }

    #[test]
    fn sheet_labels_are_figcrafts_labels() {
        let label = |id| row_of(id).map(|row| row.label);
        // The panel's own label, where the keymap has one.
        assert_eq!(label("app.actions").as_deref(), Some("Actions…"));
        assert_eq!(label("view.layers").as_deref(), Some("Layers panel"));
        // The menu's label otherwise.
        assert_eq!(label("edit.duplicate").as_deref(), Some("Duplicate"));
        assert_eq!(
            label("help.shortcuts").as_deref(),
            Some("Keyboard shortcuts")
        );
    }

    #[test]
    fn owned_by_sawit_reads_keymap_bindings_as_gpui_keystrokes() {
        for spec in [
            "Cmd+W",
            "Cmd+O",
            "Ctrl+Tab",
            "Ctrl+Shift+Tab",
            "Ctrl+Cmd+M",
            "Alt+Cmd+O",
            "Cmd+J",
            "Shift+Cmd+J",
        ] {
            assert!(owned_by_sawit(spec), "{spec} is Sawit's");
        }
        // Close, but not the same key: these go to the canvas.
        for spec in ["Cmd+D", "Shift+Cmd+W", "Cmd+B", "Cmd+K", "J", "garbage", ""] {
            assert!(!owned_by_sawit(spec), "{spec} is the canvas's");
        }
    }

    #[test]
    fn binding_keystrokes_read_shifted_symbols_as_gpui_reports_them() {
        assert_eq!(binding_keystroke("Shift+1").as_deref(), Some("!"));
        assert_eq!(binding_keystroke("Shift+=").as_deref(), Some("+"));
        assert_eq!(binding_keystroke("Shift+Cmd+=").as_deref(), Some("cmd-+"));
        assert_eq!(binding_keystroke("Ctrl+Shift+?").as_deref(), Some("ctrl-?"));
        assert_eq!(
            binding_keystroke("Shift+Cmd+K").as_deref(),
            Some("shift-cmd-k")
        );
        assert_eq!(binding_keystroke("Cmd+-").as_deref(), Some("cmd--"));
        assert_eq!(binding_keystroke("V").as_deref(), Some("v"));
        assert_eq!(
            binding_keystroke("Alt+Cmd+Enter").as_deref(),
            Some("alt-cmd-enter")
        );
    }

    #[test]
    fn key_bindings_leave_sawits_keys_and_hidden_commands_out() {
        let bindings = key_bindings();
        assert!(!bindings.is_empty());
        for binding in &bindings {
            assert!(!is_hidden(binding.id), "{binding:?}");
            assert!(!NOT_OFFERED.contains(&binding.id), "{binding:?}");
            assert!(
                !SAWIT_WINS.contains(&binding.keystroke.as_str()),
                "{binding:?}"
            );
            if binding.context == CANVAS_CONTEXT {
                assert!(
                    !SAWIT_WINS_UNLESS_EDITING.contains(&binding.keystroke.as_str()),
                    "{binding:?}"
                );
            }
        }
        let has = |keystroke: &str, id: &str, context: &str| {
            bindings
                .iter()
                .any(|b| b.keystroke == keystroke && b.id == id && b.context == context)
        };
        assert!(has("v", "tool.move", CANVAS_CONTEXT));
        // A plain key types while text is edited.
        assert!(!has("v", "tool.move", TEXT_CONTEXT));
        // Copy acts on the text being edited.
        assert!(has("cmd-c", "edit.copy", TEXT_CONTEXT));
        assert!(has("cmd-c", "edit.copy", CANVAS_CONTEXT));
    }
}
