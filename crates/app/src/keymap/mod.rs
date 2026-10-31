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

pub mod editor;
pub mod file;
pub mod names;
mod recorder;

pub(crate) use recorder::is_recorder;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::*;
use serde_json::Value;

use file::{Entry, Location, UserKeymap};

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

/// The defaults written in `sources` with their keys where the keyboard layout in use types
/// them, and the keymap's display told whether it moves any.
fn layout_defaults(sources: &[KeyBinding], cx: &App) -> Vec<KeyBinding> {
    let moves = cx.keyboard_mapper().get_key_equivalents();
    names::set_layout_moves_keys(moves.is_some_and(|table| !table.is_empty()));
    match moves {
        Some(table) => relocated(sources, |c| table.get(&c).copied(), cx),
        None => sources.to_vec(),
    }
}

/// `sources` with each key that `moves` moves put where the keyboard layout types it, as macOS
/// menus and Zed's default keymap do (Apple's key equivalents, [`names::relocate`]): on a German
/// layout ⇧⌘. (bound `cmd->`) is typed `cmd-:`, and the ⌘< that cycles windows there is no longer
/// the canvas's text size. Bindings that no key of moves stay as they are.
fn relocated(
    sources: &[KeyBinding],
    moves: impl Fn(char) -> Option<char>,
    cx: &App,
) -> Vec<KeyBinding> {
    sources
        .iter()
        .map(|binding| {
            names::relocate(&names::unparse(binding.keystrokes()), &moves)
                .and_then(|keys| {
                    KeyBinding::load(
                        &keys,
                        binding.action().boxed_clone(),
                        binding.predicate(),
                        false,
                        binding.action_input(),
                        cx,
                    )
                    .ok()
                })
                .map(|moved| match binding.meta() {
                    Some(meta) => moved.with_meta(meta),
                    None => moved,
                })
                .unwrap_or_else(|| binding.clone())
        })
        .collect()
}

impl KeymapStore {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Why the file can't be read, then why each of its bindings that doesn't load fails.
    pub fn errors(&self) -> impl Iterator<Item = &str> {
        self.file_error
            .iter()
            .chain(self.binding_errors.iter())
            .map(String::as_str)
    }

    /// Whether the editor may write the file: not while it can't be read, which would lose what
    /// the user is in the middle of writing.
    pub fn can_write(&self) -> bool {
        self.file_error.is_none()
    }

    pub fn defaults(&self) -> &[KeyBinding] {
        &self.defaults
    }

    /// Binds the defaults again where the keyboard layout now in use types their keys.
    pub fn relayout(&mut self, cx: &mut Context<Self>) {
        self.defaults = layout_defaults(&self.sources, cx);
        self.apply(cx);
        cx.notify();
    }

    /// [`Self::relayout`] for a layout that moves keys by `moves` (tests).
    #[cfg(test)]
    fn relayout_with(&mut self, moves: impl Fn(char) -> Option<char>, cx: &mut Context<Self>) {
        self.defaults = relocated(&self.sources, moves, cx);
        self.apply(cx);
        cx.notify();
    }

