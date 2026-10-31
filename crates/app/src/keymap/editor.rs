//! The keymap editor tab: every key binding of the app (each feature's defaults and the user's),
//! with what it runs, where, and where it comes from, as Zed's keymap editor shows them. A
//! binding can be changed, added, removed and restored, and the keymap file opened as JSON.
//!
//! The filter finds an action by its name, its context or its keys; with keystroke search on it
//! records keys instead, and finds the bindings that start with them. Bindings of the same keys in
//! the same context are marked: only one of them runs. A change never removes another binding: a
//! binding saved with keys another one has in its context goes after it, and runs first.

use std::collections::HashMap;
use std::ops::Range;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;

use super::recorder::{Changed, Done, KeystrokeRecorder};
use super::{KeymapStore, OpenKeymapFile, Row, Source, names, store};
use crate::states::{Banner, Placeholder};

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

const ROW_HEIGHT: f32 = 32.;
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
                    this.refresh(cx);
                }
            }),
            cx.subscribe(&keys_filter, |this, _, _: &Changed, cx| this.refresh(cx)),
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
        self.conflicts.clear();
        for row in all.iter().filter(|r| is_active(r)) {
            *self
                .conflicts
                .entry((row.context.clone(), row.keystrokes.clone()))
                .or_default() += 1;
        }
        let selected = self
            .selected
            .and_then(|ix| self.rows.get(ix))
            .map(|row| (row.source, row.action.clone(), row.args.clone()));
        self.rows = if self.keystroke_search {
            let keys = self.keys_filter.read(cx).keystrokes();
            all.into_iter()
                .filter(|row| keys.is_empty() || starts_with_keys(&row.keystrokes, &keys))
                .collect()
        } else {
            let query = self.filter.read(cx).value().trim().to_lowercase();
            all.into_iter()
                .filter(|row| matches_query(row, &query))
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
        is_active(row)
            && self
                .conflicts
                .get(&(row.context.clone(), row.keystrokes.clone()))
                .is_some_and(|n| *n > 1)
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
        self.refresh(cx);
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

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let search: AnyElement = if self.keystroke_search {
            div()
                .flex_1()
                .child(self.keys_filter.clone())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .child(Input::new(&self.filter).cleanable(true))
                .into_any_element()
        };
        let can_write = self.store.read(cx).can_write();
        h_flex()
            .gap_1()
            .p_2()
            .child(search)
            .child(
                Button::new("keystroke-search")
                    .ghost()
                    .small()
                    .icon(Lucide::Keyboard)
                    .selected(self.keystroke_search)
                    .tooltip_with_action(
                        "Search by Keystroke",
                        &ToggleKeystrokeSearch,
                        Some(CONTEXT),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_keystroke_search(&ToggleKeystrokeSearch, window, cx)
                    })),
            )
            .child(
                Button::new("show-unbound")
                    .ghost()
                    .small()
                    .icon(Lucide::ListFilter)
                    .selected(self.show_unbound)
                    .tooltip(if self.show_unbound {
                        "Hide Actions Without a Binding"
                    } else {
                        "Show Actions Without a Binding"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_unbound(cx))),
            )
            .child(
                Button::new("edit-json")
                    .ghost()
                    .small()
                    .label("Edit in JSON")
                    .tooltip_with_action("Open keymap.json", &EditInJson, Some(CONTEXT))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenKeymapFile), cx)),
            )
            .child(
                Button::new("create-binding")
                    .outline()
                    .small()
                    .icon(Lucide::Plus)
                    .label("Create Keybinding")
                    .disabled(!can_write)
                    .tooltip_with_action("Create Keybinding", &CreateBinding, Some(CONTEXT))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.create(&CreateBinding, window, cx)),
                    ),
            )
    }

    fn render_header(&self, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        h_flex()
            .h(px(ROW_HEIGHT))
            .px_2()
            .border_b_1()
            .border_color(theme.border)
            .text_sm()
            .font_semibold()
            .text_color(theme.muted_foreground)
            .child(div().w(px(28.)))
            .child(cell_action().child("Action"))
            .child(cell_args().child("Arguments"))
            .child(cell_keys().child("Keystrokes"))
            .child(cell_context().child("Context"))
            .child(cell_source().child("Source"))
            .child(div().w(px(56.)))
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(ix) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let selected = self.selected == Some(ix);
        let removed = matches!(row.source, Source::Removed { .. });
        let unbound = row.source == Source::Unbound;
        let conflict = self.row_conflicts(row);
        let muted = theme.muted_foreground;
        let source = match row.source {
            Source::Default(_) => "Default",
            Source::User(_) => "User",
            Source::Removed { .. } => "Removed",
            Source::Unbound => "",
        };
        let (remove_icon, remove_tip) = match row.source {
            Source::Removed { .. } => (Lucide::RotateCcw, "Restore the Default"),
            Source::Default(_) => (Lucide::Trash, "Remove the Default Binding"),
            _ => (Lucide::Trash, "Remove the Binding"),
        };
        let group = SharedString::from(format!("keymap-row-{ix}"));
        h_flex()
            .id(("keymap-row", ix))
            .group(group.clone())
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_2()
            .text_sm()
            .border_b_1()
            .border_color(theme.border.opacity(0.4))
            .when(ix % 2 == 1, |el| el.bg(theme.table_even))
            .when(selected, |el| el.bg(theme.table_active))
            .hover(|el| el.bg(theme.table_hover))
            .when(removed || unbound, |el| el.text_color(muted))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.selected = Some(ix);
                window.focus(&this.focus, cx);
                if event.click_count() >= 2 {
                    this.edit(ix, window, cx);
                }
                cx.notify();
            }))
            .child(
                div().w(px(28.)).child(
                    div()
                        .invisible()
                        .group_hover(group.clone(), |el| el.visible())
                        .when(selected, |el| el.visible())
                        .child(
                            Button::new(("edit-binding", ix))
                                .ghost()
                                .xsmall()
                                .icon(Lucide::Pencil)
                                .tooltip(if unbound { "Add a Keybinding" } else { "Edit" })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.edit(ix, window, cx)
                                })),
                        ),
                ),
            )
            .child(
                cell_action()
                    .child(
                        div()
                            .id(("action-name", ix))
                            .truncate()
                            .when(removed, |el| el.line_through())
                            .child(row.label.clone())
                            .tooltip({
                                let name = row.action.clone();
                                move |window, cx| Tooltip::new(name.clone()).build(window, cx)
                            }),
                    ),
            )
            .child(
                cell_args().child(
                    div()
                        .truncate()
                        .font_family("monospace")
                        .text_xs()
                        .text_color(muted)
                        .children(row.args.clone()),
                ),
            )
            .child(
                cell_keys()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .font_family("monospace")
                            .when(removed, |el| el.line_through())
                            .child(row.display.clone()),
                    )
                    .when(conflict, |el| {
                        el.child(
                            div()
                                .id(("conflict", ix))
                                .child(
                                    Icon::new(Lucide::TriangleAlert)
                                        .xsmall()
                                        .text_color(theme.warning),
                                )
                                .tooltip(|window, cx| {
                                    Tooltip::new(
                                        "Another binding has these keys in this context: only one runs",
                                    )
                                    .build(window, cx)
                                }),
                        )
                    }),
            )
            .child(cell_context().child(render_context(row.context.as_ref(), unbound, cx)))
            .child(cell_source().text_color(muted).child(source))
            .child(
                h_flex()
                    .w(px(56.))
                    .justify_end()
                    .invisible()
                    .group_hover(group, |el| el.visible())
                    .when(selected, |el| el.visible())
                    .when(!unbound, |el| {
                        el.child(
                            Button::new(("remove-binding", ix))
                                .ghost()
                                .xsmall()
                                .icon(remove_icon)
                                .tooltip(remove_tip)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.remove(ix, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl Focusable for KeymapEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for KeymapEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let errors: Vec<String> = store.errors().map(String::from).collect();
        let count = self.rows.len();
        let theme = cx.theme();
        v_flex()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(theme.background)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::edit_selected))
            .on_action(cx.listener(Self::remove_selected))
            .on_action(cx.listener(Self::create))
            .on_action(cx.listener(Self::toggle_keystroke_search))
            .on_action(cx.listener(Self::focus_filter))
            .on_action(|_: &EditInJson, window, cx| {
                window.dispatch_action(Box::new(OpenKeymapFile), cx)
            })
            .child(self.render_toolbar(cx))
            .children(errors.into_iter().enumerate().map(|(ix, error)| {
                div()
                    .px_2()
                    .pb_1()
                    .child(Banner::error(("keymap-error", ix), error))
            }))
            .children(self.error.clone().map(|error| {
                div()
                    .px_2()
                    .pb_1()
                    .child(Banner::error("keymap-action-error", error))
            }))
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when(count > 0, |el| {
                        el.child(
                            uniform_list(
                                "keymap-rows",
                                count,
                                cx.processor(move |this, range: Range<usize>, _, cx| {
                                    range.map(|ix| this.render_row(ix, cx)).collect()
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        )
                    })
                    .when(count == 0, |el| {
                        el.child(
                            Placeholder::new(Lucide::Keyboard, "No bindings match")
                                .detail(if self.keystroke_search {
                                    "Nothing is bound to these keys. Press other keys, or turn keystroke search off."
                                } else {
                                    "Try another name, or show the actions without a binding."
                                }),
                        )
                    }),
            )
            .children(self.form.clone().map(|form| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_start()
                    .justify_center()
                    .pt(px(72.))
                    .bg(gpui_kit::black().opacity(0.35))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.close_form(window, cx)),
                    )
                    .child(form)
            }))
    }
}

