//! ⌘F in a file: the find bar searches the code, or the Markdown preview when the preview shows
//! alone or has the keyboard beside the code. In the code it is the editor's own search (its
//! highlights, Match Case, and Replace and Replace All in a file that isn't read-only); in the
//! preview, the text the preview shows, with every option. An HTML, SVG or diagram preview has no
//! text to search: the bar says so.

use gpui_kit::component::input::EditorState;
use gpui_kit::component::text::{RangeHighlight, TextViewState};
use gpui_kit::*;
use sawit_core::files::FileKind;
use sawit_core::find::{FindState, find_all, matcher};

use super::{FileEditor, ViewMode};
use crate::find::{FindEvent, Finder, Supports};

/// What the bar searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Code,
    Preview,
    /// A preview with no text (HTML, SVG, a diagram).
    Nothing,
}

/// A file's find: its bar, what it searches, and the preview's matches.
pub(super) struct EditorFind {
    pub(super) finder: Finder,
    target: Target,
    /// The preview's matches, as ranges of the text it shows.
    preview_matches: Vec<std::ops::Range<usize>>,
    preview_state: FindState,
}

impl Default for EditorFind {
    fn default() -> Self {
        Self {
            finder: Finder::default(),
            target: Target::Code,
            preview_matches: Vec::new(),
            preview_state: FindState::default(),
        }
    }
}

/// How long a preview's new text takes to parse, before it is searched again.
const REPARSED: std::time::Duration = std::time::Duration::from_millis(250);

/// The preview's matches, and the current one, as the code's search marks them.
const FOUND: Hsla = Hsla {
    h: 0.12,
    s: 0.9,
    l: 0.5,
    a: 0.35,
};
const FOUND_CURRENT: Hsla = Hsla {
    h: 0.08,
    s: 0.95,
    l: 0.55,
    a: 0.8,
};

impl FileEditor {
    /// What ⌘F searches now: the preview when it shows alone, or beside the code with the
    /// keyboard in it; else the code.
    fn find_target(&self, window: &Window, cx: &App) -> Target {
        let mode = if self.previewable() {
            self.mode
        } else {
            ViewMode::Code
        };
        let preview = match mode {
            ViewMode::Code => false,
            ViewMode::Preview => true,
            ViewMode::Split => self
                .preview_view
                .as_ref()
                .is_some_and(|p| p.read(cx).focus_handle().contains_focused(window, cx)),
        };
        match (preview, self.kind) {
            (false, _) if self.state.is_some() => Target::Code,
            (true, FileKind::Markdown) => Target::Preview,
            _ => Target::Nothing,
        }
    }

