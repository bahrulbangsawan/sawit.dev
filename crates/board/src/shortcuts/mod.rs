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

use std::sync::LazyLock;

use figcraft_engine::keymap::{Context, Group, KEYS, Platform};

mod bindings;
mod sawit_keys;
mod sheet;

pub(crate) use bindings::owned_by_sawit;
pub use bindings::{
    CANVAS_CONTEXT, KeyBinding, TEXT_CONTEXT, binding_keystroke, key_bindings, unshifted,
};
pub use sawit_keys::{SAWIT_KEYS, SawitKey};
pub use sheet::{ShortcutGroup, ShortcutRow, is_offered, sheet};

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
    // Made once: the keymap and Sawit's keys never change while the app runs, and each title is a
    // search of FigCraft's menus, which every lookup (`primary`, for each toolbar button on
    // every frame) made again for every command it went past.
    static DEFAULTS: LazyLock<Vec<DefaultKeys>> = LazyLock::new(|| {
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
        figcraft.chain(sawits).collect()
    });
    DEFAULTS.iter().copied()
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

#[cfg(test)]
mod tests;
