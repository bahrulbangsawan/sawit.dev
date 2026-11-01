//! The keyboard-shortcuts sheet's model: the canvas's default keys (FigCraft's keymap, then
//! Sawit's), grouped as FigCraft's own panel groups them, less the rows Sawit does not offer. The
//! view in `crates/app/src/board/menus.rs` draws it (⌃⇧?, or Help ▸ Keyboard shortcuts).
//!
//! A row is left out when:
//! - its command is hidden ([`crate::policy::is_hidden`]), because the keyboard cannot reach it;
//! - its command has nothing to do in the canvas: Dev Mode, a live session's cursor chat, F6's
//!   walk through FigCraft's window regions, and the keyboard box selection (⌥Space, F8), whose
//!   box the canvas never draws;
//! - it belongs to the Prototype group, which is all Present and Preview.
//!
//! A key is left out when Sawit keeps it ([`crate::policy::SAWIT_WINS`]: ⌘W, ⌘J, ⌃⌘M…). A row
//! with no key left goes too, so it shows nothing for a key the canvas never receives. The keys
//! Sawit keeps only while the canvas is not editing text (⌘B, ⌥⌘T, ⇧⌘↵) stay: they work then.
//!
//! FigCraft's keymap leaves many of the canvas's commands without a key. Sawit gives them keys of
//! its own ([`SAWIT_KEYS`]), on keys neither FigCraft nor the app uses, and so it does for the
//! commands whose FigCraft key Sawit keeps (Use as mask, Outline stroke, Join): the sheet lists
//! them in their groups, after FigCraft's rows. [`defaults`] is both together.
//!
//! The same keys are the app's key bindings of the canvas ([`key_bindings`]), which the keymap
//! editor shows and changes. The sheet here lists the default keys; the app shows each row's keys
//! as the keymap binds them now (its `board::command_keys_all`), and leaves out a row whose keys
//! the user took out.

use figcraft_engine::keymap::{self, Context, Group, KEYS, Platform};

use crate::input::{TEXT_CLIPBOARD, gpui_keystroke};
use crate::policy::{SAWIT_WINS, SAWIT_WINS_UNLESS_EDITING, is_hidden};

/// A key Sawit gives one of the canvas's commands that FigCraft's keymap leaves without one, or
/// whose FigCraft key Sawit keeps. Its keys are live on the canvas, not while text is edited.
#[derive(Clone, Copy, Debug)]
pub struct SawitKey {
    /// The group of the shortcuts sheet it is listed in.
    pub group: Group,
    pub id: &'static str,
    /// The Mac keys, written as FigCraft's keymap writes them (`Alt+Shift+Cmd+G`).
    pub keys: &'static [&'static str],
    /// What the sheet and the keymap editor call the command, where its menu label says too
    /// little out of its menu ("Fill" in Edit ▸ Select all with same); empty for its menu label.
    label: &'static str,
}

impl SawitKey {
    /// What the sheet and the keymap editor call the command: its own label, else its menu's.
    pub fn title(&self) -> &'static str {
        if self.label.is_empty() {
            title(self.id)
        } else {
            self.label
        }
    }

    const fn label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }
}

