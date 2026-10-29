//! The menus as plain data: the ≡ main menu, built from FigCraft's catalog under Sawit's menu
//! policy, and the context menu the spec defines. The view in `crates/app/src/board/menus.rs`
//! turns the [`MenuNode`]s into popup menus and runs the `id` of the item picked, and the tests
//! here read the same nodes.
//!
//! The main menu is [`figcraft_engine::catalog::MENU`] in its own order, less what the policy
//! ([`crate::policy`]) hides:
//! - A menu path with a slash (`Edit/Copy as`) is a submenu of the menu before the slash, placed
//!   where its first shown item is.
//! - A separator that would lead, trail or repeat once the hidden items are gone is dropped, and
//!   so is a submenu with nothing left in it (View ▸ Theme).
//! - Only [`TOP_MENUS`] are shown: Plugins, Libraries and Tools never are. Help keeps one item,
//!   Keyboard shortcuts, because the policy hides the others.
//!
//! An item is enabled when the engine has its command or Sawit carries it out itself
//! ([`crate::policy::UI_COMMANDS`]). It is checked, or unchecked, when it is one of the View
//! toggles ([`figcraft_engine::cmd::view::checked`]).

use figcraft_engine::Session;
use figcraft_engine::catalog::{MENU, SEP};
use figcraft_engine::cmd::find_command;
use figcraft_engine::cmd::view::checked;
use figcraft_engine::keymap::{self, Platform};

use crate::policy::{is_hidden, is_ui};
use crate::shortcuts::owned_by_sawit;

/// One entry of a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuNode {
    /// A command. Picking it runs `id`.
    Item {
        id: &'static str,
        label: String,
        /// The Mac keys of the command, as a menu writes them (`⇧⌘K`). `None` when it has none, or
        /// when Sawit keeps them (see [`shortcut_label`]).
        shortcut: Option<String>,
        /// Whether the item shows a check mark, and if so whether it is on. `None` for a command
        /// that is not a toggle.
        checked: Option<bool>,
        /// Whether the command can run: the engine knows it, or Sawit carries it out itself.
        enabled: bool,
    },
    /// A menu inside the menu: its `children` open beside it.
    Submenu {
        label: String,
        children: Vec<MenuNode>,
    },
    /// A line between two groups of items.
    Separator,
}

/// The menus of the main menu, in the order they are shown.
pub const TOP_MENUS: &[&str] = &[
    "File",
    "Edit",
    "View",
    "Object",
    "Text",
    "Arrange",
    "Vector",
    "Preferences",
    "Help",
];

/// The main menu: one [`MenuNode::Submenu`] for each of [`TOP_MENUS`], built from the catalog
/// under the policy as the module's docs describe. Checks follow the open file's view, so build
/// it again when that changes.
pub fn main_menu(s: &Session) -> Vec<MenuNode> {
    TOP_MENUS
        .iter()
        .filter_map(|top| submenu(s, top, top))
        .collect()
}

/// The context menu: the spec's list for the layers selected in `s`, or for the empty canvas when
/// none is. Show/Hide and Lock/Unlock say what the engine's toggles would do to the selection.
pub fn context_menu(s: &Session) -> Vec<MenuNode> {
    let Some((all_shown, all_locked)) = selection_state(s) else {
        return commands(s, ON_EMPTY_CANVAS);
    };
    let groups = [
        commands(
            s,
            &[
                ("edit.cut", "Cut"),
                ("edit.copy", "Copy"),
                ("edit.paste", "Paste here"),
                ("edit.duplicate", "Duplicate"),
                ("edit.delete", "Delete"),
            ],
        ),
        commands(
            s,
            &[
                ("arrange.front", "Bring to front"),
                ("arrange.forward", "Bring forward"),
                ("arrange.backward", "Send backward"),
                ("arrange.back", "Send to back"),
            ],
        ),
        commands(
            s,
            &[
                ("object.group", "Group"),
                ("object.ungroup", "Ungroup"),
                ("object.frameSelection", "Frame selection"),
                ("layout.add", "Add auto layout"),
            ],
        ),
        commands(
            s,
            &[
                ("object.flipH", "Flip horizontal"),
                ("object.flipV", "Flip vertical"),
            ],
        ),
        commands(
            s,
            &[
                (
                    "object.toggleVisible",
                    if all_shown { "Hide" } else { "Show" },
                ),
                (
                    "object.toggleLock",
                    if all_locked { "Unlock" } else { "Lock" },
                ),
            ],
        ),
        vec![MenuNode::Submenu {
            label: "Copy as".into(),
            children: commands(
                s,
                &[
                    ("edit.copyAsText", "Text"),
                    ("edit.copyAsCss", "CSS"),
                    ("edit.copyAsSvg", "SVG"),
                    ("edit.copyAsPng", "PNG"),
                ],
            ),
        }],
        commands(s, &[("select.all", "Select all")]),
    ];
    let mut menu = Vec::new();
    for group in groups {
        if !menu.is_empty() {
            menu.push(MenuNode::Separator);
        }
        menu.extend(group);
    }
    menu
}

