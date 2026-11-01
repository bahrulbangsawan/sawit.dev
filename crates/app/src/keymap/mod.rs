//! Key bindings: the app's defaults, which each feature binds in code at startup, under the user's
//! keymap file ([`file`]), and the keymap editor that shows and changes them ([`editor`]).
//!
//! [`init`] runs after every feature has bound its keys: what is bound then is the defaults. They
//! and the user's bindings are bound again whenever the file changes, from the editor or on disk,
//! the user's last and with a stronger [`KeyBindingMetaIndex`]: a `null` in the file turns off a
//! default, and a default's `null` (the canvas lets its keys through that way) never turns off a
//! user's binding.
//!
//! Every feature's shortcuts are GPUI bindings, so this covers them all: the workspace, the panels,
//! the editors, the terminal, the inputs and lists of the component library, and the native
//! canvas, whose FigCraft shortcuts are `board::Command` bindings (`crate::board::bindings`).
//! The menus, tooltips and hints read the keymap as it is now.

pub mod control;
pub mod editor;
pub mod file;
pub mod names;
mod recorder;
mod rows;
mod store;

pub(crate) use recorder::is_recorder;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use gpui_kit::*;

use file::{Location, UserKeymap};
use store::{layout_defaults, mtime};

actions!(
    keymap,
    [
        /// Opens the keymap editor.
        OpenKeymapEditor,
        /// Opens the keymap file in an editor tab.
        OpenKeymapFile
    ]
);

/// The user's bindings are the strongest: GPUI ranks a smaller meta first, and reads a binding
/// without one as the user's too.
pub const USER: KeyBindingMetaIndex = KeyBindingMetaIndex(0);
/// The app's defaults.
pub const DEFAULT: KeyBindingMetaIndex = KeyBindingMetaIndex(3);

/// How often the keymap file is checked for changes made outside the editor.
const FILE_POLL: Duration = Duration::from_secs(1);

/// What a new keymap file says, before the editor writes the first binding to it.
const NEW_FILE: &str = "// Sawit's key bindings, over the defaults. Open the keymap editor (⌘K ⌘S)
// to see every action and its keys, or write bindings here, as in Zed:
//
// [
//   {
//     \"context\": \"Workspace\",
//     \"bindings\": {
//       \"cmd-shift-w\": \"workspace::CloseAllTabs\",
//       \"cmd-k e\": null
//     }
//   }
// ]
[]
";

/// Where a row of the keymap comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The app's default, `defaults[ix]`.
    Default(usize),
    /// The user's, in the keymap file.
    User(Location),
    /// A default the user turned off, with where the file does it.
    Removed { default: usize, unbind: Location },
    /// An action nothing binds.
    Unbound,
}

/// One row of the keymap: a binding, or an action with none.
#[derive(Clone, Debug)]
pub struct Row {
    pub source: Source,
    /// The action's name: `workspace::CloseActiveTab`, `zed::NoAction` for keys turned off.
    pub action: SharedString,
    /// What the row calls it: [`names::humanize_action`], or the canvas command's title.
    pub label: SharedString,
    /// The action's arguments as JSON, when known.
    pub args: Option<SharedString>,
    /// As the keymap file writes them (`cmd-k e`); empty for an unbound action.
    pub keystrokes: SharedString,
    /// As a Mac menu writes them (`⌘K E`).
    pub display: SharedString,
    pub context: Option<SharedString>,
}

pub struct KeymapStore {
    path: PathBuf,
    /// What every feature bound in code, as [`init`] found it, each with [`DEFAULT`]: written
    /// for a US keyboard layout.
    sources: Vec<KeyBinding>,
    /// The defaults as bound: `sources` with their keys where the keyboard layout types them
    /// ([`relocated`]), in the same order.
    defaults: Vec<KeyBinding>,
    /// The file as last read without error.
    user: UserKeymap,
    /// The file's bindings that load, each with its place in the file.
    loaded: Vec<(Location, KeyBinding)>,
    /// Why the file can't be read, if it can't: its bindings stay as they were until it can.
    file_error: Option<String>,
    /// The bindings in the file that don't load, and why.
    binding_errors: Vec<String>,
    mtime: Option<SystemTime>,
}

