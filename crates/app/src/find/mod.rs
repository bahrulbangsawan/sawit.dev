//! The find bar: one ⌘F bar for every view that has text to search (a file's editor and its
//! Markdown preview, a PDF, the browser's page, the canvas and the terminal). The view that has
//! the keyboard opens it with ⌘F ([`Find`]), above its content, at the top right, so it never
//! covers a match. It has the query and how many it matches ("3 of 12"), Previous and Next (⇧↩ and
//! ↩ in the field, ⇧⌘G and ⌘G anywhere in the view), the Match Case, Whole Word and Regex
//! toggles where the view can match that way, and in an editable file Replace and Replace All
//! (⌥⌘F). Escape closes it and gives the keyboard back to where it was in the view.
//!
//! The bar is the view's: it shows what the view tells it ([`FindBar::set_found`]), and tells the
//! view what the user asks for ([`FindEvent`]). The view finds, highlights every match, marks the
//! current one and scrolls it into view. A view with no text says so in the bar
//! ([`FindBar::unavailable`]).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::find::{FindOptions, FindState};

mod host;
#[cfg(test)]
mod tests;

pub use host::Finder;

actions!(
    find,
    [
        /// Opens the find bar of the view with the keyboard, its query the text selected there;
        /// in an open bar, selects the query.
        Find,
        /// The next match.
        FindNext,
        /// The previous match.
        FindPrevious,
        /// Opens the find bar with its Replace field, in an editable file.
        Replace,
        /// Closes the find bar, giving the keyboard back to the view.
        CloseFind
    ]
);

/// The key context of the bar, for its own keys.
pub const CONTEXT: &str = "FindBar";

/// The find keys. ⌘F, ⌘G and ⇧⌘G are the workspace's, so each view under it that finds handles
/// them, and a field's own ⌘F (its search, which Sawit's fields leave off) gives way to them; a
/// view with its own ⌘G (the canvas groups layers with it) keeps it, and the bar's field steps with
/// ↩ and ⇧↩ there. Escape in the bar's field closes it; a field gives Escape on when it has nothing
/// of its own to close.
pub fn init(cx: &mut App) {
    let ctx = Some("Workspace");
    let bar = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("cmd-f", Find, ctx),
        KeyBinding::new("cmd-g", FindNext, ctx),
        KeyBinding::new("cmd-shift-g", FindPrevious, ctx),
        KeyBinding::new("alt-cmd-f", Replace, ctx),
        KeyBinding::new("enter", FindNext, bar),
        KeyBinding::new("shift-enter", FindPrevious, bar),
        KeyBinding::new("cmd-g", FindNext, bar),
        KeyBinding::new("cmd-shift-g", FindPrevious, bar),
        KeyBinding::new("escape", CloseFind, bar),
    ]);
}

/// What a view's find can do: the options it matches with, and whether it can replace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Supports {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub replace: bool,
    /// The canvas: the current page, or every page (the All Pages toggle).
    pub pages: bool,
}

impl Supports {
    /// Every option; no Replace.
    pub const ALL: Self = Self {
        match_case: true,
        whole_word: true,
        regex: true,
        replace: false,
        pages: false,
    };
}

/// What the user asked the view's find for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindEvent {
    /// The query or an option changed: find again.
    Changed,
    /// Previous (`false`) or Next (`true`), from the bar's buttons.
    Step(bool),
    /// Replace the current match, then go to the next.
    ReplaceOne,
    /// Replace every match.
    ReplaceAll,
    /// The close button.
    Close,
}