    /// Reads the file again and binds the keys anew. A file that can't be read keeps the
    /// bindings it had, and says why.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.mtime = mtime(&self.path);
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                self.file_error = Some(format!("Can't read {}: {e}", self.path.display()));
                cx.notify();
                return;
            }
        };
        match UserKeymap::parse(&text) {
            Ok(user) => {
                self.file_error = None;
                self.user = user;
                self.load_user(cx);
                self.apply(cx);
            }
            Err(e) => self.file_error = Some(format!("keymap.json can't be read: {e}")),
        }
        cx.notify();
    }

    /// Builds the bindings of the file, keeping the errors of those that fail.
    fn load_user(&mut self, cx: &App) {
        self.loaded.clear();
        self.binding_errors.clear();
        for (at, context, binding) in self.user.bindings() {
            match load_binding(context, binding, cx) {
                Ok(loaded) => self.loaded.push((at, loaded)),
                Err(e) => self.binding_errors.push(format!(
                    "\"{}\"{}: {e}",
                    binding.keystrokes,
                    context.map(|c| format!(" in {c}")).unwrap_or_default()
                )),
            }
        }
    }

    /// Binds the defaults, then the user's bindings, in place of every binding the app has. The
    /// canvas then lets through the user's chords that start with a key it binds alone, which
    /// would keep that key waiting ([`crate::board::let_through_chords`]).
    fn apply(&self, cx: &mut App) {
        cx.clear_key_bindings();
        cx.bind_keys(self.defaults.iter().cloned());
        cx.bind_keys(self.loaded.iter().map(|(_, binding)| binding.clone()));
        let chords = crate::board::let_through_chords(
            &self.defaults,
            self.loaded.iter().map(|(_, binding)| binding),
            cx,
        );
        cx.bind_keys(chords);
        crate::cli::set_menus(cx);
    }

    /// Writes the file with `change` made to it, then binds the keys anew. The file is read
    /// first when it changed since the store last read it (another program saved it less than
    /// a [`FILE_POLL`] ago): when that changes anything, nothing is written, and the change must
    /// be asked for again against the file as it is now.
    fn write(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut UserKeymap),
    ) -> Result<(), String> {
        if mtime(&self.path) != self.mtime {
            let before = self.user.clone();
            self.reload(cx);
            if self.file_error.is_none() && self.user != before {
                return Err(
                    "keymap.json was changed outside the editor, which has read it again. Make the change again."
                        .into(),
                );
            }
        }
        if let Some(error) = &self.file_error {
            return Err(format!("Fix keymap.json first. {error}"));
        }
        let mut user = self.user.clone();
        change(&mut user);
        write_atomically(&self.path, &user.to_json())
            .map_err(|e| format!("Can't save {}: {e}", self.path.display()))?;
        self.mtime = mtime(&self.path);
        self.user = user;
        self.load_user(cx);
        self.apply(cx);
        cx.notify();
        Ok(())
    }

    /// Adds a binding of `keystrokes` to `action` in `context`, after the others there: it runs
    /// first. Another binding of the same keystrokes stays ([`UserKeymap::add`]).
    pub fn add(
        &mut self,
        context: Option<&str>,
        keystrokes: &str,
        action: &str,
        args: Option<Value>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.write(cx, |user| {
            user.add(context, keystrokes, Entry::action(action, args))
        })
    }

    /// Changes the row `source` to bind `keystrokes` to `action` in `context`. A user's binding
    /// changes in the file; a default is turned off there (`zed::Unbind`) and the new binding
    /// added, as Zed's keymap editor does. No other binding changes, the user's own of the same
    /// keys included.
    pub fn change(
        &mut self,
        source: Source,
        context: Option<&str>,
        keystrokes: &str,
        action: &str,
        args: Option<Value>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let entry = Entry::action(action, args);
        match source {
            Source::User(at) => self.write(cx, |user| {
                user.replace(at, context, keystrokes, entry);
            }),
            Source::Default(ix) => {
                let (default_context, default_keys, default_action) = self.default_parts(ix);
                let same_place = default_context.as_deref() == context.map(str::trim)
                    && Some(default_keys.as_str())
                        == names::normalize_keystrokes(keystrokes).as_deref();
                let same_action = cx
                    .build_action(action, args_of(&entry))
                    .is_ok_and(|built| self.defaults[ix].action().partial_eq(built.as_ref()));
                if same_place && same_action {
                    return Ok(());
                }
                self.write(cx, |user| {
                    // The unbind goes first: the new binding, after it, is not unbound by it
                    // when it has the same keys and action name (a canvas key pointed at
                    // another command).
                    user.add(
                        default_context.as_deref(),
                        &default_keys,
                        Entry::unbind(&default_action),
                    );
                    user.add(context, keystrokes, entry);
                })
            }
            Source::Removed { .. } | Source::Unbound => {
                self.add(context, keystrokes, action, args_of(&entry), cx)
            }
        }
    }

    /// Takes the row `source` out: a user's binding leaves the file, a default is turned off
    /// there, and a removed default comes back.
    pub fn remove(&mut self, source: Source, cx: &mut Context<Self>) -> Result<(), String> {
        match source {
            Source::User(at) | Source::Removed { unbind: at, .. } => self.write(cx, |user| {
                user.remove(at);
            }),
            Source::Default(ix) => {
                let (context, keys, action) = self.default_parts(ix);
                self.write(cx, |user| {
                    user.add(context.as_deref(), &keys, Entry::unbind(&action))
                })
            }
            Source::Unbound => Ok(()),
        }
    }

    /// Where the file turns default `ix` off, if it does: a `null`, or an unbind of the default's
    /// action, on the default's keystrokes, in a context that covers the default's (none, or one
    /// GPUI reads as a superset), as GPUI decides which bindings a later one turns off.
    fn turned_off(&self, ix: usize) -> Option<Location> {
        let default = &self.defaults[ix];
        self.loaded.iter().find_map(|(at, binding)| {
            if binding.keystrokes() != default.keystrokes() {
                return None;
            }
            let covers = match (binding.predicate(), default.predicate()) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(off), Some(on)) => off.is_superset(&on),
            };
            let off = is_no_action(binding.action())
                || (is_unbind(binding.action())
                    && self.user.binding(*at).and_then(|b| b.entry.unbinds())
                        == Some(default.action().name()));
            (covers && off).then_some(*at)
        })
    }

    /// The context, keystrokes and action name of default `ix`, as the file writes them.
    fn default_parts(&self, ix: usize) -> (Option<String>, String, String) {
        let binding = &self.defaults[ix];
        (
            binding.predicate().map(|p| p.to_string()),
            names::unparse(binding.keystrokes()),
            binding.action().name().to_string(),
        )
    }

    /// Whether `row`, as the editor read it, is still a row of the keymap: the place in the file
    /// a user's row came from may hold another binding once the file has changed. An action with
    /// no binding has no place in the file: its row only adds a binding, which stays right
    /// whatever the file holds now.
    pub fn is_current(&self, row: &Row, cx: &App) -> bool {
        if row.source == Source::Unbound {
            return true;
        }
        self.rows(false, cx).iter().any(|r| {
            r.source == row.source
                && r.action == row.action
                && r.args == row.args
                && r.keystrokes == row.keystrokes
                && r.context == row.context
        })
    }

    /// Creates the file, with a few lines on how to write it, if there is none.
    pub fn ensure_file(&self) -> std::io::Result<()> {
        if self.path.exists() {
            return Ok(());
        }
        write_atomically(&self.path, NEW_FILE)
    }

    /// Every row of the keymap: the defaults (those the user turned off as [`Source::Removed`]),
    /// the user's bindings, then, with `unbound`, each action nothing binds. The defaults that
    /// only let keys through to a view (`zed::NoAction`) and the file's unbinds are left out: they
    /// show as the rows they change.
    pub fn rows(&self, unbound: bool, cx: &App) -> Vec<Row> {
        let mut rows = Vec::new();
        for (ix, binding) in self.defaults.iter().enumerate() {
            let action = binding.action();
            if is_no_action(action) || is_unbind(action) {
                continue;
            }
            let source = match self.turned_off(ix) {
                Some(unbind) => Source::Removed {
                    default: ix,
                    unbind,
                },
                None => Source::Default(ix),
            };
            rows.push(row(source, binding));
        }
        for (at, binding) in &self.loaded {
            if is_unbind(binding.action()) {
                continue;
            }
            rows.push(row(Source::User(*at), binding));
        }
        if unbound {
            let bound: std::collections::HashSet<&str> =
                rows.iter().map(|r| r.action.as_ref()).collect();
            let mut extra: Vec<Row> = cx
                .all_action_names()
                .iter()
                .filter(|name| !bound.contains(**name) && !is_internal(name))
                .map(|name| Row {
                    source: Source::Unbound,
                    action: (*name).into(),
                    label: names::humanize_action(name).into(),
                    args: None,
                    keystrokes: SharedString::default(),
                    display: SharedString::default(),
                    context: None,
                })
                .collect();
            extra.extend(crate::board::unbound_rows(&rows));
            rows.extend(extra);
        }
        rows.sort_by(|a, b| {
            a.label
                .cmp(&b.label)
                .then_with(|| a.context.cmp(&b.context))
                .then_with(|| a.keystrokes.cmp(&b.keystrokes))
        });
        rows
    }
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

