//! The one question Sawit asks before it removes, closes, stops, overwrites or discards something
//! (backlog item 13). It names the thing ("Drop database **shop_dev**?"), says what goes with it,
//! and answers with a red button that says what it does ("Drop Database", never "OK").
//!
//! It is a dialog in the window (gpui-component's dialog layer), not the system alert
//! `window.prompt` shows: the alert's first button takes Return, and it has no red button and no
//! field to type in. Here **Cancel** is the default: it has the keyboard when the question opens,
//! so Return answers Cancel (from any button), and Escape or a click beside the question cancels
//! too. Only a click on the red button, or Space once the user has tabbed to it, goes ahead. For the
//! worst actions (drop a database, delete an instance that holds data) the red button stays off
//! until the user types the thing's name ([`Confirm::type_to_confirm`]); Return in that field
//! goes ahead once the name matches.
//!
//! The question covers the whole window, so the browser's page and the canvas's web views step
//! back behind it ([`crate::native_cover`]) as for any menu. When it closes, the keyboard goes back
//! to where it was before it opened (the dialog layer remembers it).
//!
//! Non-destructive questions (Save / Don't Save, notices with OK) stay on `window.prompt`.

use std::cell::{Cell, RefCell};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context as TaskContext, Poll};

use futures::channel::oneshot;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// The question's width.
const WIDTH: Pixels = px(420.);
/// How many of [`Confirm::items`] are listed; the rest are counted.
const LISTED: usize = 6;

/// A question to ask before something destructive. Build it, then [`ask`](Self::ask).
#[derive(Clone, Debug, Default)]
pub struct Confirm {
    pub(crate) title: SharedString,
    /// The thing, shown in bold where the title names it, and what
    /// [`type_to_confirm`](Self::type_to_confirm) asks for.
    pub(crate) thing: Option<SharedString>,
    pub(crate) detail: Option<SharedString>,
    /// What else goes (the running processes a close stops), one per line.
    pub(crate) items: Vec<SharedString>,
    /// The red button: a specific verb ("Drop Database").
    pub(crate) verb: SharedString,
    /// A second red button, left of the first, for a stronger way to do it ("Delete with
    /// Dependents"): [`Answer::choice`] says which was pressed.
    pub(crate) second: Option<SharedString>,
    /// Whether the red button waits for the thing's name.
    pub(crate) typed: bool,
}

impl Confirm {
    /// A question titled `title` ("Drop database shop_dev?") whose red button says `verb`.
    pub fn new(title: impl Into<SharedString>, verb: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            verb: verb.into(),
            ..Self::default()
        }
    }

    /// The thing the question is about: bold where the title names it.
    pub fn thing(mut self, thing: impl Into<SharedString>) -> Self {
        self.thing = Some(thing.into());
        self
    }

    /// What happens, below the title ("This deletes all 14 tables.").
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into()).filter(|d| !d.is_empty());
        self
    }

    /// What else goes, listed below the detail (the first few, and how many more).
    pub fn items<S: Into<SharedString>>(mut self, items: impl IntoIterator<Item = S>) -> Self {
        self.items = items.into_iter().map(Into::into).collect();
        self
    }

    /// Adds a second red button, a stronger way to do it ("Delete with Dependents").
    pub fn second(mut self, verb: impl Into<SharedString>) -> Self {
        self.second = Some(verb.into());
        self
    }

    /// Makes the user type the thing's name before the red button turns on: for the worst actions.
    /// Without a [`thing`](Self::thing) it does nothing.
    pub fn type_to_confirm(mut self) -> Self {
        self.typed = true;
        self
    }

    /// The name the user must type, when there is one.
    pub fn typed_name(&self) -> Option<&SharedString> {
        self.thing.as_ref().filter(|_| self.typed)
    }

    /// The lines listed under the detail: at most [`LISTED`], then "and N more".
    pub fn listed(&self) -> Vec<SharedString> {
        let mut lines: Vec<SharedString> = self.items.iter().take(LISTED).cloned().collect();
        if self.items.len() > LISTED {
            lines.push(format!("and {} more", self.items.len() - LISTED).into());
        }
        lines
    }

    /// Opens the question in `window`. The answer is true only when the user chose the red
    /// button; Cancel, Return, Escape, a click beside it, or the window going answer false.
    pub fn ask(self, window: &mut Window, cx: &mut App) -> Answer {
        let (tx, rx) = oneshot::channel();
        let view = cx.new(|cx| ConfirmView::new(self, tx, window, cx));
        #[cfg(test)]
        cx.default_global::<testing::Asked>()
            .0
            .push((window.window_handle(), view.downgrade()));
        let first = Cell::new(true);
        let ok_view = view.clone();
        let close_view = view.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                view.read(cx).initial_focus(cx).focus(window, cx);
            }
            let ok_view = ok_view.clone();
            let close_view = close_view.clone();
            dialog
                .w(WIDTH)
                .keyboard(true)
                .overlay_closable(true)
                .close_button(false)
                .child(view.clone())
                // Return outside a button: Cancel, unless it is in the name field and the name
                // matches.
                .on_ok(move |_, window, cx| ok_view.update(cx, |v, cx| v.on_return(window, cx)))
                .on_close(move |_, _, cx| close_view.update(cx, |v, _| v.answer(None)))
        });
        Answer(rx)
    }
}

/// The user's answer: true to go ahead.
pub struct Answer(oneshot::Receiver<Option<usize>>);

impl Answer {
    /// Which red button was pressed: 0 for the verb, 1 for [`Confirm::second`]; `None` when the
    /// user cancelled.
    pub async fn choice(self) -> Option<usize> {
        self.0.await.ok().flatten()
    }
}