/// Whether `row` is a binding that runs: a default or a user's, not one that only turns keys off.
fn is_active(row: &Row) -> bool {
    matches!(row.source, Source::Default(_) | Source::User(_))
        && row.action.as_ref() != "zed::NoAction"
}

/// Whether `row` matches the filter's `query` (lower case): its name, action, keys or context.
fn matches_query(row: &Row, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let haystacks = [
        row.label.to_lowercase(),
        row.action.to_lowercase(),
        row.keystrokes.to_lowercase(),
        row.display.to_lowercase(),
        row.context.as_deref().unwrap_or_default().to_lowercase(),
        row.args.as_deref().unwrap_or_default().to_lowercase(),
    ];
    query
        .split_whitespace()
        .all(|word| haystacks.iter().any(|h| h.contains(word)))
}

/// Whether `keystrokes` start with `keys` (both as the keymap file writes them).
fn starts_with_keys(keystrokes: &str, keys: &str) -> bool {
    let mut have = keystrokes.split_whitespace();
    keys.split_whitespace().all(|key| have.next() == Some(key))
}

fn cell_action() -> Div {
    h_flex().flex_1().min_w_0().pr_3()
}

fn cell_args() -> Div {
    h_flex().w(px(180.)).flex_shrink_0().min_w_0().pr_3()
}

