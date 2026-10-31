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
    /// What every feature bound in code, as [`init`] found it, each with [`DEFAULT`].
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
}

/// [`init`] with the keymap file at `path`, without watching it (tests).
pub fn init_at(path: PathBuf, cx: &mut App) {
    cx.bind_keys(editor::bindings());
    let defaults = cx
        .key_bindings()
        .borrow()
        .bindings()
        .map(|b| b.clone().with_meta(DEFAULT))
        .collect();
    let store = cx.new(|_| KeymapStore {
        path,
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

    /// Binds the defaults, then the user's bindings, in place of every binding the app has.
    fn apply(&self, cx: &mut App) {
        cx.clear_key_bindings();
        cx.bind_keys(self.defaults.iter().cloned());
        cx.bind_keys(self.loaded.iter().map(|(_, binding)| binding.clone()));
        crate::cli::set_menus(cx);
    }

    /// Writes the file with `change` made to it, then binds the keys anew.
    fn write(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut UserKeymap),
    ) -> Result<(), String> {
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

    /// Adds a binding of `keystrokes` to `action` in `context`. Replaces the user's binding of
    /// the same keystrokes in the same context.
    pub fn add(
        &mut self,
        context: Option<&str>,
        keystrokes: &str,
        action: &str,
        args: Option<Value>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.write(cx, |user| {
            user.set(context, keystrokes, Entry::action(action, args))
        })
    }

    /// Changes the row `source` to bind `keystrokes` to `action` in `context`. A user's binding
    /// changes in the file; a default is turned off there (`zed::Unbind`) and the new binding
    /// added, as Zed's keymap editor does.
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
                user.remove(at);
                user.set(context, keystrokes, entry);
            }),
            Source::Default(ix) => {
                let (default_context, default_keys, default_action) = self.default_parts(ix);
                let same_place = default_context.as_deref() == context.map(str::trim)
                    && Some(default_keys.as_str())
                        == names::normalize_keystrokes(keystrokes).as_deref();
                self.write(cx, |user| {
                    // The same keys in the same context: the user's binding, which is later and
                    // stronger, takes them, and an unbind under the same key would be replaced
                    // by it anyway.
                    if !same_place {
                        user.set(
                            default_context.as_deref(),
                            &default_keys,
                            Entry::unbind(&default_action),
                        );
                    }
                    user.set(context, keystrokes, entry);
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
                    user.set(context.as_deref(), &keys, Entry::unbind(&action))
                })
            }
            Source::Unbound => Ok(()),
        }
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
            let (context, keys, name) = self.default_parts(ix);
            let source = match self.user.find_unbind(context.as_deref(), &keys, &name) {
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

/// Writes `text` to `path` through a file beside it, so a crash never leaves half a keymap.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
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
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(test: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("sawit-keymap-{}-{test}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }

        fn file(&self) -> PathBuf {
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
    fn runs(cx: &App, keystroke: &str) -> Vec<&'static str> {
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
