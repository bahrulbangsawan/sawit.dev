//! The Annotate panel: one row per element the user picked, with a comment,
//! an intent and a screenshot, grouped by page. `BrowserView` owns the flow
//! around it (pick, capture, markers, copy); the panel owns the rows.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::annotations::{Annotation, Intent, Rect, page_key};

use super::panel::note_timer;

/// What the panel asks of the browser.
pub enum AnnotateEvent {
    /// Flash this selector on the page.
    Show(String),
    /// A row was added or removed: the page's markers need redrawing.
    Changed,
    Copy,
    Send,
}

struct Row {
    annotation: Annotation,
    comment: Entity<TextareaState>,
}

pub struct AnnotatePanel {
    rows: Vec<Row>,
    /// The page on show, whose group comes first.
    current: String,
    include_errors: bool,
    note: Option<SharedString>,
    _note_timer: Option<Task<()>>,
}

impl EventEmitter<AnnotateEvent> for AnnotatePanel {}

impl AnnotatePanel {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            rows: Vec::new(),
            current: String::new(),
            include_errors: true,
            note: None,
            _note_timer: None,
        }
    }

    /// Appends `a` and moves the keyboard to its comment field.
    pub fn add(&mut self, a: Annotation, window: &mut Window, cx: &mut Context<Self>) {
        let comment =
            cx.new(|cx| TextareaState::new(window, cx).placeholder("What should change?"));
        let handle = comment.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        self.rows.push(Row {
            annotation: a,
            comment,
        });
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
    }

    /// The rows, with the comments as typed so far.
    pub fn annotations(&self, cx: &App) -> Vec<Annotation> {
        self.rows
            .iter()
            .map(|r| Annotation {
                comment: r.comment.read(cx).value().to_string(),
                ..r.annotation.clone()
            })
            .collect()
    }

    /// `(n, selector)` of the rows on the page at `url` (hash ignored).
    pub fn markers_for(&self, url: &str) -> Vec<(u32, String)> {
        let key = page_key(url);
        self.rows
            .iter()
            .filter(|r| page_key(&r.annotation.url) == key)
            .map(|r| (r.annotation.n, r.annotation.element.selector.clone()))
            .collect()
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.rows.clear();
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
    }

    pub fn set_current(&mut self, url: &str, cx: &mut Context<Self>) {
        if self.current != url {
            self.current = url.to_string();
            cx.notify();
        }
    }

    /// Shows `text` in the footer for a few seconds.
    pub fn set_note(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.note = Some(text.into());
        self._note_timer = Some(note_timer(cx, |this: &mut Self| this.note = None));
        cx.notify();
    }

    pub fn include_errors(&self) -> bool {
        self.include_errors
    }

    fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.rows.len() {
            self.rows.remove(ix);
            cx.emit(AnnotateEvent::Changed);
            cx.notify();
        }
    }

    fn row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[ix];
        let a = &row.annotation;
        let theme = cx.theme();
        let selector = a.element.selector.clone();
        let intent = a.intent;
        let shot = a.include_screenshot;
        let partly = a.element.off_screen;
        v_flex()
            .gap_1p5()
            .p_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(20.))
                            .h(px(20.))
                            .px_1()
                            .rounded_full()
                            .bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .text_xs()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(a.n.to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .font_family(theme.mono_font_family.clone())
                            .child(a.element.label.clone()),
                    ),
            )
            .when(partly, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("(partly off-screen)"),
                )
            })
            .child(Textarea::new(&row.comment).h(px(60.)).aria_label("Comment"))
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .children(Intent::ALL.into_iter().enumerate().map(|(j, i)| {
                        Button::new(("annotate-intent", ix * 10 + j))
                            .xsmall()
                            .label(intent_name(i))
                            .selected(intent == i)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if let Some(r) = this.rows.get_mut(ix) {
                                    r.annotation.intent = i;
                                    cx.notify();
                                }
                            }))
                    })),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new(("annotate-shot", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Camera)
                            .selected(shot)
                            .tooltip("Include the screenshot")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if let Some(r) = this.rows.get_mut(ix) {
                                    r.annotation.include_screenshot =
                                        !r.annotation.include_screenshot;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new(("annotate-show", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Eye)
                            .tooltip("Show")
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.emit(AnnotateEvent::Show(selector.clone()));
                            })),
                    )
                    .child(
                        Button::new(("annotate-delete", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Trash)
                            .tooltip("Delete")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove(ix, cx);
                            })),
                    ),
            )
    }
}

