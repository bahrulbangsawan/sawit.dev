//! ⌘F in a PDF: the find bar under the viewer's toolbar searches the pages' text layer
//! ([`super::pdf_text`]), highlights every match on the pages drawn, the current one more strongly,
//! and scrolls its page to it.

use gpui_kit::*;
use sawit_core::find::{FindState, matcher};

use super::pdf::PdfView;
use super::pdf_text::{PdfHit, find_in_pdf};
use crate::find::{FindEvent, Finder, Supports};
use crate::rt;

/// The matches, and the current one, as the editor marks them.
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
    a: 0.55,
};

/// A PDF's find: its bar, and what it found.
#[derive(Default)]
pub(super) struct PdfFind {
    pub(super) finder: Finder,
    hits: Vec<PdfHit>,
    state: FindState,
    /// Counts the searches, so a slow one's answer never replaces a newer one's.
    generation: u64,
}

impl PdfView {
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.finder.open(
            Some(Supports::ALL),
            None,
            false,
            Self::on_find_event,
            window,
            cx,
        );
    }

    fn on_find_event(&mut self, event: &FindEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            FindEvent::Changed => self.find_again(cx),
            FindEvent::Step(forward) => self.find_step(*forward, cx),
            FindEvent::Close => self.close_find(window, cx),
            FindEvent::ReplaceOne | FindEvent::ReplaceAll => {}
        }
    }

    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.focus.clone();
        if self.find.finder.close(&focus, window, cx) {
            self.find = PdfFind::default();
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    pub(super) fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.find.finder.is_open() {
            cx.propagate();
            return;
        }
        if forward {
            self.find.state.step_next();
        } else {
            self.find.state.step_previous();
        }
        self.show_found(cx);
    }

    /// Searches the text layer again, off the main thread, for a new query or option.
    fn find_again(&mut self, cx: &mut Context<Self>) {
        let Some(bar) = self.find.finder.bar().cloned() else {
            return;
        };
        let (query, options) = bar.read_with(cx, |bar, cx| (bar.query(cx), bar.options()));
        self.find.generation += 1;
        let generation = self.find.generation;
        let regex = match matcher(&query, options) {
            Ok(Some(regex)) => regex,
            result => {
                let problem = result.err();
                self.find.hits.clear();
                self.find.state = FindState::default();
                bar.update(cx, |bar, cx| {
                    bar.set_found(FindState::default(), problem, cx)
                });
                cx.notify();
                return;
            }
        };
        let path = self.path.clone();
        let rx = rt::spawn_blocking(cx, move || find_in_pdf(&path, &regex));
        cx.spawn(async move |this, cx| {
            let found = rx.await.ok().flatten();
            let _ = this.update(cx, |this, cx| {
                if this.find.generation != generation {
                    return;
                }
                let problem = found
                    .is_none()
                    .then(|| "Search isn't available here".to_string());
                this.find.hits = found.unwrap_or_default();
                this.find.state = FindState::found(this.find.hits.len(), None);
                if let Some(bar) = this.find.finder.bar() {
                    let state = this.find.state;
                    bar.update(cx, |bar, cx| bar.set_found(state, problem, cx));
                }
                this.show_found(cx);
            });
        })
        .detach();
    }

    /// Scrolls the current match's page so the match shows, and tells the bar.
    fn show_found(&mut self, cx: &mut Context<Self>) {
        let state = self.find.state;
        if let Some(hit) = state.current.and_then(|ix| self.find.hits.get(ix)) {
            let top = hit.rects.first().map_or(0.0, |r| r.1);
            let height = self.page_height(hit.page);
            self.list.scroll_to(ListOffset {
                item_ix: hit.page,
                offset_in_item: px((top * height - 120.0).max(0.0)),
            });
        }
        if let Some(bar) = self.find.finder.bar() {
            bar.update(cx, |bar, cx| bar.set_found(state, None, cx));
        }
        cx.notify();
    }

    /// The highlights of page `page`, `width` × `height` as drawn.
    pub(super) fn page_marks(&self, page: usize, width: f32, height: f32) -> Vec<AnyElement> {
        let current = self.find.state.current;
        (self.find.hits.iter().enumerate())
            .filter(|(_, hit)| hit.page == page)
            .flat_map(|(ix, hit)| {
                let color = if Some(ix) == current {
                    FOUND_CURRENT
                } else {
                    FOUND
                };
                hit.rects.iter().map(move |&(x, y, w, h)| {
                    div()
                        .absolute()
                        .left(px(x * width))
                        .top(px(y * height))
                        .w(px(w * width))
                        .h(px(h * height))
                        .bg(color)
                        .into_any_element()
                })
            })
            .collect()
    }
}