/// The find bar of one view.
pub struct FindBar {
    query: Entity<InputState>,
    replacement: Entity<InputState>,
    options: FindOptions,
    supports: Supports,
    /// The Replace row shows (⌥⌘F), where the view can replace.
    replacing: bool,
    /// Every page is searched, not only the one shown (the canvas).
    all_pages: bool,
    found: FindState,
    /// Why there is no count: a regular expression that doesn't parse, or a view with no text.
    problem: Option<SharedString>,
    /// The view has no text to search: the bar says so, and has no field; the bar itself takes
    /// the keyboard, for its Escape.
    unavailable: bool,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FindEvent> for FindBar {}

impl FindBar {
    /// A bar for a view that finds as `supports` says.
    pub fn new(supports: Supports, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
        let replacement = cx.new(|cx| InputState::new(window, cx).placeholder("Replace"));
        let subscriptions = vec![cx.subscribe(&query, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.emit(FindEvent::Changed);
            }
        })];
        Self {
            query,
            replacement,
            options: FindOptions::default(),
            supports,
            replacing: false,
            all_pages: false,
            found: FindState::default(),
            problem: None,
            unavailable: false,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// A bar for a view with no text: it says "Search isn't available here".
    pub fn unavailable(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut bar = Self::new(Supports::default(), window, cx);
        bar.unavailable = true;
        bar
    }

    /// ⌘F: the keyboard goes to the query, all of it selected, after `seed` (the text selected
    /// in the view, on one line) replaces it. `replace` (⌥⌘F) shows the Replace row too, where the
    /// view can replace.
    pub fn open(
        &mut self,
        seed: Option<&str>,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if replace && self.supports.replace {
            self.replacing = true;
        }
        if self.unavailable {
            window.focus(&self.focus, cx);
            return;
        }
        let seed = seed
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.contains('\n'));
        self.query.update(cx, |input, cx| {
            if let Some(seed) = seed {
                input.set_value(seed.to_string(), window, cx);
            }
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        if seed.is_some() {
            cx.emit(FindEvent::Changed);
        }
        cx.notify();
    }

    /// The query as typed.
    pub fn query(&self, cx: &App) -> String {
        self.query.read(cx).value().to_string()
    }

    /// The replacement as typed.
    pub fn replacement(&self, cx: &App) -> String {
        self.replacement.read(cx).value().to_string()
    }

    pub fn options(&self) -> FindOptions {
        self.options
    }

    /// Whether every page is searched (the canvas's All Pages).
    pub fn all_pages(&self) -> bool {
        self.all_pages
    }

    /// Whether the view can replace now (an editable file), which the Replace row follows.
    pub fn set_can_replace(&mut self, can: bool, cx: &mut Context<Self>) {
        if self.supports.replace != can {
            self.supports.replace = can;
            self.replacing &= can;
            cx.notify();
        }
    }

    /// The view's matches: how many, and which is current; `problem` instead when the query
    /// can't be searched for.
    pub fn set_found(&mut self, found: FindState, problem: Option<String>, cx: &mut Context<Self>) {
        self.found = found;
        self.problem = problem.map(Into::into);
        cx.notify();
    }

    /// Whether the keyboard is in the bar.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
    }

    /// What the count shows: the problem, else "3 of 12" or "No results".
    fn status(&self, cx: &App) -> String {
        match &self.problem {
            Some(problem) => problem.to_string(),
            None => self.found.label(&self.query(cx)),
        }
    }

    fn toggle(&mut self, option: fn(&mut FindOptions) -> &mut bool, cx: &mut Context<Self>) {
        let on = option(&mut self.options);
        *on = !*on;
        cx.emit(FindEvent::Changed);
        cx.notify();
    }
}

impl Render for FindBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let row = || h_flex().w_full().justify_end().gap_1().flex_wrap();
        let frame = v_flex()
            .id("find-bar")
            .debug_selector(|| "find-bar".into())
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            // A press in the bar is the bar's: the view under it (a terminal pane) would take the
            // keyboard back.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .role(Role::Group)
            .aria_label("Find")
            .w_full()
            .flex_none()
            .px_2()
            .py_1()
            .gap_1()
            .bg(theme.tab_bar)
            .border_b_1()
            .border_color(theme.border);
        if self.unavailable {
            return frame.child(
                row()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Search isn't available here"),
                    )
                    .child(close_button(cx)),
            );
        }
        let status = self.status(cx);
        let failed = self.problem.is_some() || self.found.total == 0 && !status.is_empty();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let supports = self.supports;
        let options = self.options;
        let option = |id: &'static str, icon: Lucide, label: &'static str, on: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .selected(on)
                .accessibility_label(label)
                .tooltip(label)
        };
        frame
            .child(
                row()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(96.))
                            .max_w(px(260.))
                            .child(Input::new(&self.query).xsmall().cleanable(true)),
                    )
                    .child(
                        div()
                            .id("find-count")
                            .debug_selector(|| "find-count".into())
                            .role(Role::Status)
                            .aria_label(SharedString::from(status.clone()))
                            .min_w(px(56.))
                            .text_xs()
                            .text_color(if failed { danger } else { muted })
                            .child(status),
                    )
                    .when(supports.match_case, |el| {
                        el.child(
                            option(
                                "find-case",
                                Lucide::CaseSensitive,
                                "Match Case",
                                options.match_case,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.toggle(|o| &mut o.match_case, cx)
                                }),
                            ),
                        )
                    })
                    .when(supports.whole_word, |el| {
                        el.child(
                            option(
                                "find-word",
                                Lucide::WholeWord,
                                "Whole Word",
                                options.whole_word,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.toggle(|o| &mut o.whole_word, cx)
                                }),
                            ),
                        )
                    })
                    .when(supports.regex, |el| {
                        el.child(
                            option("find-regex", Lucide::Regex, "Regex", options.regex).on_click(
                                cx.listener(|this, _, _, cx| this.toggle(|o| &mut o.regex, cx)),
                            ),
                        )
                    })
                    .when(supports.pages, |el| {
                        el.child(
                            Button::new("find-pages")
                                .ghost()
                                .xsmall()
                                .label("All Pages")
                                .selected(self.all_pages)
                                .tooltip("Find on every page, not only this one")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.all_pages = !this.all_pages;
                                    cx.emit(FindEvent::Changed);
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        Button::new("find-previous")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::ChevronUp)
                            .accessibility_label("Previous Match")
                            .tooltip_with_action("Previous Match", &FindPrevious, Some(CONTEXT))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Step(false)))),
                    )
                    .child(
                        Button::new("find-next")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::ChevronDown)
                            .accessibility_label("Next Match")
                            .tooltip_with_action("Next Match", &FindNext, Some(CONTEXT))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Step(true)))),
                    )
                    .when(supports.replace, |el| {
                        el.child(
                            option(
                                "find-replace-toggle",
                                Lucide::Replace,
                                "Replace",
                                self.replacing,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.replacing = !this.replacing;
                                cx.notify();
                            })),
                        )
                    })
                    .child(close_button(cx)),
            )
            .when(supports.replace && self.replacing, |el| {
                el.child(
                    row()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(96.))
                                .max_w(px(260.))
                                .child(Input::new(&self.replacement).xsmall()),
                        )
                        .child(
                            Button::new("find-replace")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::Replace)
                                .label("Replace")
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(FindEvent::ReplaceOne)),
                                ),
                        )
                        .child(
                            Button::new("find-replace-all")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::ReplaceAll)
                                .label("Replace All")
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(FindEvent::ReplaceAll)),
                                ),
                        ),
                )
            })
    }
}

/// The bar's ✕, with Escape in its tooltip.
fn close_button(cx: &mut Context<FindBar>) -> Button {
    Button::new("find-close")
        .ghost()
        .xsmall()
        .icon(Lucide::X)
        .accessibility_label("Close Find")
        .tooltip_with_action("Close", &CloseFind, Some(CONTEXT))
        .on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Close)))
}
