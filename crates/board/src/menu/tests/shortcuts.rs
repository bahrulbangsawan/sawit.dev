//! The keys a menu item shows.

use super::*;

#[test]
fn shortcut_labels_use_mac_glyphs() {
    assert_eq!(shortcut_label("edit.duplicate").as_deref(), Some("⌘D"));
    assert_eq!(shortcut_label("edit.redo").as_deref(), Some("⇧⌘Z"));
    assert_eq!(shortcut_label("arrange.front").as_deref(), Some("⌥⌘]"));
    assert_eq!(shortcut_label("view.toggleUi").as_deref(), Some("⌘\\"));
    assert_eq!(shortcut_label("help.shortcuts").as_deref(), Some("⌃⇧?"));
    assert_eq!(shortcut_label("edit.delete").as_deref(), Some("⌫"));
    assert_eq!(shortcut_label("tool.section").as_deref(), Some("⇧S"));
    // A command with two keys shows the first, as FigCraft's menus do.
    assert_eq!(shortcut_label("app.actions").as_deref(), Some("⌘K"));
    // Sawit owns ⌘W, so the canvas has nothing to show for it. It owns ⌘J and ⌃⌘M too, and
    // gives Join and Use as mask keys of its own.
    assert_eq!(shortcut_label("file.close"), None);
    assert_eq!(shortcut_label("vector.join").as_deref(), Some("⌃J"));
    assert_eq!(shortcut_label("object.mask").as_deref(), Some("⌃⌥M"));
    // A key Sawit gives a command FigCraft has none for.
    assert_eq!(shortcut_label("tool.star").as_deref(), Some("⇧8"));
    assert_eq!(shortcut_label("view.zoom50").as_deref(), Some("⌥⇧-"));
    // ⌘B and ⇧⌘↩ are Sawit's too, but not while text is edited, so they are shown.
    assert_eq!(shortcut_label("text.bold").as_deref(), Some("⌘B"));
    assert_eq!(shortcut_label("board.nextSticky").as_deref(), Some("⇧⌘↵"));
    // No keys, and no such command.
    assert_eq!(shortcut_label("file.revert"), None);
    assert_eq!(shortcut_label("no.such"), None);
}

#[test]
fn menu_items_carry_their_shortcut_label() {
    let menu = main_menu(&board().session);
    assert_eq!(shortcut_of(&menu, "edit.duplicate"), Some("⌘D"));
    assert_eq!(shortcut_of(&menu, "help.shortcuts"), Some("⌃⇧?"));
    assert_eq!(shortcut_of(&menu, "vector.join"), Some("⌃J"));
    assert_eq!(shortcut_of(&menu, "file.revert"), None);
}