/// The row of `binding` from `source`.
fn row(source: Source, binding: &KeyBinding) -> Row {
    let action = binding.action();
    let args = binding.action_input();
    let label = crate::board::command_label(action).unwrap_or_else(|| {
        if is_no_action(action) {
            "(keys turned off)".into()
        } else {
            names::humanize_action(action.name())
        }
    });
    Row {
        source,
        action: action.name().into(),
        label: label.into(),
        args,
        keystrokes: names::unparse(binding.keystrokes()).into(),
        display: names::display(binding.keystrokes()).into(),
        context: binding.predicate().map(|p| p.to_string().into()),
    }
}

/// The binding the file's `binding` in `context` makes.
fn load_binding(
    context: Option<&str>,
    binding: &file::Binding,
    cx: &App,
) -> Result<KeyBinding, String> {
    let predicate = context
        .map(|c| KeyBindingContextPredicate::parse(c).map(std::rc::Rc::new))
        .transpose()
        .map_err(|e| format!("the context can't be read: {e}"))?;
    let (action, input): (Box<dyn Action>, _) = match &binding.entry {
        Entry::Disabled => (Box::new(NoAction), None),
        Entry::Action { name, args } => {
            let action = cx
                .build_action(name, args.clone())
                .map_err(|e| e.to_string())?;
            (action, args.as_ref().map(|a| a.to_string().into()))
        }
    };
    let binding = KeyBinding::load(&binding.keystrokes, action, predicate, false, input, cx)
        .map_err(|e| e.to_string())?;
    Ok(binding.with_meta(USER))
}