/// The keys of command `id` as a Mac menu writes them: the first of its bindings in FigCraft's
/// keymap, `⇧⌘K` for Shift+Cmd+K. `None` when the command has no keys, or when its keys are
/// Sawit's while the canvas has the keyboard (⌘W for closing the tab: the policy lists them in
/// [`crate::policy::SAWIT_WINS`]), because the canvas never gets them.
pub fn shortcut_label(id: &str) -> Option<String> {
    let keys = keymap::primary(id, Platform::Mac)?;
    (!owned_by_sawit(keys)).then(|| keymap::display(keys, Platform::Mac))
}

/// The empty canvas's context menu, the spec's four items.
const ON_EMPTY_CANVAS: &[(&str, &str)] = &[
    ("edit.paste", "Paste here"),
    ("select.all", "Select all"),
    ("view.zoomFit", "Zoom to fit"),
    ("view.toggleUi", "Show/Hide UI"),
];

/// Whether menu item `id` can run: the engine has the command, or Sawit carries it out itself.
fn available(id: &str) -> bool {
    find_command(id).is_some() || is_ui(id)
}

/// An item for command `id` called `label`.
fn item(s: &Session, id: &'static str, label: &str) -> MenuNode {
    MenuNode::Item {
        id,
        label: label.to_string(),
        shortcut: shortcut_label(id),
        checked: checked(s, id),
        enabled: available(id),
    }
}

/// An item for each of the (id, label) pairs `entries`.
fn commands(s: &Session, entries: &[(&'static str, &str)]) -> Vec<MenuNode> {
    entries
        .iter()
        .map(|&(id, label)| item(s, id, label))
        .collect()
}

/// Whether every selected layer is shown, and whether every one is locked; `None` when no layer
/// is selected. These pick the words of the context menu: they are the engine's own rule for
/// `object.toggleVisible` and `object.toggleLock`, which hide and unlock then, and show and lock
/// otherwise.
fn selection_state(s: &Session) -> Option<(bool, bool)> {
    let ds = s.ds().ok()?;
    let layers: Vec<_> = ds
        .selection
        .iter()
        .filter_map(|id| ds.doc.node(*id))
        .collect();
    if layers.is_empty() {
        return None;
    }
    Some((
        layers.iter().all(|layer| layer.visible),
        layers.iter().all(|layer| layer.locked),
    ))
}

/// The submenu `label` of the catalog menu `path` (`Edit`, `Edit/Copy as`): the shown entries of
/// that path and, where the first of each lies, the submenus below it. `None` when nothing in it
/// is shown.
fn submenu(s: &Session, path: &str, label: &str) -> Option<MenuNode> {
    let mut children = Vec::new();
    let mut nested: Vec<&str> = Vec::new();
    for entry in MENU
        .iter()
        .filter(|entry| entry.label == SEP || !is_hidden(entry.id))
    {
        if entry.menu == path {
            children.push(if entry.label == SEP {
                MenuNode::Separator
            } else {
                item(s, entry.id, entry.label)
            });
        } else if let Some(name) = child_menu(path, entry.menu)
            && !nested.contains(&name)
        {
            nested.push(name);
            children.extend(submenu(s, &format!("{path}/{name}"), name));
        }
    }
    let children = collapse(children);
    (!children.is_empty()).then(|| MenuNode::Submenu {
        label: label.to_string(),
        children,
    })
}

/// The submenu of `path` that the catalog menu `menu` is in or below: `Copy as` for `Edit/Copy
/// as` under `Edit`. `None` for `path` itself and for a menu that is not below it.
fn child_menu<'a>(path: &str, menu: &'a str) -> Option<&'a str> {
    menu.strip_prefix(path)?
        .strip_prefix('/')?
        .split('/')
        .next()
}