impl Render for AnnotatePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items: Vec<Annotation> = self.rows.iter().map(|r| r.annotation.clone()).collect();
        let groups = group_order(&self.current, &items);
        let muted = cx.theme().muted_foreground;
        let list = if self.rows.is_empty() {
            div()
                .text_sm()
                .text_color(muted)
                .child("Click an element on the page to annotate it. Esc stops picking.")
                .into_any_element()
        } else {
            let mut sections: Vec<AnyElement> = Vec::new();
            for key in groups {
                let (path, host) = split_url(&key);
                let mut rows: Vec<AnyElement> = Vec::new();
                for ix in 0..self.rows.len() {
                    if page_key(&self.rows[ix].annotation.url) == key {
                        rows.push(self.row(ix, cx).into_any_element());
                    }
                }
                sections.push(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .id(SharedString::from(format!("annotate-page-{key}")))
                                .truncate()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(muted)
                                .tooltip(move |window, cx| {
                                    Tooltip::new(host.clone()).build(window, cx)
                                })
                                .child(path),
                        )
                        .children(rows)
                        .into_any_element(),
                );
            }
            v_flex().gap_3().children(sections).into_any_element()
        };
        let has_rows = !self.rows.is_empty();
        v_flex().gap_3().child(list).child(
            v_flex()
                .gap_2()
                .pt_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    Checkbox::new("annotate-errors")
                        .label("Include console errors")
                        .checked(self.include_errors)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.include_errors = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("annotate-copy")
                                .xsmall()
                                .icon(Lucide::Copy)
                                .label("Copy")
                                .disabled(!has_rows)
                                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                                    cx.emit(AnnotateEvent::Copy);
                                })),
                        )
                        .child(
                            Button::new("annotate-send")
                                .xsmall()
                                .primary()
                                .icon(Lucide::Terminal)
                                .label("Send to terminal")
                                .disabled(!has_rows)
                                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                                    cx.emit(AnnotateEvent::Send);
                                })),
                        )
                        .child(
                            Button::new("annotate-clear")
                                .ghost()
                                .xsmall()
                                .label("Clear")
                                .disabled(!has_rows)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.clear(cx);
                                })),
                        ),
                )
                .children(
                    self.note
                        .clone()
                        .map(|note| div().text_xs().text_color(muted).child(note)),
                ),
        )
    }
}

/// The intent as its button shows it: "Fix", "Change", "Question", "Copy".
fn intent_name(i: Intent) -> &'static str {
    match i {
        Intent::Fix => "Fix",
        Intent::Change => "Change",
        Intent::Question => "Question",
        Intent::Copy => "Copy",
    }
}

/// The element's box grown by `pad` on every side and clamped to the
/// viewport: `(x, y, width, height)`, in CSS px.
pub(super) fn padded_rect(r: &Rect, viewport: (u32, u32), pad: f64) -> (f64, f64, f64, f64) {
    let (vw, vh) = (f64::from(viewport.0), f64::from(viewport.1));
    let x0 = (r.x - pad).clamp(0., vw);
    let y0 = (r.y - pad).clamp(0., vh);
    let x1 = (r.x + r.width + pad).clamp(0., vw);
    let y1 = (r.y + r.height + pad).clamp(0., vh);
    (x0, y0, x1 - x0, y1 - y0)
}

/// Page keys: `current` first, then in the order they were first annotated.
pub(super) fn group_order(current: &str, items: &[Annotation]) -> Vec<String> {
    let current = page_key(current);
    let mut keys: Vec<String> = Vec::new();
    for a in items {
        let key = page_key(&a.url);
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_string());
        }
    }
    if let Some(at) = keys.iter().position(|k| k == current) {
        let key = keys.remove(at);
        keys.insert(0, key);
    }
    keys
}

/// A URL's path (with query) for a header, and its host for the tooltip.
fn split_url(url: &str) -> (String, String) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    match rest.find('/') {
        Some(i) => (rest[i..].to_string(), rest[..i].to_string()),
        None => ("/".to_string(), rest.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{group_order, padded_rect, split_url};
    use sawit_core::annotations::{Annotation, ElementDetails, Intent, Rect};

    fn at(url: &str, n: u32) -> Annotation {
        Annotation {
            n,
            url: url.into(),
            element: ElementDetails {
                selector: "a".into(),
                label: "a".into(),
                tag: "a".into(),
                id: None,
                classes: vec![],
                text: None,
                rect: Rect {
                    x: 0.,
                    y: 0.,
                    width: 1.,
                    height: 1.,
                },
                styles: vec![],
                source: None,
                html: None,
                off_screen: false,
            },
            comment: String::new(),
            intent: Intent::Fix,
            screenshot: None,
            include_screenshot: true,
        }
    }

    #[test]
    fn padding_clamps_to_the_viewport() {
        let r = Rect {
            x: 5.,
            y: 5.,
            width: 100.,
            height: 40.,
        };
        assert_eq!(padded_rect(&r, (393, 852), 8.), (0., 0., 113., 53.));
        let r = Rect {
            x: 380.,
            y: 100.,
            width: 50.,
            height: 10.,
        };
        let (x, _, w, _) = padded_rect(&r, (393, 852), 8.);
        assert_eq!((x, w), (372., 21.));
    }

    #[test]
    fn current_page_comes_first() {
        let items = [
            at("http://a/1", 1),
            at("http://a/2", 1),
            at("http://a/1", 2),
        ];
        assert_eq!(
            group_order("http://a/2", &items),
            ["http://a/2", "http://a/1"]
        );
        assert_eq!(
            group_order("http://a/9#x", &items),
            ["http://a/1", "http://a/2"]
        );
    }

    #[test]
    fn urls_split_into_path_and_host() {
        assert_eq!(
            split_url("http://localhost:5173/a?b=1"),
            ("/a?b=1".into(), "localhost:5173".into())
        );
        assert_eq!(split_url("http://x.dev"), ("/".into(), "x.dev".into()));
    }
}
