//! The keymap editor tab: every key binding of the app (each feature's defaults and the user's),
//! with what it runs, where, and where it comes from, as Zed's keymap editor shows them. A
//! binding can be changed, added, removed and restored, and the keymap file opened as JSON.
//!
//! The filter finds an action by its name, its context or its keys; with keystroke search on it
//! records keys instead, and finds the bindings that start with them. Bindings of the same keys in
//! the same context are marked: only one of them runs. A change never removes another binding: a
//! binding saved with keys another one has in its context goes after it, and runs first.

mod clashes;
mod form;
mod render;

pub(crate) use clashes::{binding_counts, clash_notes, conflicts, is_active};

use std::collections::HashMap;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::recorder::{Changed, Done, KeystrokeRecorder};
use super::{KeymapStore, Row, store};
use form::{BindingForm, FormTarget};

actions!(
    keymap_editor,
    [
        /// Edits the selected binding.
        EditBinding,
        /// Removes the selected binding, or restores the removed default.
        RemoveBinding,
        /// Adds a new binding.
        CreateBinding,
        /// Selects the next binding.
        SelectNext,
        /// Selects the previous binding.
        SelectPrevious,
        /// Finds bindings by their keys instead of their names.
        ToggleKeystrokeSearch,
        /// Moves the keyboard to the filter.
        FocusFilter,
        /// Opens the keymap file in an editor tab, from the keymap editor. Its own action, so
        /// that its key (⌘E) stays the editor's: the Sawit menu's Open Keymap File, which runs
        /// `keymap::OpenKeymapFile`, would take a key bound to that as an app-wide shortcut.
        EditInJson
    ]
);

const CONTEXT: &str = "KeymapEditor";

pub fn bindings() -> Vec<KeyBinding> {
    let ctx = Some(CONTEXT);
    let table = Some("KeymapEditor && !Input");
    vec![
        KeyBinding::new("enter", EditBinding, table),
        KeyBinding::new("backspace", RemoveBinding, table),
        KeyBinding::new("down", SelectNext, table),
        KeyBinding::new("up", SelectPrevious, table),
        KeyBinding::new("cmd-n", CreateBinding, ctx),
        KeyBinding::new("alt-cmd-k", ToggleKeystrokeSearch, ctx),
        KeyBinding::new("cmd-f", FocusFilter, ctx),
        KeyBinding::new("cmd-e", EditInJson, ctx),
    ]
}