const fn sawit(group: Group, id: &'static str, keys: &'static [&'static str]) -> SawitKey {
    SawitKey {
        group,
        id,
        keys,
        label: "",
    }
}

/// The keys Sawit gives the canvas's commands besides FigCraft's ([`SawitKey`]). Figma and
/// FigJam have no key for these either, so each family of commands shares its modifiers and
/// FigCraft's neighbouring keys:
///
/// - Polygon and Star: ⇧6 and ⇧8, the keys of `^` (a triangle, the polygon the tool draws) and
///   `*` (a star). Text on a path: ⇧T, beside Text's T.
/// - Copy as and Export: Copy as PNG is FigCraft's ⇧⌘C and Export its ⇧⌘E; ⌥ makes them SVG, ⌃
///   CSS for a copy and frames to PDF for an export, and ⌃⌥⌘C copies the text.
/// - Select all with same: ⌃⇧ and the first letter of what is the same (Fill, Stroke, Effect,
///   Text, fOnt, Instance, Properties).
/// - Where Sawit keeps FigCraft's Mac key (⌃⌘M, ⌥⌘O, ⌘J, ⇧⌘J), the command takes FigCraft's
///   Windows and Linux key, with ⌃ for Ctrl: Use as mask ⌃⌥M, Outline stroke ⌃⌥O, Join ⌃J,
///   Smooth join ⌃⇧J. Release mask is ⌃⌥⇧M; Simplify and Reverse path are ⌃⌥S and ⌃⌥R, Crop
///   image ⌃⌥C.
/// - Beside FigCraft's own: Remove frame ⌥⇧⌘G (Frame selection is ⌥⌘G), Hide other layers ⌥⇧⌘H
///   (Show/Hide is ⇧⌘H), Create multiple components ⌥⇧⌘K (Create component is ⌥⌘K), Reset
///   instance ⌥⇧⌘B (Detach is ⌥⌘B), Push changes to and Restore the main component ⌃⌥⌘P and
///   ⌃⌥⌘R (Go to main is ⌃⌥⌘K), Combine as variants ⌃⌥K, Show outlines for the selection ⌥⇧⌘O
///   (Show outlines is ⇧⌘O), Soft-proof sRGB ⌃⌥P (Pixel preview is ⌃P), Pack ⌃⌥⇧H and ⌃⌥⇧V
///   (Distribute is ⌃⌥H and ⌃⌥V).
/// - Rotate 90° left and right ⌥⇧L and ⌥⇧R, 180° ⌥⇧T; Round to pixels ⌥⇧P, Rasterize ⌥⇧⌘P.
/// - Text case: ⌥⇧⌘ and U (upper), L (lower), T (title), N (as typed).
/// - Zoom to 50% and 200%: ⌥⇧- and ⌥⇧=, beside zoom out and in.
/// - Snap to geometry and to objects: ⌥⇧⌘' and ⌃⇧⌘', beside FigCraft's ⇧⌘' (snap to pixel grid).
///
/// None is a key the app keeps on the canvas ([`SAWIT_WINS`]) or one of FigCraft's, and the app's
/// tests prove that none is a key of the app's.
pub static SAWIT_KEYS: &[SawitKey] = &[
    // Tools
    sawit(Group::Tools, "tool.polygon", &["Shift+6"]),
    sawit(Group::Tools, "tool.star", &["Shift+8"]),
    sawit(Group::Tools, "tool.textOnPath", &["Shift+T"]),
    // View
    sawit(Group::View, "view.outlinesSelection", &["Alt+Shift+Cmd+O"]),
    sawit(Group::View, "view.proofSrgb", &["Ctrl+Alt+P"]),
    sawit(Group::View, "view.snapGeometry", &["Alt+Shift+Cmd+'"]),
    sawit(Group::View, "view.snapObjects", &["Ctrl+Shift+Cmd+'"]),
    // Zoom
    sawit(Group::Zoom, "view.zoom50", &["Alt+Shift+-"]),
    sawit(Group::Zoom, "view.zoom200", &["Alt+Shift+="]),
    // Text
    sawit(Group::Text, "text.caseUpper", &["Alt+Shift+Cmd+U"]),
    sawit(Group::Text, "text.caseLower", &["Alt+Shift+Cmd+L"]),
    sawit(Group::Text, "text.caseTitle", &["Alt+Shift+Cmd+T"]),
    sawit(Group::Text, "text.caseOriginal", &["Alt+Shift+Cmd+N"]),
    // Shape
    sawit(Group::Shape, "vector.outlineStroke", &["Ctrl+Alt+O"]),
    sawit(Group::Shape, "vector.join", &["Ctrl+J"]),
    sawit(Group::Shape, "vector.smoothJoin", &["Ctrl+Shift+J"]),
    sawit(Group::Shape, "vector.simplify", &["Ctrl+Alt+S"]),
    sawit(Group::Shape, "vector.reverse", &["Ctrl+Alt+R"]),
    sawit(Group::Shape, "object.rasterize", &["Alt+Shift+Cmd+P"]),
    // Selection
    sawit(Group::Selection, "select.sameFill", &["Ctrl+Shift+F"])
        .label("Select all with same fill"),
    sawit(Group::Selection, "select.sameStroke", &["Ctrl+Shift+S"])
        .label("Select all with same stroke"),
    sawit(Group::Selection, "select.sameEffect", &["Ctrl+Shift+E"])
        .label("Select all with same effect"),
    sawit(Group::Selection, "select.sameText", &["Ctrl+Shift+T"])
        .label("Select all with same text properties"),
    sawit(Group::Selection, "select.sameFont", &["Ctrl+Shift+O"])
        .label("Select all with same font"),
    sawit(Group::Selection, "select.sameInstance", &["Ctrl+Shift+I"])
        .label("Select all with same instance"),
    sawit(Group::Selection, "select.sameProperties", &["Ctrl+Shift+P"])
        .label("Select all with same properties"),
    sawit(Group::Selection, "object.removeFrame", &["Alt+Shift+Cmd+G"]),
    sawit(Group::Selection, "object.hideOthers", &["Alt+Shift+Cmd+H"]),
    // Edit
    sawit(Group::Edit, "edit.copyAsText", &["Ctrl+Alt+Cmd+C"]),
    sawit(Group::Edit, "edit.copyAsCss", &["Ctrl+Shift+Cmd+C"]),
    sawit(Group::Edit, "edit.copyAsSvg", &["Alt+Shift+Cmd+C"]),
    sawit(Group::Edit, "file.exportSvg", &["Alt+Shift+Cmd+E"]),
    sawit(Group::Edit, "file.exportFramesPdf", &["Ctrl+Shift+Cmd+E"]),
    // Transform
    sawit(Group::Transform, "object.mask", &["Ctrl+Alt+M"]),
    sawit(
        Group::Transform,
        "object.releaseMask",
        &["Ctrl+Alt+Shift+M"],
    ),
    sawit(Group::Transform, "image.crop", &["Ctrl+Alt+C"]),
    sawit(Group::Transform, "object.rotateLeft", &["Alt+Shift+L"]),
    sawit(Group::Transform, "object.rotateRight", &["Alt+Shift+R"]),
    sawit(Group::Transform, "object.rotate180", &["Alt+Shift+T"]),
    // Arrange
    sawit(Group::Arrange, "arrange.packH", &["Ctrl+Alt+Shift+H"]),
    sawit(Group::Arrange, "arrange.packV", &["Ctrl+Alt+Shift+V"]),
    sawit(Group::Arrange, "arrange.roundToPixels", &["Alt+Shift+P"]),
    // Components
    sawit(
        Group::Components,
        "component.createMultiple",
        &["Alt+Shift+Cmd+K"],
    ),
    sawit(
        Group::Components,
        "component.combineVariants",
        &["Ctrl+Alt+K"],
    ),
    sawit(
        Group::Components,
        "component.resetInstance",
        &["Alt+Shift+Cmd+B"],
    ),
    sawit(
        Group::Components,
        "component.pushChanges",
        &["Ctrl+Alt+Cmd+P"],
    ),
    sawit(
        Group::Components,
        "component.restoreMain",
        &["Ctrl+Alt+Cmd+R"],
    ),
];

/// One command's default keys on the canvas: FigCraft's ([`KEYS`]) or Sawit's ([`SAWIT_KEYS`]).
#[derive(Clone, Copy, Debug)]
pub struct DefaultKeys {
    pub group: Group,
    pub id: &'static str,
    /// The Mac keys, written as FigCraft's keymap writes them, the first being the one a menu
    /// shows.
    pub keys: &'static [&'static str],
    /// Where they are live: on the canvas, while text is edited, or both.
    pub context: Context,
    /// What the shortcuts sheet calls the command: FigCraft's label for it in its own panel, else
    /// its menu's, else the command's.
    pub title: &'static str,
}

