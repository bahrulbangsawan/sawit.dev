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
//! ([`crate::policy::UI_COMMANDS`]), and the command applies to what the canvas holds now
//! ([`applies`]): Frame selection, Group and the rest of what acts on the selection are greyed
//! out while nothing is selected, Undo while there is nothing to undo, and the few items that
//! need a dialog the canvas doesn't have yet always are. So are their keys quiet then. It is
//! checked, or unchecked, when it is one of the View toggles
//! ([`figcraft_engine::cmd::view::checked`]). Build the menus again when the selection changes.

use std::collections::HashMap;
use std::sync::LazyLock;

use figcraft_engine::Session;
use figcraft_engine::catalog::{MENU, SEP};
use figcraft_engine::cmd::view::checked;
use figcraft_engine::keymap::{self, Platform};

use crate::policy::is_hidden;
use crate::shortcuts::owned_by_sawit;

mod enabled;
#[cfg(test)]
mod tests;

use enabled::available;
pub use enabled::{NEEDS_INPUT, NEEDS_SELECTION, NEVER_ON_THE_CANVAS, applies};

/// One entry of a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuNode {
    /// A command. Picking it runs `id`.
    Item {
        id: &'static str,
        label: String,
        /// The Mac keys of the command in FigCraft's keymap, as a menu writes them (`⇧⌘K`).
        /// `None` when it has none, or when Sawit keeps them (see [`shortcut_label`]). The app's
        /// menus show the keys the keymap binds now instead.
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
    let mut menu = MAIN_MENU.clone();
    fill(s, &mut menu);
    menu
}

/// The main menu as the catalog and the policy lay it out, the same on every canvas: worked out
/// once, as that walks the whole catalog for each menu and submenu. [`main_menu`] fills in what
/// its items are on a canvas.
static MAIN_MENU: LazyLock<Vec<MenuNode>> = LazyLock::new(|| {
    TOP_MENUS
        .iter()
        .filter_map(|top| submenu(top, top))
        .collect()
});

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

/// The keys of command `id` as a Mac menu writes them: the first of its default keys
/// ([`crate::shortcuts::defaults`]: FigCraft's keymap, then Sawit's), `⇧⌘K` for Shift+Cmd+K.
/// `None` when the command has no keys, or when its keys are Sawit's while the canvas has the
/// keyboard (⌘W for closing the tab: the policy lists them in [`crate::policy::SAWIT_WINS`]),
/// because the canvas never gets them.
pub fn shortcut_label(id: &str) -> Option<String> {
    SHORTCUT_LABELS.get(id).cloned()
}

/// [`shortcut_label`] of every command that has default keys, worked out once: the defaults never
/// change while Sawit runs, and finding the first key of one command walks the keys of them all.
static SHORTCUT_LABELS: LazyLock<HashMap<&'static str, String>> = LazyLock::new(|| {
    let label = |id| {
        let keys = crate::shortcuts::primary(id)?;
        (!owned_by_sawit(keys)).then(|| keymap::display(keys, Platform::Mac))
    };
    crate::shortcuts::defaults()
        .filter_map(|keys| Some((keys.id, label(keys.id)?)))
        .collect()
});

/// The empty canvas's context menu, the spec's four items.
const ON_EMPTY_CANVAS: &[(&str, &str)] = &[
    ("edit.paste", "Paste here"),
    ("select.all", "Select all"),
    ("view.zoomFit", "Zoom to fit"),
    ("view.toggleUi", "Show/Hide UI"),
];

/// An item for command `id` called `label` as the menus lay it out: unchecked and disabled until
/// [`fill`] says what it is on a canvas.
fn item(id: &'static str, label: &str) -> MenuNode {
    MenuNode::Item {
        id,
        label: label.to_string(),
        shortcut: shortcut_label(id),
        checked: None,
        enabled: false,
    }
}

/// An item for each of the (id, label) pairs `entries`, as it is on the canvas `s`.
fn commands(s: &Session, entries: &[(&'static str, &str)]) -> Vec<MenuNode> {
    let mut items: Vec<MenuNode> = entries.iter().map(|&(id, label)| item(id, label)).collect();
    fill(s, &mut items);
    items
}

/// Gives each item among `nodes`, however deep, what it is on the canvas `s`: its check mark, and
/// whether it is enabled, which it is while the command can run and applies to what the canvas
/// holds now ([`applies`]).
fn fill(s: &Session, nodes: &mut [MenuNode]) {
    for node in nodes {
        match node {
            MenuNode::Item {
                id,
                checked: mark,
                enabled,
                ..
            } => {
                *mark = checked(s, id);
                *enabled = available(id) && applies(s, id);
            }
            MenuNode::Submenu { children, .. } => fill(s, children),
            MenuNode::Separator => {}
        }
    }
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
fn submenu(path: &str, label: &str) -> Option<MenuNode> {
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
                item(entry.id, entry.label)
            });
        } else if let Some(name) = child_menu(path, entry.menu)
            && !nested.contains(&name)
        {
            nested.push(name);
            children.extend(submenu(&format!("{path}/{name}"), name));
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