struct GlobalKeymap(Entity<KeymapStore>);
impl Global for GlobalKeymap {}

/// Takes what is bound now as the defaults, binds the user's file over them, and watches it. Runs
/// once at startup, after every feature has bound its keys.
pub fn init(cx: &mut App) {
    init_at(
        sawit_core::instances::AppPaths::default_macos().keymap_file(),
        cx,
    );
    let store = store(cx).downgrade();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(FILE_POLL).await;
            let Ok(path) = store.read_with(cx, |store, _| store.path.clone()) else {
                return;
            };
            let now = cx
                .background_executor()
                .spawn(async move { mtime(&path) })
                .await;
            let watched = store.update(cx, |store, cx| {
                if now != store.mtime {
                    store.reload(cx);
                }
            });
            if watched.is_err() {
                return;
            }
        }
    })
    .detach();
    // Another keyboard layout may type the defaults' keys elsewhere.
    let store = self::store(cx).downgrade();
    cx.on_keyboard_layout_change(move |cx| {
        store.update(cx, |store, cx| store.relayout(cx)).ok();
    })
    .detach();
}

/// [`init`] with the keymap file at `path`, without watching it (tests).
pub fn init_at(path: PathBuf, cx: &mut App) {
    cx.bind_keys(editor::bindings());
    let sources: Vec<KeyBinding> = cx
        .key_bindings()
        .borrow()
        .bindings()
        .map(|b| b.clone().with_meta(DEFAULT))
        .collect();
    let defaults = layout_defaults(&sources, cx);
    let store = cx.new(|_| KeymapStore {
        path,
        sources,
        defaults,
        user: UserKeymap::default(),
        loaded: Vec::new(),
        file_error: None,
        binding_errors: Vec::new(),
        mtime: None,
    });
    cx.set_global(GlobalKeymap(store.clone()));
    store.update(cx, |store, cx| store.reload(cx));
}

pub fn store(cx: &App) -> Entity<KeymapStore> {
    cx.global::<GlobalKeymap>().0.clone()
}

/// The keys that run `action` with `context` focused, as the keymap is now, as a Mac menu writes
/// them (`⌘K E`): what a hint, a menu or a tooltip shows for it. Empty when no key runs it there.
pub fn keys_for(action: &dyn Action, context: &str, window: &Window) -> SharedString {
    let context = KeyContext::parse(context).unwrap_or_default();
    window
        .highest_precedence_binding_for_action_in_context(action, context)
        .map(|binding| names::display(binding.keystrokes()).into())
        .unwrap_or_default()
}

/// [`keys_for`] with the focus deeper, in `path`, the contexts from the outermost in (`["GitCommitMessage",
/// "Input"]`): for a binding whose context is a path (`GitCommitMessage > Input`), which no
/// single context matches. As GPUI's own lookup, a binding counts where it matches and no other
/// binding of its keys runs first.
pub fn keys_for_path(action: &dyn Action, path: &[&str], cx: &App) -> SharedString {
    let stack: Vec<KeyContext> = path
        .iter()
        .map(|context| KeyContext::parse(context).unwrap_or_default())
        .collect();
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    keymap
        .bindings_for_action(action)
        .rev()
        .find(|binding| {
            let (found, _) = keymap.bindings_for_input(binding.keystrokes(), &stack);
            found
                .first()
                .is_some_and(|first| first.action().partial_eq(binding.action()))
        })
        .map(|binding| names::display(binding.keystrokes()).into())
        .unwrap_or_default()
}

/// Actions that are no command: the keymap's own markers.
fn is_internal(name: &str) -> bool {
    matches!(name, "zed::NoAction" | "zed::Unbind")
}

#[cfg(test)]
mod tests;