/// Every command's default keys on the canvas, FigCraft's first, in its keymap's order, then
/// Sawit's ([`SAWIT_KEYS`]).
pub fn defaults() -> impl Iterator<Item = DefaultKeys> {
    let figcraft = KEYS.iter().map(|binding| DefaultKeys {
        group: binding.group,
        id: binding.id,
        keys: binding.keys(Platform::Mac),
        context: binding.context,
        title: binding.title(),
    });
    let sawits = SAWIT_KEYS.iter().map(|key| DefaultKeys {
        group: key.group,
        id: key.id,
        keys: key.keys,
        context: Context::Canvas,
        title: key.title(),
    });
    figcraft.chain(sawits)
}

/// The first default key of command `id` ([`defaults`]) that the canvas gets, as FigCraft's
/// keymap writes it: what a menu shows. Not one Sawit keeps ([`owned_by_sawit`]): Join shows
/// Sawit's ⌃J, not FigCraft's ⌘J. `None` when it has none.
pub fn primary(id: &str) -> Option<&'static str> {
    defaults()
        .filter(|keys| keys.id == id)
        .flat_map(|keys| keys.keys.iter().copied())
        .find(|spec| !owned_by_sawit(spec))
}

/// What FigCraft calls command `id`: its menu item's label, else the command's own.
fn title(id: &'static str) -> &'static str {
    figcraft_engine::catalog::items()
        .find(|item| item.id == id)
        .map(|item| item.label)
        .or_else(|| figcraft_engine::find_command(id).map(|command| command.label))
        .unwrap_or(id)
}

/// Most keys a row shows: a command's first bindings, as in FigCraft's own panel. The rest stay
/// as alternatives that work without being listed.
const MAX_KEYS: usize = 2;

/// Commands that have keys in the keymap and that the policy does not hide, but that the canvas
/// has no use for: Dev Mode, the switch between design and prototype and a live session's cursor
/// chat (all three are in the spec's Non-goals), F6's walk through the regions of FigCraft's own
/// window (toolbar, layers, canvas, properties), which Sawit's chrome does not have, and
/// FigCraft's keyboard box selection (`select.area`, ⌥Space or F8): it raises a `selectArea`
/// request for a box that the arrows move and Enter selects with, and the canvas never draws that
/// box. So is the Prototype group, Present (⌥⌘↩) and Preview (⇧Space): they ask for FigCraft's
/// prototype player, which the canvas doesn't show (CANVAS-UI-6). The sheet does not advertise
/// them, they get no key binding ([`key_bindings`]), and the keymap editor doesn't offer them to
/// bind ([`is_offered`]): only a binding written into the keymap file by hand reaches one.
const NOT_OFFERED: &[&str] = &[
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
/// ([`is_hidden`]) and the canvas has a use for it (it isn't one of `NOT_OFFERED`). The sheet,
/// the key bindings ([`key_bindings`]) and the keymap editor's rows for commands with no key
/// offer only these.
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

/// The canvas's default keys ([`defaults`]: FigCraft's shortcuts, then Sawit's) as the app binds
/// them, which the keymap editor shows and changes. Each command's keys, but those of the
/// commands the sheet leaves out (hidden, or of no use in the canvas) and the keys Sawit keeps
/// ([`SAWIT_WINS`]), in [`defaults`]' order:
///
/// - On the canvas ([`CANVAS_CONTEXT`]), the keys of the commands live there, but those Sawit
///   keeps while no text is edited ([`SAWIT_WINS_UNLESS_EDITING`]).
/// - While text is edited ([`TEXT_CONTEXT`]), the keys with ⌘, ⌥ or ⌃ of the commands live
///   there, and of copy and paste, which act on the text: plain keys type.
pub fn key_bindings() -> Vec<KeyBinding> {
    let mut out = Vec::new();
    for binding in defaults() {
        if !is_offered(binding.id) {
            continue;
        }
        for spec in binding.keys {
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

/// The key that types `symbol` with Shift on a US keyboard (`.` for `>`), when `symbol` is one of
/// its shifted symbols: what a binding of the symbol, written as GPUI reports it
/// ([`binding_keystroke`]), is pressed with. `None` for any other character.
pub fn unshifted(symbol: char) -> Option<char> {
    SHIFTED
        .iter()
        .find(|&&(shifted, _)| shifted == symbol)
        .map(|&(_, key)| key)
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
        // them out. If it were missing, these rows would show them. Their rows show Sawit's keys
        // for them instead.
        for (id, sawits) in [("object.mask", "⌃⌥M"), ("vector.join", "⌃J")] {
            assert!(!is_hidden(id), "{id} is a hidden command");
            assert!(KEYS.iter().any(|b| b.id == id), "{id} has no keys to drop");
            assert_eq!(
                row_of(id).map(|row| row.keys).as_deref(),
                Some(sawits),
                "{id}"
            );
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
        // Outline stroke (⌥⌘O) and Smooth join (⇧⌘J) have no other FigCraft key: their rows show
        // only Sawit's.
        for (id, sawits) in [
            ("vector.outlineStroke", "⌃⌥O"),
            ("vector.smoothJoin", "⌃⇧J"),
        ] {
            assert_eq!(
                row_of(id).map(|row| row.keys).as_deref(),
                Some(sawits),
                "{id}"
            );
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
    fn unshifted_gives_the_key_of_a_shifted_symbol() {
        assert_eq!(unshifted('>'), Some('.'));
        assert_eq!(unshifted('?'), Some('/'));
        assert_eq!(unshifted('|'), Some('\\'));
        assert_eq!(unshifted('!'), Some('1'));
        // A key is no shifted symbol, and neither is a letter.
        for &(symbol, key) in SHIFTED {
            assert_eq!(unshifted(symbol), Some(key));
            assert_eq!(unshifted(key), None, "{key}");
        }
        assert_eq!(unshifted('k'), None);
        assert_eq!(unshifted('K'), None);
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

    /// CANVAS-UI-6: the Prototype group's commands, Present (⌥⌘↩) and Preview (⇧Space), open
    /// FigCraft's prototype player, which the canvas can't show: it drops the player's requests,
    /// or says "no frame to present". The sheet leaves the group out, and so do the key bindings
    /// and the keymap editor. Any command a FigCraft bump adds to the group is held to it too.
    #[test]
    fn the_prototype_players_commands_get_no_key() {
        let prototype: Vec<&str> = KEYS
            .iter()
            .filter(|binding| binding.group == Group::Prototype)
            .map(|binding| binding.id)
            .collect();
        assert!(prototype.contains(&"proto.present"), "{prototype:?}");
        assert!(prototype.contains(&"proto.preview"), "{prototype:?}");
        let bindings = key_bindings();
        for id in prototype {
            assert!(!is_offered(id), "{id} is offered");
            assert!(
                bindings.iter().all(|binding| binding.id != id),
                "{id} has a key"
            );
        }
        // ⇧Space and ⌥⌘↩ are no keys of the canvas.
        for keystroke in ["shift-space", "alt-cmd-enter"] {
            assert!(
                bindings
                    .iter()
                    .all(|binding| binding.keystroke != keystroke),
                "{keystroke} is bound"
            );
        }
    }

    /// The commands the canvas offers that get no key by default, each with why. Every other one
    /// has a key ([`every_offered_command_has_a_key_but_these`]).
    const LEFT_UNBOUND: &[(&str, &str)] = &[
        // They need what the canvas has no dialog to ask for yet (`crate::menu::NEEDS_INPUT`): a
        // key would do nothing. An agent passes it in the command's parameters.
        ("object.batchRename", "needs the new names"),
        ("object.moveToPage", "needs the page to move to"),
        ("vector.offset", "needs the offset"),
        ("file.colorProfile", "needs the colour profile"),
        ("pref.nudge", "needs the nudge amounts"),
        // It never applies on the canvas, which saves its file as it is edited.
        ("file.revert", "never applies on the canvas"),
        // Boolean groups ▸ Flatten runs Flatten itself (the engine's vector.flatten), whose keys
        // are ⌘E and ⌥⇧F.
        ("bool.flatten", "is Flatten, ⌘E"),
        // Settings, set once rather than used as actions, as in Figma: a stray chord would turn
        // off the number keys' opacity or the hover highlight without the user seeing why.
        ("pref.keepTool", "a preference"),
        ("pref.highlightHover", "a preference"),
        ("pref.renameDuplicates", "a preference"),
        ("pref.showDimensions", "a preference"),
        ("pref.numberKeysOpacity", "a preference"),
        ("view.outlinesHiddenLayers", "an option of outline mode"),
        ("view.outlinesObjectBounds", "an option of outline mode"),
    ];

    /// Every command the keymap editor offers on the canvas: FigCraft's shortcuts' and its menus'
    /// commands that the canvas offers ([`is_offered`]), as `board::unbound_rows` lists them.
    fn offered() -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = KEYS
            .iter()
            .map(|binding| binding.id)
            .chain(figcraft_engine::catalog::items().map(|item| item.id))
            .filter(|id| is_offered(id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The user's ask: every command has a key by default, but those that shouldn't
    /// ([`LEFT_UNBOUND`], and the commands the canvas doesn't offer at all: agent-only, Dev Mode,
    /// the prototype player). A FigCraft bump that adds a command fails here until it gets a key
    /// or a reason.
    #[test]
    fn every_offered_command_has_a_key_but_these() {
        let bound: Vec<&str> = key_bindings().iter().map(|binding| binding.id).collect();
        let offered = offered();
        let missing: Vec<&str> = offered
            .iter()
            .copied()
            .filter(|id| !bound.contains(id))
            .filter(|id| LEFT_UNBOUND.iter().all(|(unbound, _)| unbound != id))
            .collect();
        assert!(missing.is_empty(), "no key: {missing:?}");
        for (id, why) in LEFT_UNBOUND {
            assert!(offered.contains(id), "{id} is not offered ({why})");
            assert!(!bound.contains(id), "{id} has a key ({why})");
        }
        // The prototype player's and Dev Mode's commands stay off the keyboard.
        for id in NOT_OFFERED {
            assert!(!bound.contains(id), "{id} has a key");
        }
        // What needs a dialog gets no key of Sawit's: text.link keeps FigCraft's while text is
        // edited, and they do nothing until the dialog comes.
        for id in crate::menu::NEEDS_INPUT {
            assert!(SAWIT_KEYS.iter().all(|key| key.id != *id), "{id}");
        }
    }

    /// Sawit's keys are keys of their own: none is FigCraft's (for any command, in any context),
    /// one the app keeps, or another of Sawit's, and each is a key FigCraft can read. Each goes to
    /// a command the canvas offers that has no FigCraft key the canvas gets.
    #[test]
    fn sawits_keys_clash_with_no_other_key() {
        let figcraft: Vec<String> = KEYS
            .iter()
            .flat_map(|binding| binding.keys(Platform::Mac))
            .filter_map(|spec| binding_keystroke(spec))
            .collect();
        let mut seen: Vec<String> = Vec::new();
        for key in SAWIT_KEYS {
            assert!(is_offered(key.id), "{} is not offered", key.id);
            assert!(
                figcraft_engine::find_command(key.id).is_some() || crate::policy::is_ui(key.id),
                "{} is no command",
                key.id
            );
            let theirs = KEYS
                .iter()
                .filter(|binding| binding.id == key.id)
                .flat_map(|binding| binding.keys(Platform::Mac));
            for spec in theirs {
                assert!(owned_by_sawit(spec), "{} has FigCraft's {spec}", key.id);
            }
            assert!(!key.keys.is_empty(), "{}", key.id);
            for spec in key.keys {
                let keystroke = binding_keystroke(spec)
                    .unwrap_or_else(|| panic!("{} can't read {spec}", key.id));
                assert!(
                    !figcraft.contains(&keystroke),
                    "{spec} ({}) is FigCraft's",
                    key.id
                );
                let gpui = gpui_keystroke(spec).unwrap();
                assert!(!SAWIT_WINS.contains(&gpui.as_str()), "{spec} is Sawit's");
                assert!(
                    !SAWIT_WINS_UNLESS_EDITING.contains(&gpui.as_str()),
                    "{spec}"
                );
                assert!(!seen.contains(&keystroke), "{spec} ({}) twice", key.id);
                seen.push(keystroke);
            }
        }
        // They are the canvas's key bindings, and the sheet lists them.
        let bindings = key_bindings();
        for key in SAWIT_KEYS {
            let keystroke = binding_keystroke(key.keys[0]).unwrap();
            assert!(
                bindings.iter().any(|b| b.id == key.id
                    && b.keystroke == keystroke
                    && b.context == CANVAS_CONTEXT),
                "{} isn't bound",
                key.id
            );
            assert!(row_of(key.id).is_some(), "{} isn't on the sheet", key.id);
        }
        // The family keys read as the module's docs write them.
        let shown = |id| row_of(id).map(|row| row.keys);
        assert_eq!(shown("tool.polygon").as_deref(), Some("⇧6"));
        assert_eq!(shown("tool.star").as_deref(), Some("⇧8"));
        assert_eq!(shown("edit.copyAsSvg").as_deref(), Some("⌥⇧⌘C"));
        assert_eq!(shown("select.sameFill").as_deref(), Some("⌃⇧F"));
        assert_eq!(
            row_of("select.sameFill").map(|row| row.label).as_deref(),
            Some("Select all with same fill")
        );
    }

    /// No two commands share a key in one context: a key runs one command.
    #[test]
    fn no_key_runs_two_commands() {
        let bindings = key_bindings();
        for (i, a) in bindings.iter().enumerate() {
            for b in &bindings[i + 1..] {
                assert!(
                    !(a.keystroke == b.keystroke && a.context == b.context && a.id != b.id),
                    "{} runs {} and {} in {}",
                    a.keystroke,
                    a.id,
                    b.id,
                    a.context
                );
            }
        }
    }
}
