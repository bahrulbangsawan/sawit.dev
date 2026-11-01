//! The keymap store's work on the keymap file: reading it, binding its keys over the defaults
//! (where the keyboard layout types them), and writing the editor's changes to it.

use std::path::Path;
use std::time::SystemTime;

use gpui_kit::*;
use serde_json::Value;

use super::file::{self, Entry, UserKeymap};
use super::{KeymapStore, NEW_FILE, Source, USER, names};

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

    /// Whether writing the file would drop comments the user wrote in it, as it is on disk now:
    /// every change writes it whole, as JSON ([`file::has_comments`]). The lines a new file
    /// starts with ([`NEW_FILE`]) are Sawit's, not the user's.
    pub fn has_user_comments(&self) -> bool {
        std::fs::read_to_string(&self.path)
            .is_ok_and(|text| text != NEW_FILE && file::has_comments(&text))
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
    pub(super) fn relayout_with(
        &mut self,
        moves: impl Fn(char) -> Option<char>,
        cx: &mut Context<Self>,
    ) {
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
}

/// The defaults written in `sources` with their keys where the keyboard layout in use types
/// them, and the keymap's display told whether it moves any.
pub(super) fn layout_defaults(sources: &[KeyBinding], cx: &App) -> Vec<KeyBinding> {
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

pub(super) fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Writes `text` to `path` as Sawit writes every file it saves ([`files::write_atomic`]): through
/// a staging file of its own, made new beside it, then renamed over it, so a crash never leaves
/// half a keymap, and nothing already beside it (a link at a name another write used) is written
/// through. The file keeps its permissions, and a `path` that is a link (to a dotfiles folder)
/// stays one: the file it points to is written.
///
/// [`files::write_atomic`]: sawit_core::files::write_atomic
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    sawit_core::files::write_atomic(path, text.as_bytes()).map_err(|e| match e {
        sawit_core::CoreError::Io(e) => e,
        e => std::io::Error::other(e.to_string()),
    })
}