pub struct KeymapEditor {
    store: Entity<KeymapStore>,
    focus: FocusHandle,
    filter: Entity<InputState>,
    /// The keys keystroke search looks for.
    keys_filter: Entity<KeystrokeRecorder>,
    keystroke_search: bool,
    show_unbound: bool,
    /// Every row of the keymap as last read (with the actions nothing binds while
    /// `show_unbound`), sorted, each after what the filter finds it by ([`haystack`]): read again
    /// when the keymap changes, not at each key typed in the filter.
    all: Vec<(String, Row)>,
    /// The rows the filter leaves, sorted.
    rows: Vec<Row>,
    /// How many active bindings each (context, keystrokes) has, for conflicts.
    conflicts: HashMap<(Option<SharedString>, SharedString), usize>,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    form: Option<Entity<BindingForm>>,
    /// A message after an action that failed.
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl KeymapEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = store(cx);
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter action names…"));
        let keys_filter =
            cx.new(|cx| KeystrokeRecorder::new("Press keys to find their bindings", window, cx));
        let subscriptions = vec![
            cx.observe(&store, |this, _, cx| this.refresh(cx)),
            cx.subscribe(&filter, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.apply_filter(cx);
                }
            }),
            cx.subscribe(&keys_filter, |this, _, _: &Changed, cx| {
                this.apply_filter(cx)
            }),
            // ⎋ or ↩ in the search field: the list takes the keyboard, for ↑, ↓ and ↩.
            cx.subscribe_in(&keys_filter, window, |this, _, _: &Done, window, cx| {
                window.focus(&this.focus, cx)
            }),
        ];
        let mut this = Self {
            store,
            focus: cx.focus_handle(),
            filter,
            keys_filter,
            keystroke_search: false,
            show_unbound: false,
            all: Vec::new(),
            rows: Vec::new(),
            conflicts: HashMap::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            form: None,
            error: None,
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this
    }

    /// Reads the keymap again and filters it.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let all = self.store.read(cx).rows(self.show_unbound, cx);
        self.conflicts = binding_counts(&all);
        self.all = all.into_iter().map(|row| (haystack(&row), row)).collect();
        self.apply_filter(cx);
    }

    /// Filters the keymap as last read ([`Self::refresh`]): all a key typed in the filter, or
    /// pressed for keystroke search, changes.
    fn apply_filter(&mut self, cx: &mut Context<Self>) {
        let selected = self
            .selected
            .and_then(|ix| self.rows.get(ix))
            .map(|row| (row.source, row.action.clone(), row.args.clone()));
        self.rows = if self.keystroke_search {
            let keys = self.keys_filter.read(cx).keystrokes();
            self.all
                .iter()
                .filter(|(_, row)| keys.is_empty() || starts_with_keys(&row.keystrokes, &keys))
                .map(|(_, row)| row.clone())
                .collect()
        } else {
            let query = self.filter.read(cx).value().trim().to_lowercase();
            self.all
                .iter()
                .filter(|(haystack, _)| has_words(haystack, &query))
                .map(|(_, row)| row.clone())
                .collect()
        };
        self.selected = selected.and_then(|(source, action, args)| {
            self.rows
                .iter()
                .position(|r| r.source == source && r.action == action && r.args == args)
        });
        cx.notify();
    }

    fn row_conflicts(&self, row: &Row) -> bool {
        conflicts(row, &self.conflicts)
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.rows.len() {
            self.selected = Some(ix);
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let next = self.selected.map_or(0, |ix| ix + 1);
        self.select(next.min(self.rows.len().saturating_sub(1)), cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        let previous = self.selected.map_or(0, |ix| ix.saturating_sub(1));
        self.select(previous, cx);
    }

    fn edit_selected(&mut self, _: &EditBinding, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.edit(ix, window, cx);
        }
    }

    fn remove_selected(&mut self, _: &RemoveBinding, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.remove(ix, cx);
        }
    }

    /// Opens the form for row `ix`: a binding to change, or an action with none to bind.
    fn edit(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(ix).cloned() else {
            return;
        };
        self.selected = Some(ix);
        self.open_form(FormTarget::Row(row), window, cx);
    }

    fn create(&mut self, _: &CreateBinding, window: &mut Window, cx: &mut Context<Self>) {
        self.open_form(FormTarget::New, window, cx);
    }

    fn open_form(&mut self, target: FormTarget, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.weak_entity();
        let form = cx.new(|cx| BindingForm::new(target, editor, window, cx));
        form.update(cx, |form, cx| form.focus_first(window, cx));
        self.form = Some(form);
        cx.notify();
    }

    fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Takes row `ix` out: a user's binding leaves the file, a default is turned off, and a
    /// removed default comes back.
    fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(ix) else {
            return;
        };
        let source = row.source;
        let result = self.store.update(cx, |store, cx| store.remove(source, cx));
        self.error = result.err().map(Into::into);
        cx.notify();
    }

    fn toggle_keystroke_search(
        &mut self,
        _: &ToggleKeystrokeSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keystroke_search = !self.keystroke_search;
        if self.keystroke_search {
            self.keys_filter.update(cx, |keys, cx| keys.clear(cx));
            let focus = self.keys_filter.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        } else {
            self.filter
                .update(cx, |filter, cx| filter.focus(window, cx));
        }
        self.apply_filter(cx);
    }

    fn focus_filter(&mut self, _: &FocusFilter, window: &mut Window, cx: &mut Context<Self>) {
        if self.keystroke_search {
            let focus = self.keys_filter.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        } else {
            self.filter
                .update(cx, |filter, cx| filter.focus(window, cx));
        }
    }

    fn toggle_unbound(&mut self, cx: &mut Context<Self>) {
        self.show_unbound = !self.show_unbound;
        self.refresh(cx);
    }
}

impl Focusable for KeymapEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Whether `row` matches the filter's `query` (lower case): its name, action, keys or context.
pub(crate) fn matches_query(row: &Row, query: &str) -> bool {
    query.is_empty() || has_words(&haystack(row), query)
}

/// What the filter finds `row` by: its name, action, keys (as the keymap file and as a menu
/// writes them), context and arguments, in lower case, a line each.
fn haystack(row: &Row) -> String {
    let mut haystack = String::new();
    for text in [
        &*row.label,
        &*row.action,
        &*row.keystrokes,
        &*row.display,
        row.context.as_deref().unwrap_or_default(),
        row.args.as_deref().unwrap_or_default(),
    ] {
        haystack.push_str(&text.to_lowercase());
        haystack.push('\n');
    }
    haystack
}

/// Whether `haystack` ([`haystack`]) has every word of `query` (lower case) in it. A word has no
/// whitespace, so none spans two of its lines.
fn has_words(haystack: &str, query: &str) -> bool {
    query.split_whitespace().all(|word| haystack.contains(word))
}

/// Whether `keystrokes` start with `keys` (both as the keymap file writes them).
pub(crate) fn starts_with_keys(keystrokes: &str, keys: &str) -> bool {
    let mut have = keystrokes.split_whitespace();
    keys.split_whitespace().all(|key| have.next() == Some(key))
}

#[cfg(test)]
mod tests;

/// The editor in a window, driven as a person does: the form opened on a row, keys pressed into
/// its recorder, then Save.
#[cfg(test)]
mod form_tests;
