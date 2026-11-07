//! B5: the area keys (⌘T, ⌘U, ⌘B, ⌘E and the rest) in every context. Each is the workspace's
//! everywhere but where a place has its own use for it while it has the keyboard, and only
//! those places: the terminal's ⇧⌘C (Copy Last Output), the file tree's ⌘N and ⌥⌘N (New File,
//! New Folder), the keymap editor's ⌘N and ⌘E (Create
//! Binding, Edit in JSON), and canvas text's ⌘B, ⌘U and ⌥⌘T (bold, underline, centre).

use gpui_kit::{KeybindingKeystroke, TestAppContext};

/// A keystroke written as `ctrl-alt-shift-cmd-key`.
fn written(keystrokes: &[KeybindingKeystroke]) -> String {
    let one = |k: &KeybindingKeystroke| {
        let m = k.modifiers();
        let mut out = String::new();
        for (on, name) in [
            (m.control, "ctrl-"),
            (m.alt, "alt-"),
            (m.shift, "shift-"),
            (m.platform, "cmd-"),
        ] {
            if on {
                out.push_str(name);
            }
        }
        out + k.key()
    };
    keystrokes.iter().map(one).collect::<Vec<_>>().join(" ")
}

const AREA_KEYS: &[&str] = &[
    "cmd-t",
    "shift-cmd-t",
    "alt-cmd-t",
    "cmd-u",
    "shift-cmd-c",
    "cmd-b",
    "shift-cmd-b",
    "cmd-e",
    "cmd-n",
    "shift-cmd-n",
    "alt-cmd-n",
    "alt-tab",
    "alt-shift-tab",
];

/// The places that keep an area key while they have the keyboard: the key, and a word of the
/// context their binding names.
const LOCAL: &[(&str, &str)] = &[
    ("shift-cmd-c", "Terminal"),
    // The canvas's Copy as PNG (it fills the window with ⌃⌘3 or ⇧⌘↩).
    ("shift-cmd-c", "Board"),
    ("cmd-n", "FilesPanel"),
    ("alt-cmd-n", "FilesPanel"),
    ("cmd-n", "KeymapEditor"),
    ("cmd-e", "KeymapEditor"),
    ("cmd-b", "BoardText"),
    ("cmd-u", "BoardText"),
    ("alt-cmd-t", "BoardText"),
];

#[gpui_kit::test]
fn the_area_keys_lose_only_to_their_own_places(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::workspace::init(cx);
        crate::files::init(cx);
        cx.bind_keys(crate::terminal::key_bindings());
        cx.bind_keys(crate::keymap::editor::bindings());
    });
    let mut wrong = Vec::new();
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        for binding in keymap.bindings() {
            let keys = written(binding.keystrokes());
            if !AREA_KEYS.contains(&keys.as_str()) {
                continue;
            }
            let context = binding
                .predicate()
                .map(|p| p.to_string())
                .unwrap_or_default();
            // The workspace's own, and the app-wide New Window.
            if context == "Workspace" || (context.is_empty() && keys == "alt-cmd-n") {
                continue;
            }
            let local = LOCAL
                .iter()
                .any(|(key, place)| *key == keys && context.contains(place));
            if !local {
                wrong.push(format!(
                    "{keys} in {context:?}: {}",
                    binding.action().name()
                ));
            }
        }
    });
    assert!(
        wrong.is_empty(),
        "area keys taken elsewhere:\n{}",
        wrong.join("\n")
    );
}
