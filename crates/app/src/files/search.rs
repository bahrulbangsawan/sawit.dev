//! Find in Folder: a center tab that searches a folder's files for text.
//! Clicking a hit opens the file at that line.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::search::{Hit, find};

use crate::rt;
use crate::states::Placeholder;

const LIMIT: usize = 2000;
const ROW_HEIGHT: f32 = 24.;

pub enum SearchEvent {
    Open { path: PathBuf, line: u32 },
}

impl EventEmitter<SearchEvent> for FolderSearch {}

pub struct FolderSearch {
    pub dir: PathBuf,
    pub query: Entity<InputState>,
    hits: Vec<Hit>,
    /// Query the hits are for.
    searched: Option<String>,
    loading: bool,
    error: Option<String>,
    scroll: UniformListScrollHandle,
    _subscription: Subscription,
}

impl FolderSearch {
    pub fn key(dir: &Path) -> String {
        format!("search\u{0}{}", dir.display())
    }

    pub fn new(dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search text, then press Enter"));
        let subscription = cx.subscribe(&query, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                this.search(cx);
            }
        });
        query.update(cx, |s, cx| s.focus(window, cx));
        Self {
            dir,
            query,
            hits: Vec::new(),
            searched: None,
            loading: false,
            error: None,
            scroll: UniformListScrollHandle::new(),
            _subscription: subscription,
        }
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        let text = self.query.read(cx).value().to_string();
        if text.is_empty() {
            return;
        }
        let dir = self.dir.clone();
        let query = text.clone();
        self.loading = true;
        cx.notify();
        let rx = rt::spawn(cx, async move { find(&dir, &query, LIMIT).await });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(hits) => {
                        this.hits = hits;
                        this.error = None;
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                this.searched = Some(text);
                cx.notify();
            });
        })
        .detach();
    }

    fn render_hit(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(hit) = self.hits.get(ix) else {
            return div().into_any_element();
        };
        let rel = hit
            .path
            .strip_prefix(&self.dir)
            .unwrap_or(&hit.path)
            .to_string_lossy()
            .into_owned();
        let (path, line) = (hit.path.clone(), hit.line);
        h_flex()
            .id(("search-hit", ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_3()
            .gap_2()
            .text_sm()
            .cursor_pointer()
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(SearchEvent::Open {
                    path: path.clone(),
                    line,
                })
            }))
            .child(crate::file_icons::file_icon(&hit.path, cx))
            .child(
                div()
                    .flex_none()
                    .max_w(px(320.))
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{rel}:{line}")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(hit.text.clone()),
            )
            .into_any_element()
    }
}

impl Render for FolderSearch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.hits.len();
        let summary = match &self.searched {
            Some(_) if count >= LIMIT => format!("First {LIMIT} matches"),
            Some(_) => format!("{count} match{}", if count == 1 { "" } else { "es" }),
            None => String::new(),
        };
        let list: AnyElement = if let Some(e) = &self.error {
            Placeholder::error("Search failed")
                .detail(e.clone())
                .into_any_element()
        } else if self.loading && count == 0 {
            Placeholder::loading("Searching…").into_any_element()
        } else if let Some(q) = self.searched.as_ref().filter(|_| count == 0) {
            Placeholder::new(Lucide::Search, "No matches")
                .detail(format!("Nothing in this folder contains “{q}”."))
                .into_any_element()
        } else if count == 0 {
            Placeholder::new(Lucide::Search, "Find in Folder")
                .detail(self.dir.display().to_string())
                .into_any_element()
        } else {
            uniform_list(
                "search-hits",
                count,
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range.map(|ix| this.render_hit(ix, cx)).collect()
                }),
            )
            .track_scroll(&self.scroll)
            .size_full()
            .into_any_element()
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().flex_1().child(Input::new(&self.query).small()))
                    .when(!summary.is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(summary),
                        )
                    }),
            )
            .child(div().flex_1().min_h_0().child(list))
    }
}