/// `nodes` without the separators that lead, trail or repeat.
fn collapse(nodes: Vec<MenuNode>) -> Vec<MenuNode> {
    let mut kept: Vec<MenuNode> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let repeats =
            node == MenuNode::Separator && matches!(kept.last(), None | Some(MenuNode::Separator));
        if !repeats {
            kept.push(node);
        }
    }
    if kept.last() == Some(&MenuNode::Separator) {
        kept.pop();
    }
    kept
}

#[cfg(test)]
mod tests {
    use figcraft_doc::NodeId;
    use serde_json::json;

    use super::*;
    use crate::BoardDoc;
    use crate::actions::all_actions;
    use crate::shortcuts::sheet;

    /// A new, empty board: nothing selected.
    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// Makes a sticky at `x` on `board`, which selects it, as `board.sticky` selects what it makes.
    fn sticky(board: &mut BoardDoc, x: i32) -> NodeId {
        let made = board
            .execute("board.sticky", &json!({"x": x, "y": 0, "text": "hi"}))
            .unwrap();
        NodeId(made["id"].as_u64().unwrap())
    }

    /// A new board with one sticky on it, selected.
    fn board_with_sticky() -> BoardDoc {
        let mut board = board();
        sticky(&mut board, 0);
        board
    }

    /// Every item in `nodes`, however deep, as (id, label), in menu order.
    fn items(nodes: &[MenuNode]) -> Vec<(&'static str, &str)> {
        let mut found = Vec::new();
        for node in nodes {
            match node {
                MenuNode::Item { id, label, .. } => found.push((*id, label.as_str())),
                MenuNode::Submenu { children, .. } => found.extend(items(children)),
                MenuNode::Separator => {}
            }
        }
        found
    }