fn cell_keys() -> Div {
    h_flex().w(px(150.)).flex_shrink_0().min_w_0().pr_3()
}

fn cell_context() -> Div {
    h_flex().flex_1().min_w_0().pr_3()
}

fn cell_source() -> Div {
    h_flex().w(px(80.)).flex_shrink_0()
}

/// A context predicate with its operators in the accent color: `AcpThread > Editor && mode == full`.
fn render_context(context: Option<&SharedString>, muted: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let Some(context) = context else {
        return div()
            .text_color(theme.muted_foreground)
            .child("(anywhere)")
            .into_any_element();
    };
    let mut parts: Vec<AnyElement> = Vec::new();
    for token in tokenize_context(context) {
        let operator = matches!(token, "&&" | "||" | "!" | ">" | "==" | "!=" | "(" | ")");
        parts.push(
            div()
                .when(operator && !muted, |el| el.text_color(theme.link))
                .child(SharedString::from(token.to_string()))
                .into_any_element(),
        );
    }
    h_flex()
        .gap_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .children(parts)
        .into_any_element()
}

/// The words and operators of a context predicate, in order.
fn tokenize_context(context: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = context.as_bytes();
    let mut ix = 0;
    while ix < bytes.len() {
        let c = bytes[ix];
        if c.is_ascii_whitespace() {
            ix += 1;
            continue;
        }
        let two = context.get(ix..ix + 2);
        let len = if matches!(two, Some("&&" | "||" | "==" | "!=")) {
            2
        } else if matches!(c, b'!' | b'>' | b'(' | b')') {
            1
        } else {
            let mut end = ix;
            while end < bytes.len()
                && !bytes[end].is_ascii_whitespace()
                && !matches!(bytes[end], b'!' | b'>' | b'(' | b')' | b'&' | b'|' | b'=')
            {
                end += 1;
            }
            (end - ix).max(1)
        };
        tokens.push(&context[ix..ix + len]);
        ix += len;
    }
    tokens
}

/// What the form changes or adds.
#[derive(Clone)]
enum FormTarget {
    /// A row: a binding to change, or an action with none (or a removed default) to bind.
    Row(Row),
    /// A new binding of an action the form picks.
    New,
}

