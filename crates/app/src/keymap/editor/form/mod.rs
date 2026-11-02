//! The binding form: a binding's action (picked by name when new), keys, context and arguments,
//! what the keys would clash with, and a save checked as the editor checks it.

mod render;

use std::collections::HashMap;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use serde_json::Value;

use super::KeymapEditor;
use super::clashes::clash_notes;
#[cfg(test)]
use super::clashes::{chord_clashes, clashes};
use crate::keymap::recorder::{Changed, Done, KeystrokeRecorder};
use crate::keymap::{Row, Source, names, store};

/// What the form changes or adds.
#[derive(Clone)]
pub(super) enum FormTarget {
    /// A row: a binding to change, or an action with none (or a removed default) to bind.
    Row(Row),
    /// A new binding of an action the form picks.
    New,
}

/// The form that changes or adds a binding: its action (picked when new), arguments, keys and
/// context, and the bindings it would clash with.
pub(super) struct BindingForm {
    target: FormTarget,
    editor: WeakEntity<KeymapEditor>,
    /// The form's own: it takes the keyboard when the keys field stops recording (⎋), so that a
    /// second ⎋ cancels.
    focus: FocusHandle,
    pub(super) action: Entity<InputState>,
    pub(super) args: Entity<InputState>,
    pub(super) context: Entity<InputState>,
    pub(super) keys: Entity<KeystrokeRecorder>,
    /// The action names the action field's text matches, while it is being picked.
    suggestions: Vec<&'static str>,
    pub(super) error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl BindingForm {
    pub(super) fn new(
        target: FormTarget,
        editor: WeakEntity<KeymapEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let row = match &target {
            FormTarget::Row(row) => Some(row.clone()),
            FormTarget::New => None,
        };
        let action = cx.new(|cx| {
            let state =
                InputState::new(window, cx).placeholder("Action, e.g. workspace::CloseAllTabs");
            match &row {
                Some(row) => state.default_value(row.action.clone()),
                None => state,
            }
        });
        let args = cx.new(|cx| {
            let state = InputState::new(window, cx)
                .placeholder("No arguments (or JSON, e.g. {\"id\": \"tool.move\"})");
            match row.as_ref().and_then(|r| r.args.clone()) {
                Some(args) => state.default_value(args),
                None => state,
            }
        });
        let context = cx.new(|cx| {
            let state = InputState::new(window, cx)
                .placeholder("Anywhere (or e.g. Workspace, Editor && !Input)");
            match row.as_ref().and_then(|r| r.context.clone()) {
                Some(context) => state.default_value(context),
                None => state,
            }
        });
        let keys = cx.new(|cx| {
            let mut recorder = KeystrokeRecorder::new("Click, then press the keys", window, cx);
            if let Some(row) = &row {
                recorder.set_keystrokes(&row.keystrokes, cx);
            }
            recorder
        });
        let subscriptions = vec![
            cx.subscribe(&action, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.suggest(cx);
                }
            }),
            cx.subscribe(&context, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&keys, |this, _, _: &Changed, cx| {
                this.error = None;
                cx.notify();
            }),
            cx.subscribe_in(
                &keys,
                window,
                |this, _, done: &Done, window, cx| match done {
                    Done::Stopped => {
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }
                    Done::Confirmed => this.save(window, cx),
                },
            ),
        ];
        Self {
            target,
            editor,
            focus: cx.focus_handle(),
            action,
            args,
            context,
            keys,
            suggestions: Vec::new(),
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn is_new(&self) -> bool {
        matches!(self.target, FormTarget::New)
    }

    /// Gives the keyboard to the field to fill first: the action of a new binding, else the keys.
    pub(super) fn focus_first(&self, window: &mut Window, cx: &mut App) {
        if self.is_new() {
            self.action
                .update(cx, |action, cx| action.focus(window, cx));
        } else {
            let focus = self.keys.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        }
    }

    /// The action names that match the action field, best first.
    fn suggest(&mut self, cx: &mut Context<Self>) {
        let query = self.action.read(cx).value().trim().to_lowercase();
        self.suggestions = if query.is_empty() {
            Vec::new()
        } else {
            let mut found: Vec<&'static str> = cx
                .all_action_names()
                .iter()
                .copied()
                .filter(|name| !matches!(*name, "zed::NoAction" | "zed::Unbind"))
                .filter(|name| {
                    let human = names::humanize_action(name);
                    query
                        .split_whitespace()
                        .all(|word| name.to_lowercase().contains(word) || human.contains(word))
                })
                .collect();
            found.sort_by_key(|name| (!name.to_lowercase().starts_with(&query), name.len()));
            found.truncate(8);
            if found.len() == 1 && found[0] == self.action.read(cx).value().trim() {
                found.clear();
            }
            found
        };
        cx.notify();
    }

    fn pick(&mut self, name: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.action
            .update(cx, |action, cx| action.set_value(name, window, cx));
        self.suggestions.clear();
        if name == "board::Command" && self.context.read(cx).value().is_empty() {
            self.context.update(cx, |context, cx| {
                context.set_value(sawit_board::shortcuts::CANVAS_CONTEXT, window, cx)
            });
        }
        let focus = self.keys.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The row the form changes, if it changes one: its own keys don't clash with it.
    fn own(&self) -> Option<Source> {
        match &self.target {
            FormTarget::Row(row) => Some(row.source),
            FormTarget::New => None,
        }
    }

    /// The other bindings of the keys being recorded: those in the same context, which would
    /// clash, then how many in other contexts ([`clashes`]). The form shows them as
    /// [`Self::clash_notes`]; the tests read them here.
    #[cfg(test)]
    pub(super) fn clashes(&self, cx: &App) -> (Vec<String>, usize) {
        let keys = self.keys.read(cx).keystrokes();
        let context = self.context.read(cx).value().trim().to_string();
        let rows = store(cx).read(cx).rows(false, cx);
        clashes(&rows, &keys, &context, self.own())
    }

    /// The active bindings, in this context or anywhere, that the keys being recorded start,
    /// then those that start them ([`chord_clashes`]), as [`Self::clashes`] reads them.
    #[cfg(test)]
    pub(super) fn chord_clashes(&self, cx: &App) -> (Vec<String>, Vec<String>) {
        let keys = self.keys.read(cx).keystrokes();
        let context = self.context.read(cx).value().trim().to_string();
        let rows = store(cx).read(cx).rows(false, cx);
        chord_clashes(&rows, &keys, &context, self.own())
    }

    /// What the keys being recorded clash with, as notes under the fields ([`clash_notes`]).
    fn clash_notes(&self, cx: &App) -> Vec<(&'static str, String)> {
        let keys = self.keys.read(cx).keystrokes();
        let context = self.context.read(cx).value().trim().to_string();
        let rows = store(cx).read(cx).rows(false, cx);
        clash_notes(&rows, &keys, &context, self.own())
    }

    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.try_save(cx) {
            Ok(()) => {
                if let Some(editor) = self.editor.upgrade() {
                    editor.update(cx, |editor, cx| editor.close_form(window, cx));
                }
            }
            Err(error) => {
                self.error = Some(error.into());
                cx.notify();
            }
        }
    }

    fn try_save(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let keystrokes = self.keys.read(cx).keystrokes();
        if keystrokes.is_empty() {
            return Err("Press the keys for the binding first.".into());
        }
        let action = self.action.read(cx).value().trim().to_string();
        if action.is_empty() {
            return Err("Pick the action the keys run.".into());
        }
        let args_text = self.args.read(cx).value().trim().to_string();
        let args: Option<Value> = if args_text.is_empty() {
            None
        } else {
            Some(
                serde_json::from_str(&args_text)
                    .map_err(|e| format!("The arguments aren't JSON: {e}"))?,
            )
        };
        let built = cx
            .build_action(&action, args.clone())
            .map_err(|e| format!("{e}"))?;
        let context = self.context.read(cx).value().trim().to_string();
        if !context.is_empty() {
            KeyBindingContextPredicate::parse(&context)
                .map_err(|e| format!("The context can't be read: {e}"))?;
        }
        let context = (!context.is_empty()).then_some(context.as_str());
        let store = store(cx);
        match &self.target {
            FormTarget::New => store.update(cx, |store, cx| {
                store.add(context, &keystrokes, &action, args, cx)
            }),
            FormTarget::Row(row) => {
                if !store.read(cx).is_current(row, cx) {
                    return Err("keymap.json has changed since this form opened. Cancel, then edit the binding again.".into());
                }
                if let Source::Default(ix) = row.source
                    && row.args.is_none()
                {
                    // A default built in code with arguments the keymap can't show: writing it
                    // back would bind another action. (A default that shows its arguments, as
                    // the canvas's do, may be given others.)
                    let default = store.read(cx).defaults()[ix].action().boxed_clone();
                    if action == row.action.as_ref() && !default.partial_eq(built.as_ref()) {
                        return Err("This default's action has arguments the keymap can't show. Remove it, then create a binding with the arguments.".into());
                    }
                }
                let source = row.source;
                store.update(cx, |store, cx| {
                    store.change(source, context, &keystrokes, &action, args, cx)
                })
            }
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.editor.upgrade() {
            editor.update(cx, |editor, cx| editor.close_form(window, cx));
        }
    }

    fn title(&self) -> SharedString {
        match &self.target {
            FormTarget::New => "Create Keybinding".into(),
            FormTarget::Row(row) => match row.source {
                Source::Unbound | Source::Removed { .. } => {
                    format!("Add a Keybinding for {}", row.label).into()
                }
                _ => format!("Edit {}", row.label).into(),
            },
        }
    }
}

/// The contexts the defaults use most, most used first.
fn known_contexts(cx: &App) -> Vec<SharedString> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for binding in store(cx).read(cx).defaults() {
        if let Some(predicate) = binding.predicate() {
            *counts.entry(predicate.to_string()).or_default() += 1;
        }
    }
    let mut contexts: Vec<(String, usize)> = counts.into_iter().collect();
    contexts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    contexts.into_iter().map(|(c, _)| c.into()).collect()
}
