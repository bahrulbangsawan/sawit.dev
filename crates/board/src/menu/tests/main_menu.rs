//! The main menu: the catalog under the policy, its submenus, separators and checks.

use super::*;
use crate::actions::all_actions;
use crate::shortcuts::sheet;

#[test]
fn top_menus_follow_policy() {
    let menu = main_menu(&board().session);
    let labels: Vec<&str> = menu
        .iter()
        .map(|node| match node {
            MenuNode::Submenu { label, .. } => label.as_str(),
            other => panic!("a top menu is a submenu, not {other:?}"),
        })
        .collect();
    assert_eq!(labels, TOP_MENUS);
}

#[test]
fn no_hidden_ids_anywhere() {
    let empty = board();
    let selected = board_with_sticky();
    assert_none_hidden("the main menu", ids(&main_menu(&empty.session)));
    assert_none_hidden(
        "the context menu over a selection",
        ids(&context_menu(&selected.session)),
    );
    assert_none_hidden(
        "the context menu on empty canvas",
        ids(&context_menu(&empty.session)),
    );
    assert_none_hidden(
        "the actions",
        all_actions(&empty.session).iter().map(|a| a.id).collect(),
    );
    let sheet = sheet();
    assert_none_hidden(
        "the shortcut sheet",
        sheet
            .iter()
            .flat_map(|group| group.rows.iter().map(|row| row.id))
            .collect(),
    );
    // Closing a tab is ⌘W, which Sawit keeps: the sheet has no row for it.
    let has_cmd_w = sheet
        .iter()
        .flat_map(|group| &group.rows)
        .any(|row| row.keys.split(" or ").any(|keys| keys == "⌘W"));
    assert!(!has_cmd_w, "{sheet:?}");
}

#[test]
fn copy_as_is_a_submenu_of_edit() {
    let menu = main_menu(&board().session);
    let copy_as = children_of(children_of(&menu, "Edit"), "Copy as");
    assert_eq!(copy_as.len(), 4);
    assert_eq!(
        ids(copy_as),
        [
            "edit.copyAsText",
            "edit.copyAsCss",
            "edit.copyAsSvg",
            "edit.copyAsPng"
        ]
    );
    // FigCraft's own menu has a fifth item, a link to the selection. The policy hides it.
    assert!(!ids(&menu).contains(&"edit.copyLink"));
}

#[test]
fn checked_follows_engine() {
    let mut board = board();
    let checked = |board: &BoardDoc, id: &str| match item(&main_menu(&board.session), id) {
        MenuNode::Item { checked, .. } => *checked,
        _ => unreachable!(),
    };
    assert_eq!(checked(&board, "view.outlines"), Some(false));
    board.execute("view.outlines", &json!({})).unwrap();
    assert_eq!(checked(&board, "view.outlines"), Some(true));
    board.execute("view.outlines", &json!({})).unwrap();
    assert_eq!(checked(&board, "view.outlines"), Some(false));
    // A command that is not a toggle shows no check mark at all.
    assert_eq!(checked(&board, "edit.duplicate"), None);
}

#[test]
fn separators_are_collapsed() {
    let empty = board();
    let selected = board_with_sticky();
    let mut lists_checked = 0;
    for session in [&empty.session, &selected.session] {
        for menu in [main_menu(session), context_menu(session)] {
            for list in lists(&menu) {
                lists_checked += 1;
                assert!(!list.is_empty(), "an empty submenu was kept");
                assert_ne!(list.first(), Some(&MenuNode::Separator), "{list:?}");
                assert_ne!(list.last(), Some(&MenuNode::Separator), "{list:?}");
                let twice = list
                    .windows(2)
                    .any(|pair| pair[0] == MenuNode::Separator && pair[1] == MenuNode::Separator);
                assert!(!twice, "two separators in a row in {list:?}");
            }
        }
    }
    // The main menu alone is 19 lists: itself, its 9 top menus and 9 submenus.
    assert!(lists_checked > 30, "checked only {lists_checked} lists");
}