fn args_of(entry: &Entry) -> Option<Value> {
    match entry {
        Entry::Action { args, .. } => args.clone(),
        Entry::Disabled => None,
    }
}

/// Actions that are no command: the keymap's own markers.
fn is_internal(name: &str) -> bool {
    matches!(name, "zed::NoAction" | "zed::Unbind")
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Writes `text` to `path` through a file beside it, so a crash never leaves half a keymap. A
/// `path` that is a link (to a dotfiles folder) stays one: the file it points to is written.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui_kit::{
        App, Entity, KeyBinding, KeyContext, Keystroke, OperatingSystem, TestAppContext,
    };

    use super::{KeymapStore, Source, init_at, store};

    gpui_kit::actions!(keymap_test, [Close, CloseAll, Other]);

    /// A keymap file of the test's own, removed when the test ends.
    pub(super) struct Scratch(pub(super) PathBuf);

    impl Scratch {
        pub(super) fn new(test: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("sawit-keymap-{}-{test}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }

        pub(super) fn file(&self) -> PathBuf {
            self.0.join("keymap.json")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The app's defaults for the tests: ⌘W closes, ⌘K W closes all, in `Workspace`.
    fn start(cx: &mut TestAppContext, scratch: &Scratch) -> Entity<KeymapStore> {
        cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("cmd-w", Close, Some("Workspace")),
                KeyBinding::new("cmd-k w", CloseAll, Some("Workspace")),
            ]);
            init_at(scratch.file(), cx);
            store(cx)
        })
    }

    /// What `keystroke` runs in the workspace, by action name.
    pub(super) fn runs(cx: &App, keystroke: &str) -> Vec<&'static str> {
        let typed: Vec<Keystroke> = keystroke
            .split(' ')
            .map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).unwrap())
            .collect();
        let contexts = [KeyContext::parse("Workspace").unwrap()];
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, _) = keymap.bindings_for_input(&typed, &contexts);
        bindings.iter().map(|b| b.action().name()).collect()
    }

    fn default_row(store: &Entity<KeymapStore>, cx: &App, action: &str) -> Source {
        store
            .read(cx)
            .rows(false, cx)
            .into_iter()
            .find(|row| row.action.as_ref() == action)
            .map(|row| row.source)
            .unwrap()
    }

    #[gpui_kit::test]
    fn changing_a_default_moves_it_to_the_new_keys(cx: &mut TestAppContext) {
        let scratch = Scratch::new("change");
        let store = start(cx, &scratch);
        cx.update(|cx| {
            let source = default_row(&store, cx, "keymap_test::Close");
            store
                .update(cx, |store, cx| {
                    store.change(
                        source,
                        Some("Workspace"),
                        "cmd-e",
                        "keymap_test::Close",
                        None,
                        cx,
                    )
                })
                .unwrap();
            assert_eq!(runs(cx, "cmd-e"), ["keymap_test::Close"]);
            assert!(runs(cx, "cmd-w").is_empty(), "{:?}", runs(cx, "cmd-w"));
            // The default shows as removed, beside the user's binding.
            let rows = store.read(cx).rows(false, cx);
            let close: Vec<_> = rows
                .iter()
                .filter(|r| r.action.as_ref() == "keymap_test::Close")
                .map(|r| (r.keystrokes.to_string(), r.source))
                .collect();
            assert_eq!(close.len(), 2, "{close:?}");
            assert!(
                close
                    .iter()
                    .any(|(k, s)| k == "cmd-w" && matches!(s, Source::Removed { .. }))
            );
            assert!(
                close
                    .iter()
                    .any(|(k, s)| k == "cmd-e" && matches!(s, Source::User(_)))
            );
        });
        let text = std::fs::read_to_string(scratch.file()).unwrap();
        assert!(text.contains("zed::Unbind"), "{text}");
    }

    #[gpui_kit::test]
    fn a_removed_default_comes_back(cx: &mut TestAppContext) {
        let scratch = Scratch::new("restore");
        let store = start(cx, &scratch);
        cx.update(|cx| {
            let source = default_row(&store, cx, "keymap_test::CloseAll");
            store
                .update(cx, |store, cx| store.remove(source, cx))
                .unwrap();
            assert!(runs(cx, "cmd-k w").is_empty());
            let removed = default_row(&store, cx, "keymap_test::CloseAll");
            assert!(matches!(removed, Source::Removed { .. }), "{removed:?}");
            store
                .update(cx, |store, cx| store.remove(removed, cx))
                .unwrap();
            assert_eq!(runs(cx, "cmd-k w"), ["keymap_test::CloseAll"]);
            assert!(matches!(
                default_row(&store, cx, "keymap_test::CloseAll"),
                Source::Default(_)
            ));
        });
    }

    #[gpui_kit::test]
    fn the_file_written_by_hand_binds_over_the_defaults(cx: &mut TestAppContext) {
        let scratch = Scratch::new("by-hand");
        let store = start(cx, &scratch);
        std::fs::create_dir_all(&scratch.0).unwrap();
        std::fs::write(
            scratch.file(),
            r#"// Mine.
            [{"context": "Workspace", "bindings": {"cmd-w": "keymap_test::Other", "cmd-k w": null}}]"#,
        )
        .unwrap();
        cx.update(|cx| {
            store.update(cx, |store, cx| store.reload(cx));
            // The user's binding runs first; the default still runs if it doesn't take the key.
            assert_eq!(
                runs(cx, "cmd-w"),
                ["keymap_test::Other", "keymap_test::Close"]
            );
            assert!(runs(cx, "cmd-k w").is_empty());
            assert_eq!(store.read(cx).errors().count(), 0);
        });
        // A file that can't be read keeps the bindings it had, and says why; the editor won't
        // write over it.
        std::fs::write(scratch.file(), "[{").unwrap();
        cx.update(|cx| {
            store.update(cx, |store, cx| store.reload(cx));
            assert_eq!(
                runs(cx, "cmd-w"),
                ["keymap_test::Other", "keymap_test::Close"]
            );
            assert_eq!(store.read(cx).errors().count(), 1);
            assert!(!store.read(cx).can_write());
        });
    }

    /// The rows on `keystrokes`: each action's name and where it comes from.
    fn rows_on(store: &Entity<KeymapStore>, cx: &App, keystrokes: &str) -> Vec<(String, Source)> {
        store
            .read(cx)
            .rows(false, cx)
            .into_iter()
            .filter(|row| row.keystrokes.as_ref() == keystrokes)
            .map(|row| (row.action.to_string(), row.source))
            .collect()
    }

    fn user_row(store: &Entity<KeymapStore>, cx: &App, keystrokes: &str, action: &str) -> Source {
        rows_on(store, cx, keystrokes)
            .into_iter()
            .find(|(name, source)| name == action && matches!(source, Source::User(_)))
            .map(|(_, source)| source)
            .unwrap_or_else(|| panic!("a user row {keystrokes} {action}"))
    }

    /// Hand check PR2-1: a second binding of the same keys in the same context is added beside
    /// the first, which the form's clash warning promises: both rows stay, and the one bound last
    /// runs first.
    #[gpui_kit::test]
    fn two_bindings_of_the_same_keys_both_stay(cx: &mut TestAppContext) {
        let scratch = Scratch::new("same-keys");
        let store = start(cx, &scratch);
        cx.update(|cx| {
            for action in ["keymap_test::CloseAll", "keymap_test::Other"] {
                store
                    .update(cx, |s, cx| {
                        s.add(Some("Workspace"), "ctrl-alt-w", action, None, cx)
                    })
                    .unwrap();
            }
            let rows = rows_on(&store, cx, "ctrl-alt-w");
            assert_eq!(rows.len(), 2, "{rows:?}");
            assert_eq!(
                runs(cx, "ctrl-alt-w"),
                ["keymap_test::Other", "keymap_test::CloseAll"]
            );
        });
    }

    /// Removing a default (or changing its keys, `change`) turns off only the default: the
    /// user's own binding of the same keys in the same context stays and still runs.
    fn turn_off_a_default_the_user_rebound(cx: &mut TestAppContext, change: bool) {
        let scratch = Scratch::new(if change { "keep-change" } else { "keep-remove" });
        let store = start(cx, &scratch);
        cx.update(|cx| {
            store
                .update(cx, |s, cx| {
                    s.add(Some("Workspace"), "cmd-w", "keymap_test::Other", None, cx)
                })
                .unwrap();
            let close = default_row(&store, cx, "keymap_test::Close");
            store
                .update(cx, |s, cx| {
                    if change {
                        s.change(
                            close,
                            Some("Workspace"),
                            "cmd-e",
                            "keymap_test::Close",
                            None,
                            cx,
                        )
                    } else {
                        s.remove(close, cx)
                    }
                })
                .unwrap();
            assert_eq!(runs(cx, "cmd-w"), ["keymap_test::Other"]);
            user_row(&store, cx, "cmd-w", "keymap_test::Other");
            let on_cmd_w = rows_on(&store, cx, "cmd-w");
            assert!(
                on_cmd_w
                    .iter()
                    .any(|(name, source)| name == "keymap_test::Close"
                        && matches!(source, Source::Removed { .. })),
                "{on_cmd_w:?}"
            );
            if change {
                assert_eq!(runs(cx, "cmd-e"), ["keymap_test::Close"]);
            }
        });
    }

    #[gpui_kit::test]
    fn removing_a_default_keeps_the_users_binding_of_its_keys(cx: &mut TestAppContext) {
        turn_off_a_default_the_user_rebound(cx, false);
    }

    #[gpui_kit::test]
    fn changing_a_default_keeps_the_users_binding_of_its_keys(cx: &mut TestAppContext) {
        turn_off_a_default_the_user_rebound(cx, true);
    }

    /// Changing a user's binding to keys another of the user's bindings has keeps the other one.
    #[gpui_kit::test]
    fn changing_a_binding_onto_anothers_keys_keeps_the_other(cx: &mut TestAppContext) {
        let scratch = Scratch::new("onto-another");
        let store = start(cx, &scratch);
        cx.update(|cx| {
            store
                .update(cx, |s, cx| {
                    s.add(
                        Some("Workspace"),
                        "ctrl-alt-a",
                        "keymap_test::CloseAll",
                        None,
                        cx,
                    )
                })
                .unwrap();
            store
                .update(cx, |s, cx| {
                    s.add(
                        Some("Workspace"),
                        "ctrl-alt-b",
                        "keymap_test::Other",
                        None,
                        cx,
                    )
                })
                .unwrap();
            let first = user_row(&store, cx, "ctrl-alt-a", "keymap_test::CloseAll");
            store
                .update(cx, |s, cx| {
                    s.change(
                        first,
                        Some("Workspace"),
                        "ctrl-alt-b",
                        "keymap_test::CloseAll",
                        None,
                        cx,
                    )
                })
                .unwrap();
            assert!(rows_on(&store, cx, "ctrl-alt-a").is_empty());
            assert_eq!(
                runs(cx, "ctrl-alt-b"),
                ["keymap_test::CloseAll", "keymap_test::Other"]
            );
        });
    }

    /// A default the file turns off shows as removed however the file does it: with `null` (the
    /// new file's own example), or with an unbind whose context covers the default's (none at
    /// all, or the same written another way), as GPUI reads them.
    #[gpui_kit::test]
    fn a_default_turned_off_by_hand_shows_as_removed(cx: &mut TestAppContext) {
        let scratch = Scratch::new("off-by-hand");
        let store = start(cx, &scratch);
        std::fs::create_dir_all(&scratch.0).unwrap();
        for file in [
            r#"[{"context": "Workspace", "bindings": {"cmd-k w": null}}]"#,
            r#"[{"bindings": {"cmd-k w": null}}]"#,
            r#"[{"bindings": {"cmd-k w": ["zed::Unbind", "keymap_test::CloseAll"]}}]"#,
            r#"[{"context": "Workspace || Editor", "bindings": {"cmd-k w": ["zed::Unbind", "keymap_test::CloseAll"]}}]"#,
        ] {
            std::fs::write(scratch.file(), file).unwrap();
            cx.update(|cx| {
                store.update(cx, |store, cx| store.reload(cx));
                assert!(runs(cx, "cmd-k w").is_empty(), "{file}");
                let source = default_row(&store, cx, "keymap_test::CloseAll");
                assert!(
                    matches!(source, Source::Removed { .. }),
                    "{file}: {source:?}"
                );
                // Restore takes the line out.
                store.update(cx, |s, cx| s.remove(source, cx)).unwrap();
                assert_eq!(runs(cx, "cmd-k w"), ["keymap_test::CloseAll"], "{file}");
            });
        }
        // A null in another context leaves the default as it is.
        std::fs::write(
            scratch.file(),
            r#"[{"context": "Editor", "bindings": {"cmd-k w": null}}]"#,
        )
        .unwrap();
        cx.update(|cx| {
            store.update(cx, |store, cx| store.reload(cx));
            let source = default_row(&store, cx, "keymap_test::CloseAll");
            assert!(matches!(source, Source::Default(_)), "{source:?}");
        });
    }

    /// Writes `text` to the keymap file as another program would, with a modification time the
    /// store can't have seen.
    fn edit_by_hand(scratch: &Scratch, text: &str) {
        std::fs::create_dir_all(&scratch.0).unwrap();
        std::fs::write(scratch.file(), text).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(scratch.file())
            .unwrap()
            .set_modified(later)
            .unwrap();
    }

    /// A change made in the editor before its once-a-second look at the file never writes over
    /// what another program saved there meanwhile: the store reads the file again and asks for
    /// the change again, with the file as it is now.
    #[gpui_kit::test]
    fn an_edit_on_disk_is_never_written_over(cx: &mut TestAppContext) {
        let scratch = Scratch::new("edited-on-disk");
        let store = start(cx, &scratch);
        cx.update(|cx| {
            store
                .update(cx, |s, cx| {
                    s.add(
                        Some("Workspace"),
                        "ctrl-alt-a",
                        "keymap_test::Other",
                        None,
                        cx,
                    )
                })
                .unwrap();
        });
        let by_hand =
            r#"[{"context": "Workspace", "bindings": {"ctrl-alt-h": "keymap_test::Other"}}]"#;
        edit_by_hand(&scratch, by_hand);
        cx.update(|cx| {
            let result = store.update(cx, |s, cx| {
                s.add(
                    Some("Workspace"),
                    "ctrl-alt-b",
                    "keymap_test::CloseAll",
                    None,
                    cx,
                )
            });
            assert!(result.is_err(), "{result:?}");
            // The store has read the hand edit, and binds it.
            assert_eq!(runs(cx, "ctrl-alt-h"), ["keymap_test::Other"]);
            assert!(runs(cx, "ctrl-alt-a").is_empty());
        });
        assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), by_hand);
        // Asked again, the change goes in beside the hand edit.
        cx.update(|cx| {
            store
                .update(cx, |s, cx| {
                    s.add(
                        Some("Workspace"),
                        "ctrl-alt-b",
                        "keymap_test::CloseAll",
                        None,
                        cx,
                    )
                })
                .unwrap();
            assert_eq!(runs(cx, "ctrl-alt-h"), ["keymap_test::Other"]);
            assert_eq!(runs(cx, "ctrl-alt-b"), ["keymap_test::CloseAll"]);
        });
    }

    /// A keymap.json that is a link (to a dotfiles folder) stays a link: the editor writes the
    /// file it points to.
    #[gpui_kit::test]
    fn a_linked_keymap_file_stays_a_link(cx: &mut TestAppContext) {
        let scratch = Scratch::new("linked");
        let dotfiles = scratch.0.join("dotfiles");
        std::fs::create_dir_all(&dotfiles).unwrap();
        let target = dotfiles.join("sawit-keymap.json");
        std::fs::write(&target, "[]").unwrap();
        std::os::unix::fs::symlink(&target, scratch.file()).unwrap();
        let store = start(cx, &scratch);
        cx.update(|cx| {
            store
                .update(cx, |s, cx| {
                    s.add(
                        Some("Workspace"),
                        "ctrl-alt-l",
                        "keymap_test::Other",
                        None,
                        cx,
                    )
                })
                .unwrap();
        });
        let link = std::fs::symlink_metadata(scratch.file()).unwrap();
        assert!(
            link.file_type().is_symlink(),
            "keymap.json is no longer a link"
        );
        assert!(
            std::fs::read_to_string(&target)
                .unwrap()
                .contains("ctrl-alt-l")
        );
    }

    /// A chord the user binds that starts with a key the canvas binds alone (⌘K, its Actions
    /// palette) doesn't keep that key waiting on the canvas for the chord's second key: the
    /// canvas lets it through, as it does the workspace's own ⌘K chords. Off the canvas the
    /// chord works.
    #[gpui_kit::test]
    fn a_users_chord_doesnt_keep_the_canvas_waiting(cx: &mut TestAppContext) {
        let scratch = Scratch::new("chord-canvas");
        let store = cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("cmd-k", Other, Some("Board && !BoardText")),
                KeyBinding::new("cmd-k w", CloseAll, Some("Workspace")),
                KeyBinding::new("cmd-k w", gpui_kit::NoAction, Some("Board")),
            ]);
            init_at(scratch.file(), cx);
            store(cx)
        });
        cx.update(|cx| {
            store
                .update(cx, |s, cx| {
                    s.add(Some("Workspace"), "cmd-k l", "keymap_test::Close", None, cx)
                })
                .unwrap();
            let typed = |keys: &str| -> Vec<Keystroke> {
                keys.split(' ')
                    .map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).unwrap())
                    .collect()
            };
            let canvas = ["Workspace", "Board"].map(|c| KeyContext::parse(c).unwrap());
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            let (bindings, waits) = keymap.bindings_for_input(&typed("cmd-k"), &canvas);
            assert!(!waits, "⌘K waits on the canvas");
            assert_eq!(
                bindings.first().map(|b| b.action().name()),
                Some("keymap_test::Other")
            );
            let workspace = [KeyContext::parse("Workspace").unwrap()];
            let (bindings, _) = keymap.bindings_for_input(&typed("cmd-k l"), &workspace);
            assert_eq!(
                bindings.first().map(|b| b.action().name()),
                Some("keymap_test::Close")
            );
        });
    }

    /// On a layout that types some keys elsewhere (German), the defaults are bound where the
    /// layout types them, as macOS menus have them: ⇧⌘. (`cmd->`) as `cmd-:`, and the window
    /// cycling key `cmd->` (⇧⌘<) is left to macOS. A binding the user turns off there stays off.
    #[gpui_kit::test]
    fn the_defaults_follow_the_keyboard_layout(cx: &mut TestAppContext) {
        let scratch = Scratch::new("layout");
        let store = cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("cmd->", Other, Some("Workspace")),
                KeyBinding::new("cmd-w", Close, Some("Workspace")),
            ]);
            init_at(scratch.file(), cx);
            store(cx)
        });
        let german = |c: char| match c {
            '>' => Some(':'),
            '<' => Some(';'),
            _ => None,
        };
        cx.update(|cx| {
            assert_eq!(runs(cx, "cmd->"), ["keymap_test::Other"]);
            store.update(cx, |s, cx| s.relayout_with(german, cx));
            assert_eq!(runs(cx, "cmd-:"), ["keymap_test::Other"]);
            assert!(runs(cx, "cmd->").is_empty());
            assert_eq!(runs(cx, "cmd-w"), ["keymap_test::Close"]);
            // Removed in the editor, it is unbound where it is bound.
            let other = default_row(&store, cx, "keymap_test::Other");
            store.update(cx, |s, cx| s.remove(other, cx)).unwrap();
            assert!(runs(cx, "cmd-:").is_empty());
            assert!(matches!(
                default_row(&store, cx, "keymap_test::Other"),
                Source::Removed { .. }
            ));
        });
    }

    #[gpui_kit::test]
    fn a_binding_that_cant_load_is_named(cx: &mut TestAppContext) {
        let scratch = Scratch::new("bad-binding");
        let store = start(cx, &scratch);
        std::fs::create_dir_all(&scratch.0).unwrap();
        std::fs::write(
            scratch.file(),
            r#"[{"bindings": {"cmd-e": "no_such::Action", "cmd-r": "keymap_test::Other"}}]"#,
        )
        .unwrap();
        cx.update(|cx| {
            store.update(cx, |store, cx| store.reload(cx));
            let errors: Vec<String> = store.read(cx).errors().map(String::from).collect();
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert!(errors[0].contains("cmd-e"), "{errors:?}");
            // The others load.
            assert_eq!(runs(cx, "cmd-r"), ["keymap_test::Other"]);
        });
    }
}