    /// ⌘F (⌥⌘F for `replace`): opens the bar on what [`Self::find_target`] says, its query the
    /// text selected there.
    pub(super) fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.find_target(window, cx);
        if self.find.finder.is_open() && self.find.target != target {
            self.close_find(window, cx);
        }
        self.find.target = target;
        let (supports, seed) = match target {
            Target::Code => {
                let state = self.state.as_ref().map(|s| s.read(cx));
                let seed = state.map(|s| s.selected_text().to_string());
                let supports = Supports {
                    match_case: true,
                    replace: !self.read_only,
                    ..Supports::default()
                };
                (Some(supports), seed)
            }
            Target::Preview => {
                let seed = self
                    .preview_view
                    .as_ref()
                    .map(|p| p.read(cx).selected_text());
                (Some(Supports::ALL), seed)
            }
            Target::Nothing => (None, None),
        };
        let bar = self.find.finder.open(
            supports,
            seed.as_deref(),
            replace,
            Self::on_find_event,
            window,
            cx,
        );
        let can_replace = target == Target::Code && !self.read_only;
        bar.update(cx, |bar, cx| bar.set_can_replace(can_replace, cx));
    }

    fn on_find_event(&mut self, event: &FindEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            FindEvent::Changed => self.find_again(cx),
            FindEvent::Step(forward) => self.find_step(*forward, cx),
            FindEvent::ReplaceOne | FindEvent::ReplaceAll => {
                self.replace_found(*event == FindEvent::ReplaceAll, window, cx)
            }
            FindEvent::Close => self.close_find(window, cx),
        }
    }

    /// Escape in the bar or its ✕: the highlights go, and the keyboard goes back to the code or
    /// the preview.
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = match (self.find.target, &self.state) {
            (Target::Code, Some(state)) => state.read(cx).focus_handle(cx),
            _ => self.focus.clone(),
        };
        if !self.find.finder.close(&back, window, cx) {
            cx.propagate();
            return;
        }
        if let Some(state) = &self.state {
            state.update(cx, |s, cx| s.close_search(cx));
        }
        if let Some(preview) = &self.preview_view {
            preview.update(cx, |p, cx| p.clear_range_highlights(cx));
        }
        self.find.preview_matches.clear();
        cx.notify();
    }

    /// The query or an option changed, or the preview's text did: find again.
    pub(super) fn find_again(&mut self, cx: &mut Context<Self>) {
        let Some(bar) = self.find.finder.bar().cloned() else {
            return;
        };
        let (query, options) = bar.read_with(cx, |bar, cx| (bar.query(cx), bar.options()));
        match self.find.target {
            Target::Code => {
                let Some(state) = self.state.clone() else {
                    return;
                };
                let found = state.update(cx, |s, cx| {
                    s.set_search_query(query, !options.match_case, cx);
                    // The match nearest the view, scrolled to without moving on.
                    if s.next_search_match(cx).is_some() {
                        s.previous_search_match(cx);
                    }
                    code_found(s)
                });
                bar.update(cx, |bar, cx| bar.set_found(found, None, cx));
            }
            Target::Preview => {
                let (found, problem) = match matcher(&query, options) {
                    Ok(Some(re)) => {
                        let text = self.preview_text(cx);
                        self.find.preview_matches = find_all(&text, &re);
                        let near = self.find.preview_state.current;
                        (
                            FindState::found(self.find.preview_matches.len(), near),
                            None,
                        )
                    }
                    Ok(None) => {
                        self.find.preview_matches.clear();
                        (FindState::default(), None)
                    }
                    Err(problem) => {
                        self.find.preview_matches.clear();
                        (FindState::default(), Some(problem))
                    }
                };
                self.find.preview_state = found;
                self.mark_preview(cx);
                bar.update(cx, |bar, cx| bar.set_found(found, problem, cx));
            }
            Target::Nothing => {}
        }
    }

    /// ⌘G, ⇧⌘G, ↩ and ⇧↩ in the bar, its arrows.
    pub(super) fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(bar) = self.find.finder.bar().cloned() else {
            cx.propagate();
            return;
        };
        let found = match self.find.target {
            Target::Code => {
                let Some(state) = self.state.clone() else {
                    return;
                };
                state.update(cx, |s, cx| {
                    if forward {
                        s.next_search_match(cx);
                    } else {
                        s.previous_search_match(cx);
                    }
                    code_found(s)
                })
            }
            Target::Preview => {
                if forward {
                    self.find.preview_state.step_next();
                } else {
                    self.find.preview_state.step_previous();
                }
                self.mark_preview(cx);
                self.find.preview_state
            }
            Target::Nothing => return,
        };
        bar.update(cx, |bar, cx| bar.set_found(found, None, cx));
    }

    /// Replace (`all` false: the current match, then on to the next) or Replace All, in the code
    /// of a file that isn't read-only.
    fn replace_found(&mut self, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.target != Target::Code || self.read_only {
            return;
        }
        let (Some(bar), Some(state)) = (self.find.finder.bar().cloned(), self.state.clone()) else {
            return;
        };
        let with = bar.read(cx).replacement(cx);
        let found = state.update(cx, |s, cx| {
            if all {
                s.replace_all_search_matches(&with, window, cx);
            } else {
                s.replace_current_search_match(&with, window, cx);
            }
            code_found(s)
        });
        bar.update(cx, |bar, cx| bar.set_found(found, None, cx));
    }

    /// The text the Markdown preview shows, which its highlights index.
    fn preview_text(&self, cx: &App) -> String {
        self.preview_view
            .as_ref()
            .map(|p| p.read(cx).rendered_text().as_str().to_string())
            .unwrap_or_default()
    }

    /// Highlights the preview's matches, the current one more strongly, and scrolls to it.
    fn mark_preview(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = self.preview_view.clone() else {
            return;
        };
        let current = self.find.preview_state.current;
        let marks: Vec<RangeHighlight> = (self.find.preview_matches.iter().enumerate())
            .map(|(ix, range)| {
                let color = if Some(ix) == current {
                    FOUND_CURRENT
                } else {
                    FOUND
                };
                RangeHighlight::new(range.clone(), color)
            })
            .collect();
        let reveal = current.and_then(|ix| self.find.preview_matches.get(ix).cloned());
        preview.update(cx, |p, cx| {
            let _ = p.set_range_highlights(marks, cx);
            if let Some(range) = reveal {
                let _ = p.reveal_range(range, cx);
            }
        });
    }

    /// The Markdown preview's view, made on first use; its text follows the file's.
    pub(super) fn markdown_view(&mut self, cx: &mut Context<Self>) -> Entity<TextViewState> {
        let text = self.diagrams.markdown.clone();
        match &self.preview_view {
            Some(view) => {
                if self.preview_view_text != text {
                    self.preview_view_text = text.clone();
                    view.update(cx, |v, cx| v.set_text(&text, cx));
                    if self.find.finder.is_open() && self.find.target == Target::Preview {
                        // Its text is parsed in the background: find again once it shows.
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(REPARSED).await;
                            let _ = this.update(cx, |this, cx| this.find_again(cx));
                        })
                        .detach();
                    }
                }
                view.clone()
            }
            None => {
                let view = cx.new(|cx| TextViewState::markdown(&text, cx));
                self.preview_view_text = text;
                self.preview_view = Some(view.clone());
                view
            }
        }
    }
}

/// The code's matches as the bar counts them.
fn code_found(state: &EditorState) -> FindState {
    let matcher = &state.search_session().matcher;
    FindState {
        total: matcher.len(),
        current: matcher.current(),
    }
}