#[test]
fn file_menu_follows_the_catalog_under_the_policy() {
    // The catalog's File menu has 27 entries and 5 separators. The policy hides 15 of the
    // entries, which leaves 2 separators next to each other, and the last entries are shown.
    // Sawit's own Add Routes from Dev Server… follows Place image….
    let menu = main_menu(&board().session);
    assert_eq!(
        outline(children_of(&menu, "File")),
        [
            "Import from file…",
            "Place image…",
            "Add Routes from Dev Server…",
            "-",
            "Save",
            "Save as…",
            "-",
            "Export…",
            "Export frames to PDF…",
            "Export selection as SVG…",
            "-",
            "Color profile…",
            "Revert to saved",
        ]
    );
}

#[test]
fn paths_become_nested_submenus() {
    let menu = main_menu(&board().session);
    let edit = children_of(&menu, "Edit");
    let view = children_of(&menu, "View");
    assert_eq!(submenu_labels(edit), ["Copy as", "Select all with same"]);
    assert_eq!(
        children_of(edit, "Select all with same").len(),
        7,
        "Fill, Stroke, Effect, Text properties, Font, Instance, Properties"
    );
    assert_eq!(
        submenu_labels(children_of(&menu, "Object")),
        ["Boolean groups", "Select layer"]
    );
    assert_eq!(
        submenu_labels(children_of(&menu, "Text")),
        ["Alignment", "Adjust", "Case"]
    );
    // A submenu sits where its first item does: Panels follows Soft-proof sRGB and a line.
    let order = outline(view);
    let outlines = order
        .iter()
        .position(|label| label == "Outlines ▸")
        .unwrap();
    let panels = order.iter().position(|label| label == "Panels ▸").unwrap();
    assert!(outlines < panels, "{order:?}");
    assert_eq!(order[panels - 2], "Soft-proof sRGB");
    assert_eq!(order[panels - 1], "-");
    // A submenu keeps its own separators, and loses the hidden items.
    assert_eq!(
        outline(children_of(view, "Outlines")),
        [
            "Show outlines",
            "Show outlines for selection",
            "-",
            "Include hidden layers",
            "Include object bounds"
        ]
    );
    assert_eq!(
        ids(children_of(view, "Panels")),
        ["view.toggleUi", "view.layers", "view.design"],
        "Reset panel widths, Assets and Prototype are hidden"
    );
}

#[test]
fn a_submenu_with_nothing_shown_is_dropped() {
    // View ▸ Theme holds only the three theme commands, which the policy hides.
    let menu = main_menu(&board().session);
    assert_eq!(
        submenu_labels(children_of(&menu, "View")),
        ["Outlines", "Panels"]
    );
    // The line before Text ▸ Missing fonts, a hidden item, goes with it.
    let text = outline(children_of(&menu, "Text"));
    assert_eq!(text.last().map(String::as_str), Some("Case ▸"), "{text:?}");
}

#[test]
fn help_menu_keeps_only_the_shortcuts_sheet() {
    let menu = main_menu(&board().session);
    assert_eq!(
        items(children_of(&menu, "Help")),
        [("help.shortcuts", "Keyboard shortcuts")]
    );
}

#[test]
fn plugins_libraries_and_tools_never_show() {
    let menu = main_menu(&board().session);
    let shown = ids(&menu);
    // The Tools menu lives in the toolbar, and Actions is the palette, not a menu item.
    for id in [
        "plugins.runLast",
        "libraries.open",
        "tool.move",
        "app.actions",
    ] {
        assert!(!shown.contains(&id), "{id} is in the main menu");
    }
    // Preferences holds the three snapping toggles and the six preferences, less
    // Preferences… itself, which the policy hides.
    assert_eq!(ids(children_of(&menu, "Preferences")).len(), 9);
}