/// The form that changes or adds a binding: its action (picked when new), arguments, keys and
/// context, and the bindings it would clash with.
struct BindingForm {
    target: FormTarget,
    editor: WeakEntity<KeymapEditor>,
    /// The form's own: it takes the keyboard when the keys field stops recording (⎋), so that a
    /// second ⎋ cancels.
    focus: FocusHandle,
    action: Entity<InputState>,
    args: Entity<InputState>,
    context: Entity<InputState>,
    keys: Entity<KeystrokeRecorder>,
    /// The action names the action field's text matches, while it is being picked.
    suggestions: Vec<&'static str>,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl BindingForm {
    fn new(
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
    fn focus_first(&self, window: &mut Window, cx: &mut App) {
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

    /// The other bindings of the keys being recorded: those in the same context, which would
    /// clash, then how many in other contexts.
    fn clashes(&self, cx: &App) -> (Vec<String>, usize) {
        let keys = self.keys.read(cx).keystrokes();
        if keys.is_empty() {
            return (Vec::new(), 0);
        }
        let context = self.context.read(cx).value().trim().to_string();
        let own = match &self.target {
            FormTarget::Row(row) => Some(row.source),
            FormTarget::New => None,
        };
        let rows = store(cx).read(cx).rows(false, cx);
        let mut same = Vec::new();
        let mut elsewhere = 0;
        for row in rows
            .iter()
            .filter(|r| is_active(r) && Some(r.source) != own)
        {
            if row.keystrokes.as_ref() != keys {
                continue;
            }
            if row.context.as_deref().unwrap_or_default() == context {
                same.push(row.label.to_string());
            } else {
                elsewhere += 1;
            }
        }
        (same, elsewhere)
    }

    /// The active bindings, in this context or anywhere, that the keys being recorded start
    /// (`⌘K W (workspace: close all tabs)`), then those that start them: a key bound alone and a
    /// chord on it either keep the key waiting for the chord's next key, or leave the chord out
    /// of reach.
    fn chord_clashes(&self, cx: &App) -> (Vec<String>, Vec<String>) {
        let keys = self.keys.read(cx).keystrokes();
        if keys.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let context = self.context.read(cx).value().trim().to_string();
        let own = match &self.target {
            FormTarget::Row(row) => Some(row.source),
            FormTarget::New => None,
        };
        let rows = store(cx).read(cx).rows(false, cx);
        let (mut longer, mut shorter) = (Vec::new(), Vec::new());
        for row in rows
            .iter()
            .filter(|r| is_active(r) && Some(r.source) != own)
        {
            let row_context = row.context.as_deref().unwrap_or_default();
            if !(row_context == context || row_context.is_empty() || context.is_empty()) {
                continue;
            }
            let named = || format!("{} ({})", row.display, row.label);
            if row.keystrokes.len() > keys.len() && starts_with_keys(&row.keystrokes, &keys) {
                longer.push(named());
            } else if keys.len() > row.keystrokes.len() && starts_with_keys(&keys, &row.keystrokes)
            {
                shorter.push(named());
            }
        }
        (longer, shorter)
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

impl Render for BindingForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let (same, elsewhere) = self.clashes(cx);
        let (longer, shorter) = self.chord_clashes(cx);
        let keys = self.keys.read(cx).keystrokes();
        let label = |text: &'static str| {
            div()
                .text_xs()
                .font_semibold()
                .text_color(muted)
                .child(text)
        };
        let known_contexts = known_contexts(cx);
        let new = self.is_new();
        v_flex()
            .id("binding-form")
            .track_focus(&self.focus)
            .w(px(560.))
            .max_w_full()
            .gap_3()
            .p_4()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_lg()
            // A click inside doesn't close the form.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    this.cancel(window, cx);
                }
            }))
            .child(div().text_base().font_semibold().child(self.title()))
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Action"))
                    .map(|el| {
                        if new {
                            el.child(Input::new(&self.action).small())
                                .children(self.suggestions.iter().copied().enumerate().map(
                                    |(ix, name)| {
                                        h_flex()
                                            .id(("suggestion", ix))
                                            .px_2()
                                            .py_1()
                                            .gap_2()
                                            .rounded(theme.radius)
                                            .cursor_pointer()
                                            .hover(|el| el.bg(theme.list_hover))
                                            .text_sm()
                                            .child(names::humanize_action(name))
                                            .child(div().text_xs().text_color(muted).child(name))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.pick(name, window, cx)
                                            }))
                                    },
                                ))
                        } else {
                            let FormTarget::Row(row) = &self.target else {
                                return el;
                            };
                            el.child(
                                h_flex()
                                    .gap_2()
                                    .text_sm()
                                    .child(row.label.clone())
                                    .child(div().text_xs().text_color(muted).child(row.action.clone())),
                            )
                        }
                    }),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Keystrokes"))
                    .child(self.keys.clone())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Every key is recorded while the field is focused, up to 3 for a chord (⌘K ⌘S). ↩ saves; ⎋ stops recording, and a second ⎋ cancels."),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Context"))
                    .child(Input::new(&self.context).small())
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_1()
                            .children(known_contexts.into_iter().take(10).enumerate().map(
                                |(ix, context)| {
                                    Button::new(("context", ix))
                                        .ghost()
                                        .xsmall()
                                        .label(context.clone())
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.context.update(cx, |input, cx| {
                                                input.set_value(context.clone(), window, cx)
                                            });
                                            cx.notify();
                                        }))
                                },
                            )),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Arguments"))
                    .child(Input::new(&self.args).small()),
            )
            .when(!same.is_empty(), |el| {
                el.child(Banner::info(
                    "binding-clash",
                    format!(
                        "{} is also bound to {} in this context. Both bindings stay, and this one runs: the binding saved last goes first.",
                        names::display_typed(&parse_keys(&keys)),
                        same.join(", ")
                    ),
                ))
            })
            .when(!longer.is_empty(), |el| {
                el.child(Banner::info(
                    "binding-starts-chords",
                    format!(
                        "{} also starts {} here. Bound alone, it runs at once, and those chords can no longer be typed here.",
                        names::display_typed(&parse_keys(&keys)),
                        longer.join(", ")
                    ),
                ))
            })
            .when(!shorter.is_empty(), |el| {
                el.child(Banner::info(
                    "binding-extends-keys",
                    format!(
                        "These keys start with {}, bound alone here: with this chord, those keys wait up to a second for the next key before they run.",
                        shorter.join(", ")
                    ),
                ))
            })
            .when(same.is_empty() && elsewhere > 0, |el| {
                el.child(Banner::info(
                    "binding-elsewhere",
                    format!(
                        "These keys are bound in {elsewhere} other context{}: the deepest context with the focus wins.",
                        if elsewhere == 1 { "" } else { "s" }
                    ),
                ))
            })
            .children(
                self.error
                    .clone()
                    .map(|error| Banner::error("binding-error", error)),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel-binding")
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
                    )
                    .child(
                        Button::new("save-binding")
                            .primary()
                            .small()
                            .label("Save")
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
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

fn parse_keys(text: &str) -> Vec<Keystroke> {
    text.split_whitespace()
        .filter_map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{starts_with_keys, tokenize_context};

    #[test]
    fn tokenizes_contexts() {
        assert_eq!(
            tokenize_context("AcpThread > Editor && !use_modifier_to_send"),
            [
                "AcpThread",
                ">",
                "Editor",
                "&&",
                "!",
                "use_modifier_to_send"
            ]
        );
        assert_eq!(
            tokenize_context("Editor && mode == full"),
            ["Editor", "&&", "mode", "==", "full"]
        );
    }

    #[test]
    fn keystroke_search_matches_a_chords_start() {
        assert!(starts_with_keys("cmd-k e", "cmd-k"));
        assert!(starts_with_keys("cmd-k e", "cmd-k e"));
        assert!(!starts_with_keys("cmd-k", "cmd-k e"));
        assert!(!starts_with_keys("cmd-e", "cmd-k"));
    }
}

/// The editor in a window, driven as a person does: the form opened on a row, keys pressed into
/// its recorder, then Save.
#[cfg(test)]
mod form_tests {
    use gpui_kit::{
        AnyWindowHandle, App, AppContext as _, Entity, KeyBinding, KeyBindingContextPredicate,
        KeyContext, Keystroke, OperatingSystem, TestAppContext,
    };
    use sawit_board::shortcuts::CANVAS_CONTEXT;

    use super::super::tests::{Scratch, runs};
    use super::KeymapEditor;

    gpui_kit::actions!(keymap_editor_test, [ToggleDock, CloseAll, Other]);

    /// A keymap editor in an active window, over the defaults ⌘J (toggle the dock) and ⌘K W
    /// (close all) in `Workspace`, and the canvas's R (the rectangle tool), with its keymap file
    /// in `scratch`.
    fn open(cx: &mut TestAppContext, scratch: &Scratch) -> (AnyWindowHandle, Entity<KeymapEditor>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-j", ToggleDock, Some("Workspace")),
                KeyBinding::new("cmd-k w", CloseAll, Some("Workspace")),
            ]);
            cx.bind_keys([canvas_key("r", "tool.rectangle", cx)]);
            crate::keymap::init_at(scratch.file(), cx);
        });
        let (window, editor) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                cx.new(|cx| KeymapEditor::new(window, cx))
            })
            .unwrap()
        });
        cx.update_window(window, |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        (window, editor)
    }

    /// The canvas's binding of `keys` to its command `id`, as `board::bindings` makes it.
    fn canvas_key(keys: &str, id: &str, cx: &App) -> KeyBinding {
        let context = KeyBindingContextPredicate::parse(CANVAS_CONTEXT).unwrap();
        KeyBinding::load(
            keys,
            Box::new(crate::board::Command { id: id.into() }),
            Some(std::rc::Rc::new(context)),
            false,
            Some(serde_json::json!({ "id": id }).to_string().into()),
            cx,
        )
        .unwrap()
    }

    /// The canvas commands `keys` runs on the canvas, by id.
    fn canvas_runs(cx: &App, keys: &str) -> Vec<String> {
        let typed = [Keystroke::parse_with_os(keys, OperatingSystem::Mac).unwrap()];
        let contexts = ["Workspace", "Board"].map(|c| KeyContext::parse(c).unwrap());
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, _) = keymap.bindings_for_input(&typed, &contexts);
        bindings
            .iter()
            .filter_map(|b| b.action().as_any().downcast_ref::<crate::board::Command>())
            .map(|command| command.id.clone())
            .collect()
    }

    /// Opens the form on the first row of `action`, as its Edit button does.
    fn edit(
        cx: &mut TestAppContext,
        window: AnyWindowHandle,
        editor: &Entity<KeymapEditor>,
        action: &str,
    ) {
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                let ix = editor
                    .rows
                    .iter()
                    .position(|r| r.action.as_ref() == action)
                    .unwrap_or_else(|| panic!("a row for {action}"));
                editor.edit(ix, window, cx);
            })
        })
        .unwrap();
        cx.run_until_parked();
    }

    /// The keys the open form's recorder holds, as the keymap file writes them.
    fn recorded(cx: &mut TestAppContext, editor: &Entity<KeymapEditor>) -> String {
        cx.update(|cx| {
            let form = editor.read(cx).form.clone().expect("the form is open");
            form.read(cx).keys.read(cx).keystrokes()
        })
    }

    /// Clicks the open form's Save.
    fn save(cx: &mut TestAppContext, window: AnyWindowHandle, editor: &Entity<KeymapEditor>) {
        cx.update_window(window, |_, window, cx| {
            let form = editor.read(cx).form.clone().expect("the form is open");
            form.update(cx, |form, cx| form.save(window, cx));
        })
        .unwrap();
        cx.run_until_parked();
    }

    /// Hand check PR2-1: Edit a default (⌘J), press ⌃⌥J, Save. The new keys replace the row's,
    /// which the form shows until the first key: ⌃⌥J alone toggles the dock, and ⌘J is unbound.
    #[gpui_kit::test]
    fn editing_a_binding_records_only_the_new_keys(cx: &mut TestAppContext) {
        let scratch = Scratch::new("form-new-keys");
        let (window, editor) = open(cx, &scratch);
        edit(cx, window, &editor, "keymap_editor_test::ToggleDock");
        assert_eq!(
            recorded(cx, &editor),
            "cmd-j",
            "the form starts with the row's keys"
        );
        cx.simulate_keystrokes(window, "ctrl-alt-j");
        assert_eq!(recorded(cx, &editor), "ctrl-alt-j");
        // The next key makes a chord of them.
        cx.simulate_keystrokes(window, "ctrl-alt-k");
        assert_eq!(recorded(cx, &editor), "ctrl-alt-j ctrl-alt-k");
        // Back to the field (a click gives it the keyboard), the next key starts again.
        cx.update_window(window, |_, window, cx| {
            let form = editor.read(cx).form.clone().unwrap();
            let context = form.read(cx).context.clone();
            context.update(cx, |context, cx| context.focus(window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            let form = editor.read(cx).form.clone().unwrap();
            let keys = form.read(cx).keys.read(cx).focus_handle().clone();
            window.focus(&keys, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window, "ctrl-alt-j");
        assert_eq!(recorded(cx, &editor), "ctrl-alt-j");
        save(cx, window, &editor);
        cx.update(|cx| {
            assert!(
                editor.read(cx).form.is_none(),
                "{:?}",
                editor.read(cx).error
            );
            assert_eq!(runs(cx, "ctrl-alt-j"), ["keymap_editor_test::ToggleDock"]);
            assert!(runs(cx, "cmd-j").is_empty(), "{:?}", runs(cx, "cmd-j"));
        });
        let text = std::fs::read_to_string(scratch.file()).unwrap();
        assert!(!text.contains("cmd-j ctrl-alt-j"), "{text}");
    }

    /// Creates a binding through the form, as Create Keybinding does: the action typed, the keys
    /// recorded, the context written.
    fn create(
        cx: &mut TestAppContext,
        window: AnyWindowHandle,
        editor: &Entity<KeymapEditor>,
        action: &str,
        keys: &str,
        context: &str,
    ) {
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.create(&super::CreateBinding, window, cx)
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            let form = editor.read(cx).form.clone().expect("the form is open");
            let (action_field, context_field, recorder) = {
                let form = form.read(cx);
                (form.action.clone(), form.context.clone(), form.keys.clone())
            };
            action_field.update(cx, |a, cx| a.set_value(action.to_string(), window, cx));
            context_field.update(cx, |c, cx| c.set_value(context.to_string(), window, cx));
            let focus = recorder.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window, keys);
        save(cx, window, editor);
    }

    /// Hand check PR2-1: a second binding on ⌃⌥W in the same context, for another action. The
    /// form warns of the clash, saving keeps both, and both rows show ⚠.
    #[gpui_kit::test]
    fn a_second_binding_of_the_same_keys_keeps_the_first(cx: &mut TestAppContext) {
        let scratch = Scratch::new("form-same-keys");
        let (window, editor) = open(cx, &scratch);
        create(
            cx,
            window,
            &editor,
            "keymap_editor_test::CloseAll",
            "ctrl-alt-w",
            "Workspace",
        );
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.create(&super::CreateBinding, window, cx)
            });
            let form = editor.read(cx).form.clone().unwrap();
            let (context_field, recorder) = {
                let form = form.read(cx);
                (form.context.clone(), form.keys.clone())
            };
            context_field.update(cx, |c, cx| c.set_value("Workspace", window, cx));
            recorder.update(cx, |r, cx| r.set_keystrokes("ctrl-alt-w", cx));
            let (same, _) = form.read(cx).clashes(cx);
            assert_eq!(same, ["keymap editor test: close all"]);
            editor.update(cx, |editor, cx| editor.close_form(window, cx));
        })
        .unwrap();
        create(
            cx,
            window,
            &editor,
            "keymap_editor_test::Other",
            "ctrl-alt-w",
            "Workspace",
        );
        cx.update(|cx| {
            let editor = editor.read(cx);
            let rows: Vec<_> = editor
                .rows
                .iter()
                .filter(|row| row.keystrokes.as_ref() == "ctrl-alt-w")
                .collect();
            assert_eq!(rows.len(), 2, "{rows:?}");
            assert!(rows.iter().all(|row| editor.row_conflicts(row)));
            assert_eq!(
                runs(cx, "ctrl-alt-w"),
                ["keymap_editor_test::Other", "keymap_editor_test::CloseAll"]
            );
        });
    }

    /// A canvas key can be pointed at another command: its row shows the command's id as its
    /// arguments, and a new id is saved as the user's binding of the key in place of the default.
    #[gpui_kit::test]
    fn a_canvas_key_can_run_another_command(cx: &mut TestAppContext) {
        let scratch = Scratch::new("form-canvas-id");
        let (window, editor) = open(cx, &scratch);
        edit(cx, window, &editor, "board::Command");
        cx.update_window(window, |_, window, cx| {
            let form = editor.read(cx).form.clone().unwrap();
            let args = form.read(cx).args.clone();
            assert_eq!(args.read(cx).value().as_ref(), r#"{"id":"tool.rectangle"}"#);
            args.update(cx, |a, cx| {
                a.set_value(r#"{"id": "tool.ellipse"}"#, window, cx)
            });
        })
        .unwrap();
        save(cx, window, &editor);
        cx.update(|cx| {
            let editor = editor.read(cx);
            let refused = editor.form.as_ref().map(|form| form.read(cx).error.clone());
            assert_eq!(refused, None, "the form refused");
            assert_eq!(canvas_runs(cx, "r"), ["tool.ellipse"]);
            let sources: Vec<_> = editor
                .rows
                .iter()
                .filter(|row| row.keystrokes.as_ref() == "r")
                .map(|row| (row.args.clone().unwrap_or_default(), row.source))
                .collect();
            assert!(
                sources
                    .iter()
                    .any(|(args, source)| args.contains("tool.rectangle")
                        && matches!(source, super::Source::Removed { .. })),
                "{sources:?}"
            );
            assert!(
                !sources
                    .iter()
                    .any(|(_, source)| matches!(source, super::Source::Default(_)))
            );
        });
    }

    /// A form opened before keymap.json changed on disk never changes another binding: the
    /// place in the file its row came from may hold another binding now.
    #[gpui_kit::test]
    fn a_form_opened_before_the_file_changed_changes_nothing_else(cx: &mut TestAppContext) {
        let scratch = Scratch::new("form-stale");
        std::fs::create_dir_all(&scratch.0).unwrap();
        std::fs::write(
            scratch.file(),
            r#"[{"context": "Workspace", "bindings": {"ctrl-alt-a": "keymap_editor_test::Other"}}]"#,
        )
        .unwrap();
        let (window, editor) = open(cx, &scratch);
        edit(cx, window, &editor, "keymap_editor_test::Other");
        // Another program writes the file, and the editor reads it (its look once a second).
        let by_hand = r#"[{"context": "Workspace", "bindings": {"ctrl-alt-z": "keymap_editor_test::ToggleDock"}}]"#;
        std::fs::write(scratch.file(), by_hand).unwrap();
        cx.update(|cx| crate::keymap::store(cx).update(cx, |s, cx| s.reload(cx)));
        cx.run_until_parked();
        cx.simulate_keystrokes(window, "ctrl-alt-q");
        save(cx, window, &editor);
        cx.update(|cx| {
            let form = editor.read(cx).form.clone().expect("the form stays open");
            assert!(form.read(cx).error.is_some());
            assert_eq!(runs(cx, "ctrl-alt-z"), ["keymap_editor_test::ToggleDock"]);
            assert!(runs(cx, "ctrl-alt-q").is_empty());
        });
        assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), by_hand);
    }

    /// The form's keys field records every key but plain ⎋ and ↩: ⎋ stops recording, and a
    /// second ⎋ cancels the form; ↩ saves it.
    #[gpui_kit::test]
    fn escape_and_enter_end_the_recording(cx: &mut TestAppContext) {
        let scratch = Scratch::new("form-escape-enter");
        let (window, editor) = open(cx, &scratch);
        edit(cx, window, &editor, "keymap_editor_test::ToggleDock");
        cx.simulate_keystrokes(window, "escape");
        assert_eq!(recorded(cx, &editor), "cmd-j", "⎋ is not recorded");
        cx.simulate_keystrokes(window, "escape");
        cx.update(|cx| assert!(editor.read(cx).form.is_none(), "a second ⎋ cancels"));
        assert!(!scratch.file().exists(), "nothing is written");

        edit(cx, window, &editor, "keymap_editor_test::ToggleDock");
        cx.simulate_keystrokes(window, "ctrl-alt-j enter");
        cx.update(|cx| {
            assert!(editor.read(cx).form.is_none(), "↩ saves");
            assert_eq!(runs(cx, "ctrl-alt-j"), ["keymap_editor_test::ToggleDock"]);
        });
    }

    gpui_kit::actions!(keymap_editor_test, [CloseTab]);

    /// A stand-in for the workspace: the editor in a tab that can be switched away, ⌘W bound in
    /// `Workspace`, and the keyboard taken back when nothing holds it, as the workspace does.
    struct Tabs {
        focus: gpui_kit::FocusHandle,
        editor: Entity<KeymapEditor>,
        shown: bool,
        closed: usize,
        /// How many times keymap.json was asked for.
        opened: usize,
    }

    impl gpui_kit::Render for Tabs {
        fn render(
            &mut self,
            window: &mut gpui_kit::Window,
            cx: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            use gpui_kit::{InteractiveElement as _, ParentElement as _, Styled as _};
            if window.focused(cx).is_none() {
                self.focus.focus(window, cx);
            }
            gpui_kit::div()
                .key_context("Workspace")
                .track_focus(&self.focus)
                .size_full()
                .on_action(cx.listener(|this, _: &CloseTab, _, _| this.closed += 1))
                .on_action(
                    cx.listener(|this, _: &crate::keymap::OpenKeymapFile, _, _| this.opened += 1),
                )
                .children(self.shown.then(|| self.editor.clone()))
        }
    }

    /// The keymap editor in [`Tabs`], in an active window, with the app's ⌘W.
    fn open_in_tabs(
        cx: &mut TestAppContext,
        scratch: &Scratch,
    ) -> (AnyWindowHandle, Entity<Tabs>, Entity<KeymapEditor>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.bind_keys([KeyBinding::new("cmd-w", CloseTab, Some("Workspace"))]);
            crate::keymap::init_at(scratch.file(), cx);
        });
        let (window, tabs) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                let editor = cx.new(|cx| KeymapEditor::new(window, cx));
                cx.new(|cx| Tabs {
                    focus: cx.focus_handle(),
                    editor,
                    shown: true,
                    closed: 0,
                    opened: 0,
                })
            })
            .unwrap()
        });
        cx.update_window(window, |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        let editor = cx.update(|cx| tabs.read(cx).editor.clone());
        (window, tabs, editor)
    }

    /// ⌘E in the keymap editor opens keymap.json, through the editor's own action: the Sawit
    /// menu's Open Keymap File has no key.
    #[gpui_kit::test]
    fn cmd_e_in_the_editor_opens_the_keymap_file(cx: &mut TestAppContext) {
        let scratch = Scratch::new("cmd-e");
        let (window, tabs, editor) = open_in_tabs(cx, &scratch);
        cx.update_window(window, |_, window, cx| {
            let focus = editor.read(cx).focus.clone();
            window.focus(&focus, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window, "cmd-e");
        cx.update(|cx| assert_eq!(tabs.read(cx).opened, 1));
    }

    /// A recorder whose tab is switched away while it has the keyboard lets go of it: the
    /// window's keys (⌘W) work again instead of going into the hidden field.
    #[gpui_kit::test]
    fn a_hidden_recorder_lets_go_of_the_keyboard(cx: &mut TestAppContext) {
        let scratch = Scratch::new("hidden-recorder");
        let (window, tabs, editor) = open_in_tabs(cx, &scratch);
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.toggle_keystroke_search(&super::ToggleKeystrokeSearch, window, cx)
            })
        })
        .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window, "cmd-k");
        let searched = |cx: &mut TestAppContext| {
            cx.update(|cx| editor.read(cx).keys_filter.read(cx).keystrokes())
        };
        assert_eq!(searched(cx), "cmd-k");
        // Another tab shows.
        cx.update(|cx| {
            tabs.update(cx, |tabs, cx| {
                tabs.shown = false;
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.simulate_keystrokes(window, "cmd-w");
        assert_eq!(searched(cx), "cmd-k", "the hidden field took ⌘W");
        cx.update(|cx| assert_eq!(tabs.read(cx).closed, 1));
    }

    /// The form warns when the keys and another binding in the context are a chord and its first
    /// keys: ⌘K bound alone beside ⌘K W, or ⌘J X beside ⌘J.
    #[gpui_kit::test]
    fn the_form_warns_of_a_chord_on_bound_keys(cx: &mut TestAppContext) {
        let scratch = Scratch::new("form-chords");
        let (window, editor) = open(cx, &scratch);
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.create(&super::CreateBinding, window, cx)
            });
            let form = editor.read(cx).form.clone().unwrap();
            let (context, recorder) = {
                let form = form.read(cx);
                (form.context.clone(), form.keys.clone())
            };
            context.update(cx, |c, cx| c.set_value("Workspace", window, cx));
            recorder.update(cx, |r, cx| r.set_keystrokes("cmd-k", cx));
            let (longer, shorter) = form.read(cx).chord_clashes(cx);
            assert_eq!(longer, ["⌘K W (keymap editor test: close all)"]);
            assert!(shorter.is_empty(), "{shorter:?}");
            recorder.update(cx, |r, cx| r.set_keystrokes("cmd-j x", cx));
            let (longer, shorter) = form.read(cx).chord_clashes(cx);
            assert!(longer.is_empty(), "{longer:?}");
            assert_eq!(shorter, ["⌘J (keymap editor test: toggle dock)"]);
            // Another context doesn't count.
            context.update(cx, |c, cx| c.set_value("Editor", window, cx));
            assert_eq!(form.read(cx).chord_clashes(cx), (Vec::new(), Vec::new()));
        })
        .unwrap();
    }
}