    /// The ids of `items(nodes)`.
    fn ids(nodes: &[MenuNode]) -> Vec<&'static str> {
        items(nodes).into_iter().map(|(id, _)| id).collect()
    }

    /// The children of the submenu called `label` among `nodes`.
    fn children_of<'a>(nodes: &'a [MenuNode], label: &str) -> &'a [MenuNode] {
        nodes
            .iter()
            .find_map(|node| match node {
                MenuNode::Submenu {
                    label: name,
                    children,
                } if name == label => Some(children.as_slice()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no submenu called {label} in {nodes:?}"))
    }

    /// The labels of the submenus directly in `nodes`.
    fn submenu_labels(nodes: &[MenuNode]) -> Vec<&str> {
        nodes
            .iter()
            .filter_map(|node| match node {
                MenuNode::Submenu { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The item for `id`, wherever it sits in `nodes`.
    fn item<'a>(nodes: &'a [MenuNode], id: &str) -> &'a MenuNode {
        fn find<'a>(nodes: &'a [MenuNode], wanted: &str) -> Option<&'a MenuNode> {
            nodes.iter().find_map(|node| match node {
                MenuNode::Item { id, .. } if *id == wanted => Some(node),
                MenuNode::Submenu { children, .. } => find(children, wanted),
                _ => None,
            })
        }
        find(nodes, id).unwrap_or_else(|| panic!("no item {id} in {nodes:?}"))
    }

    /// The label of the item for `id`.
    fn label_of<'a>(nodes: &'a [MenuNode], id: &str) -> &'a str {
        match item(nodes, id) {
            MenuNode::Item { label, .. } => label,
            _ => unreachable!(),
        }
    }

    /// The shortcut of the item for `id`.
    fn shortcut_of<'a>(nodes: &'a [MenuNode], id: &str) -> Option<&'a str> {
        match item(nodes, id) {
            MenuNode::Item { shortcut, .. } => shortcut.as_deref(),
            _ => unreachable!(),
        }
    }

    /// The nodes as a list of labels, with `-` for a separator and `Name ▸` for a submenu (its
    /// children are not listed).
    fn outline(nodes: &[MenuNode]) -> Vec<String> {
        nodes
            .iter()
            .map(|node| match node {
                MenuNode::Item { label, .. } => label.clone(),
                MenuNode::Submenu { label, .. } => format!("{label} ▸"),
                MenuNode::Separator => "-".into(),
            })
            .collect()
    }

    /// `nodes` and every list of nodes inside them, the outermost first.
    fn lists(nodes: &[MenuNode]) -> Vec<&[MenuNode]> {
        let mut found = vec![nodes];
        for node in nodes {
            if let MenuNode::Submenu { children, .. } = node {
                found.extend(lists(children));
            }
        }
        found
    }

    fn assert_none_hidden(source: &str, ids: Vec<&str>) {
        assert!(!ids.is_empty(), "{source} lists no command at all");
        for id in ids {
            assert!(!is_hidden(id), "{source} lists the hidden command {id}");
        }
    }

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
        assert_eq!(checked(&board, "view.rulers"), Some(false));
        board.execute("view.rulers", &json!({})).unwrap();
        assert_eq!(checked(&board, "view.rulers"), Some(true));
        board.execute("view.rulers", &json!({})).unwrap();
        assert_eq!(checked(&board, "view.rulers"), Some(false));
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
                    let twice = list.windows(2).any(|pair| {
                        pair[0] == MenuNode::Separator && pair[1] == MenuNode::Separator
                    });
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
        let menu = main_menu(&board().session);
        assert_eq!(
            outline(children_of(&menu, "File")),
            [
                "Import from file…",
                "Place image…",
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

    #[test]
    fn items_are_enabled_when_the_engine_or_sawit_runs_them() {
        assert!(available("edit.duplicate"));
        // Sawit carries these out itself, so the engine has no command for them.
        assert!(available("file.export"));
        assert!(available("edit.copyAsPng"));
        assert!(!available("no.such"));
        assert!(!available(""));
        // Every item the menus show can run.
        let selected = board_with_sticky();
        for menu in [
            main_menu(&selected.session),
            context_menu(&selected.session),
            context_menu(&board().session),
        ] {
            for node in lists(&menu).into_iter().flatten() {
                if let MenuNode::Item { id, enabled, .. } = node {
                    assert!(enabled, "{id} is shown disabled");
                }
            }
        }
    }

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
        // Sawit owns ⌘W, ⌘J and ⌃⌘M, so the canvas has nothing to show for them.
        assert_eq!(shortcut_label("file.close"), None);
        assert_eq!(shortcut_label("vector.join"), None);
        assert_eq!(shortcut_label("object.mask"), None);
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
        assert_eq!(shortcut_of(&menu, "vector.join"), None);
        assert_eq!(shortcut_of(&menu, "file.revert"), None);
    }

    #[test]
    fn context_menu_on_selection() {
        let board = board_with_sticky();
        let menu = context_menu(&board.session);
        let shown = ids(&menu);
        for id in ["edit.delete", "arrange.front", "object.toggleVisible"] {
            assert!(shown.contains(&id), "{id} is not in {shown:?}");
        }
        assert_eq!(label_of(&menu, "object.toggleVisible"), "Hide");
        assert_eq!(label_of(&menu, "object.toggleLock"), "Lock");
        assert_eq!(label_of(&menu, "edit.paste"), "Paste here");
        // The spec's list, in its order.
        assert_eq!(
            shown,
            [
                "edit.cut",
                "edit.copy",
                "edit.paste",
                "edit.duplicate",
                "edit.delete",
                "arrange.front",
                "arrange.forward",
                "arrange.backward",
                "arrange.back",
                "object.group",
                "object.ungroup",
                "object.frameSelection",
                "layout.add",
                "object.flipH",
                "object.flipV",
                "object.toggleVisible",
                "object.toggleLock",
                "edit.copyAsText",
                "edit.copyAsCss",
                "edit.copyAsSvg",
                "edit.copyAsPng",
                "select.all",
            ]
        );
        // Copy as is a submenu of Text, CSS, SVG and PNG.
        assert_eq!(
            items(children_of(&menu, "Copy as")),
            [
                ("edit.copyAsText", "Text"),
                ("edit.copyAsCss", "CSS"),
                ("edit.copyAsSvg", "SVG"),
                ("edit.copyAsPng", "PNG"),
            ]
        );
        // The spec's seven groups, with a line between each.
        let lines = outline(&menu).iter().filter(|label| *label == "-").count();
        assert_eq!(lines, 6);
    }

    #[test]
    fn context_menu_on_empty_canvas() {
        let menu = context_menu(&board().session);
        assert_eq!(menu.len(), 4, "{menu:?}");
        assert_eq!(
            items(&menu),
            [
                ("edit.paste", "Paste here"),
                ("select.all", "Select all"),
                ("view.zoomFit", "Zoom to fit"),
                ("view.toggleUi", "Show/Hide UI"),
            ]
        );
    }

    #[test]
    fn context_menu_items_show_their_shortcuts() {
        let selected = board_with_sticky();
        let menu = context_menu(&selected.session);
        assert_eq!(shortcut_of(&menu, "edit.duplicate"), Some("⌘D"));
        assert_eq!(shortcut_of(&menu, "edit.delete"), Some("⌫"));
        assert_eq!(shortcut_of(&menu, "object.toggleVisible"), Some("⇧⌘H"));
        let empty = context_menu(&board().session);
        assert_eq!(shortcut_of(&empty, "view.toggleUi"), Some("⌘\\"));
        assert_eq!(shortcut_of(&empty, "view.zoomFit"), Some("⇧1"));
    }

    #[test]
    fn context_menu_labels_follow_the_selection() {
        let mut board = board();
        let a = sticky(&mut board, 0);
        let b = sticky(&mut board, 300);
        let labels = |board: &BoardDoc| {
            let menu = context_menu(&board.session);
            (
                label_of(&menu, "object.toggleVisible").to_string(),
                label_of(&menu, "object.toggleLock").to_string(),
            )
        };
        let select = |board: &mut BoardDoc, ids: &[NodeId]| {
            board.execute("select.set", &json!({"ids": ids})).unwrap();
        };

        // Both shown and unlocked: the menu hides and locks.
        select(&mut board, &[a, b]);
        assert_eq!(labels(&board), ("Hide".into(), "Lock".into()));

        // One hidden: the engine's toggle shows every layer then, so the menu says Show.
        let params = json!({"ids": [a], "visible": false});
        board.execute("object.toggleVisible", &params).unwrap();
        select(&mut board, &[a, b]);
        assert_eq!(labels(&board).0, "Show");
        select(&mut board, &[a]);
        assert_eq!(labels(&board).0, "Show");
        select(&mut board, &[b]);
        assert_eq!(labels(&board).0, "Hide");

        // One locked: the engine's toggle locks every layer then, so the menu says Lock. Locking
        // deselects, and the layers panel can still select a locked layer.
        let params = json!({"ids": [a], "locked": true});
        board.execute("object.toggleLock", &params).unwrap();
        select(&mut board, &[a, b]);
        assert_eq!(labels(&board).1, "Lock");
        select(&mut board, &[a]);
        assert_eq!(labels(&board).1, "Unlock");
        let params = json!({"ids": [b], "locked": true});
        board.execute("object.toggleLock", &params).unwrap();
        select(&mut board, &[a, b]);
        assert_eq!(labels(&board).1, "Unlock");
    }
}
