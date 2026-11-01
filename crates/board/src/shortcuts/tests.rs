//! The sheet's rows and groups, the canvas's key bindings, and Sawit's own keys.

use figcraft_engine::keymap;

use super::bindings::SHIFTED;
use super::sheet::NOT_OFFERED;
use super::*;
use crate::input::gpui_keystroke;
use crate::policy::{SAWIT_WINS, SAWIT_WINS_UNLESS_EDITING, is_hidden};

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
    for key in ["⌘W", "⌘O", "⌃⇥", "⌃⇧⇥", "⌃⌘M", "⌥⌘O", "⌘J", "⇧⌘J"] {
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
            let keystroke =
                binding_keystroke(spec).unwrap_or_else(|| panic!("{} can't read {spec}", key.id));
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
            bindings
                .iter()
                .any(|b| b.id == key.id && b.keystroke == keystroke && b.context == CANVAS_CONTEXT),
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