impl Future for Answer {
    type Output = bool;

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<bool> {
        Pin::new(&mut self.0)
            .poll(cx)
            .map(|r| matches!(r, Ok(Some(_))))
    }
}

/// The question's body and its state: the field for the name, and the answer, sent once.
pub struct ConfirmView {
    confirm: Confirm,
    answer: Rc<RefCell<Option<oneshot::Sender<Option<usize>>>>>,
    cancel_focus: FocusHandle,
    go_focus: FocusHandle,
    field: Option<Entity<InputState>>,
    _subscription: Option<Subscription>,
}

impl ConfirmView {
    fn new(
        confirm: Confirm,
        tx: oneshot::Sender<Option<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = confirm.typed_name().map(|name| {
            let name = name.clone();
            cx.new(|cx| InputState::new(window, cx).placeholder(name))
        });
        let subscription = field.as_ref().map(|field| {
            cx.subscribe(field, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            })
        });
        Self {
            confirm,
            answer: Rc::new(RefCell::new(Some(tx))),
            cancel_focus: cx.focus_handle(),
            go_focus: cx.focus_handle(),
            field,
            _subscription: subscription,
        }
    }

    /// Where the keyboard goes when the question opens: the name field when there is one, else
    /// Cancel.
    pub fn initial_focus(&self, cx: &App) -> FocusHandle {
        match &self.field {
            Some(field) => field.read(cx).focus_handle(cx),
            None => self.cancel_focus.clone(),
        }
    }

    /// Whether the red button is on: always, or once the typed name matches.
    pub fn can_go(&self, cx: &App) -> bool {
        match (&self.field, self.confirm.typed_name()) {
            (Some(field), Some(name)) => field.read(cx).value().trim() == name.trim(),
            _ => true,
        }
    }

    /// Whether the question is still waiting for its answer (for tests).
    #[cfg(test)]
    pub fn waiting(&self) -> bool {
        self.answer.borrow().is_some()
    }

    /// Sends the answer (the red button pressed, or `None`), once: later calls do nothing.
    fn answer(&mut self, go: Option<usize>) {
        if let Some(tx) = self.answer.borrow_mut().take() {
            let _ = tx.send(go);
        }
    }

    /// Return reached the dialog (not a button): goes ahead only from the name field once it
    /// matches; else it is Cancel. Says whether the dialog closes.
    fn on_return(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let in_field = self
            .field
            .as_ref()
            .is_some_and(|f| f.read(cx).focus_handle(cx).is_focused(window));
        if in_field {
            if !self.can_go(cx) {
                return false;
            }
            self.answer(Some(0));
        } else {
            self.answer(None);
        }
        true
    }

    pub(crate) fn go(&mut self, which: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_go(cx) {
            return;
        }
        self.answer(Some(which));
        window.close_dialog(cx);
    }

    pub(crate) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.answer(None);
        window.close_dialog(cx);
    }

    /// The title, with the thing in bold where it names it.
    fn title_text(&self) -> StyledText {
        let title = self.confirm.title.clone();
        let bold = self
            .confirm
            .thing
            .as_ref()
            .filter(|t| !t.is_empty())
            .and_then(|t| title.find(t.as_ref()).map(|at| at..at + t.len()));
        let text = StyledText::new(title);
        match bold {
            Some(range) => text.with_highlights([(
                range,
                HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
            )]),
            None => text,
        }
    }
}

impl Render for ConfirmView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let can_go = self.can_go(cx);
        let listed = self.confirm.listed();
        let typed = self.confirm.typed_name().cloned();
        v_flex()
            .id("confirm")
            .debug_selector(|| "confirm".into())
            // Space presses the button that has the keyboard, as on any Mac. Return never goes
            // ahead from a button: the dialog takes it as Cancel ([`Self::on_return`]).
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key != "space" || e.keystroke.modifiers.modified() {
                    return;
                }
                if this.go_focus.is_focused(window) {
                    cx.stop_propagation();
                    this.go(0, window, cx);
                } else if this.cancel_focus.is_focused(window) {
                    cx.stop_propagation();
                    this.cancel(window, cx);
                }
            }))
            .gap_3()
            .w_full()
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title_text()),
            )
            .children(self.confirm.detail.clone().map(|detail| {
                // A long detail scrolls, so the buttons below it stay on screen.
                div()
                    .id("confirm-detail")
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(detail)
            }))
            .when(!listed.is_empty(), |el| {
                el.child(
                    v_flex()
                        .gap_0p5()
                        .text_sm()
                        .font_family(theme.mono_font_family.clone())
                        .children(listed.into_iter().map(|line| {
                            div()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(line)
                        })),
                )
            })
            .when_some(self.field.clone().zip(typed), |el, (field, name)| {
                el.child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().child(format!("Type {name} to confirm.")))
                        .child(Input::new(&field)),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .pt_1()
                    .child(
                        Button::new("confirm-cancel")
                            .label("Cancel")
                            .track_focus(&self.cancel_focus)
                            .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
                    )
                    .children(self.confirm.second.clone().map(|second| {
                        Button::new("confirm-second")
                            .danger()
                            .label(second)
                            .disabled(!can_go)
                            .on_click(cx.listener(|this, _, window, cx| this.go(1, window, cx)))
                    }))
                    .child(
                        Button::new("confirm-go")
                            .danger()
                            .label(self.confirm.verb.clone())
                            .track_focus(&self.go_focus)
                            .disabled(!can_go)
                            .on_click(cx.listener(|this, _, window, cx| this.go(0, window, cx))),
                    ),
            )
    }
}

#[cfg(test)]
pub mod testing;
#[cfg(test)]
mod tests;
