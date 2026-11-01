//! The Annotate panel: one card per element the user picked, with a comment, an intent and a
//! screenshot, grouped by page. Notes are written in a popover on the page itself
//! (`browser_overlay.js`); the panel is their list, kept in step both ways. `BrowserView` owns
//! the flow around it (pick, capture, markers, copy); the panel owns the rows.

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::annotations::{Annotation, Intent, Rect, page_key};

use super::panel::{self, note_timer, note_timer_for};

/// How long Undo stays after a delete or a clear.
const UNDO_FOR: Duration = Duration::from_secs(8);

/// What the panel asks of the browser.
pub enum AnnotateEvent {
    /// Flash this selector on the page.
    Show(String),
    /// Rows were added, removed or edited: the page's markers need redrawing.
    Changed,
    Copy,
    Send,
    /// Pick again (after Esc stopped it).
    Pick,
}

/// A marker on the page, with what its popover shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub n: u32,
    pub selector: String,
    pub comment: String,
    pub intent: Intent,
    pub label: String,
}

struct Row {
    annotation: Annotation,
    comment: Entity<TextareaState>,
    _changed: Subscription,
}

/// Whether the page picks, as the panel's status line says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picking {
    On,
    /// Asked for; the page isn't ready yet.
    Waiting,
    Off,
}

pub struct AnnotatePanel {
    rows: Vec<Row>,
    /// The notes as last copied, sent or read by an agent: `(url, n, comment)`.
    sent: Vec<(String, u32, String)>,
    /// The page on show, whose group comes first, and its title.
    current: String,
    title: String,
    picking: Picking,
    include_errors: bool,
    /// Console errors on the page on show, for the checkbox's count.
    errors: usize,
    note: Option<SharedString>,
    /// Rows a delete or a clear took, with their places, while Undo can bring them back.
    removed: Vec<(usize, Row)>,
    _note_timer: Option<Task<()>>,
}

impl EventEmitter<AnnotateEvent> for AnnotatePanel {}

impl AnnotatePanel {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            rows: Vec::new(),
            sent: Vec::new(),
            current: String::new(),
            title: String::new(),
            picking: Picking::Off,
            include_errors: true,
            errors: 0,
            note: None,
            removed: Vec::new(),
            _note_timer: None,
        }
    }

    /// Appends `a`. Its comment, written in the page's popover, is the row's; without one the
    /// row's comment field takes the keyboard (`focus`), as when the page couldn't ask.
    pub fn add(&mut self, a: Annotation, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let comment =
            cx.new(|cx| TextareaState::new(window, cx).placeholder("What should change?"));
        if !a.comment.is_empty() {
            let text = a.comment.clone();
            comment.update(cx, |c, cx| c.set_value(text, window, cx));
        }
        if focus {
            let handle = comment.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        let changed = cx.subscribe(&comment, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.emit(AnnotateEvent::Changed);
            }
        });
        self.rows.push(Row {
            annotation: a,
            comment,
            _changed: changed,
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

    /// The markers of the rows on the page at `url` (hash ignored).
    pub fn markers_for(&self, url: &str, cx: &App) -> Vec<Marker> {
        let key = page_key(url);
        self.rows
            .iter()
            .filter(|r| page_key(&r.annotation.url) == key)
            .map(|r| Marker {
                n: r.annotation.n,
                selector: r.annotation.element.selector.clone(),
                comment: r.comment.read(cx).value().to_string(),
                intent: r.annotation.intent,
                label: r.annotation.element.label.clone(),
            })
            .collect()
    }

    /// The row of note `n` on the page at `url`.
    fn find(&self, url: &str, n: u32) -> Option<usize> {
        let key = page_key(url);
        self.rows
            .iter()
            .position(|r| r.annotation.n == n && page_key(&r.annotation.url) == key)
    }

    /// The user saved note `n` in its popover on the page at `url`. False when there is no
    /// such note.
    pub fn update_from_page(
        &mut self,
        url: &str,
        n: u32,
        comment: &str,
        intent: Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(ix) = self.find(url, n) else {
            return false;
        };
        let row = &mut self.rows[ix];
        row.annotation.intent = intent;
        let field = row.comment.clone();
        let text = comment.to_string();
        field.update(cx, |c, cx| c.set_value(text, window, cx));
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
        true
    }

    /// The user removed note `n` from the page at `url`.
    pub fn remove_from_page(&mut self, url: &str, n: u32, cx: &mut Context<Self>) -> bool {
        match self.find(url, n) {
            Some(ix) => {
                self.remove(ix, cx);
                true
            }
            None => false,
        }
    }

    /// Drops every row for good (an agent read them with `clear`).
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.rows.clear();
        self.sent.clear();
        self.removed.clear();
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
    }

    /// Takes every row, which Undo can bring back for a while.
    fn clear_with_undo(&mut self, cx: &mut Context<Self>) {
        let n = self.rows.len();
        self.removed = self.rows.drain(..).enumerate().collect();
        cx.emit(AnnotateEvent::Changed);
        self.undo_note(
            if n == 1 {
                "Cleared 1 annotation".to_string()
            } else {
                format!("Cleared {n} annotations")
            },
            cx,
        );
    }

    /// Each row's `(url, n, comment)`, the comment as typed so far.
    fn notes(&self, cx: &App) -> Vec<(String, u32, String)> {
        self.rows
            .iter()
            .map(|r| {
                let a = &r.annotation;
                (a.url.clone(), a.n, r.comment.read(cx).value().to_string())
            })
            .collect()
    }

    /// How many notes would be lost with the rows: comments not copied, sent or read by an
    /// agent as they are now. The notes live only here (spec D6), so closing the tab or
    /// Clear asks first.
    pub fn unsent(&self, cx: &App) -> usize {
        self.notes(cx)
            .into_iter()
            .filter(|note| !note.2.trim().is_empty() && !self.sent.contains(note))
            .count()
    }

    /// The notes as they are now were copied, sent or read by an agent.
    pub fn mark_sent(&mut self, cx: &App) {
        self.sent = self.notes(cx);
    }

    /// Types `note` as the comment of row `ix` (for tests).
    #[cfg(test)]
    pub(super) fn type_note(&mut self, ix: usize, note: &str, window: &mut Window, cx: &mut App) {
        let comment = self.rows[ix].comment.clone();
        comment.update(cx, |c, cx| c.set_value(note.to_string(), window, cx));
    }

    /// The Clear button: asks first before it takes several notes, or one not yet sent, and
    /// offers Undo after.
    fn request_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.rows.len();
        let unsent = self.unsent(cx);
        if count == 0 {
            return;
        }
        if count == 1 && unsent == 0 {
            return self.clear_with_undo(cx);
        }
        let title = if count == 1 {
            "Clear 1 annotation?".to_string()
        } else {
            format!("Clear {count} annotations?")
        };
        let mut detail = String::new();
        if unsent > 0 {
            detail.push_str(&notes_lost(unsent));
            detail.push(' ');
        }
        detail.push_str("Their screenshots stay in .nyawit/annotations. You can undo right after.");
        let confirm = clear_label(count);
        let answer = window.prompt(
            PromptLevel::Warning,
            &title,
            Some(&detail),
            &[confirm.as_str(), "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if let Ok(0) = answer.await {
                let _ = this.update(cx, |this, cx| this.clear_with_undo(cx));
            }
        })
        .detach();
    }

    /// The page on show, and its title for the group header.
    pub fn set_current(&mut self, url: &str, title: &str, cx: &mut Context<Self>) {
        if self.current != url || self.title != title {
            self.current = url.to_string();
            self.title = title.to_string();
            cx.notify();
        }
    }

    pub fn set_picking(&mut self, picking: Picking, cx: &mut Context<Self>) {
        if self.picking != picking {
            self.picking = picking;
            cx.notify();
        }
    }

    pub fn set_errors(&mut self, errors: usize, cx: &mut Context<Self>) {
        if self.errors != errors {
            self.errors = errors;
            cx.notify();
        }
    }

    /// Shows `text` in the footer for a few seconds.
    pub fn set_note(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.note = Some(text.into());
        self._note_timer = Some(note_timer(cx, |this: &mut Self| {
            this.note = None;
            this.removed.clear();
        }));
        cx.notify();
    }

    /// Shows `text` with Undo, for longer.
    fn undo_note(&mut self, text: String, cx: &mut Context<Self>) {
        self.note = Some(text.into());
        self._note_timer = Some(note_timer_for(cx, UNDO_FOR, |this: &mut Self| {
            this.note = None;
            this.removed.clear();
        }));
        cx.notify();
    }

    /// Brings back what the last delete or clear took.
    fn undo(&mut self, cx: &mut Context<Self>) {
        let mut back = std::mem::take(&mut self.removed);
        back.sort_by_key(|(ix, _)| *ix);
        for (ix, row) in back {
            let at = ix.min(self.rows.len());
            self.rows.insert(at, row);
        }
        self.note = None;
        self._note_timer = None;
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
    }

    pub fn include_errors(&self) -> bool {
        self.include_errors
    }

    fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.rows.len() {
            let row = self.rows.remove(ix);
            let n = row.annotation.n;
            self.removed = vec![(ix, row)];
            cx.emit(AnnotateEvent::Changed);
            self.undo_note(format!("Removed annotation {n}"), cx);
        }
    }

    fn set_intent(&mut self, ix: usize, intent: Intent, cx: &mut Context<Self>) {
        if let Some(r) = self.rows.get_mut(ix) {
            r.annotation.intent = intent;
            cx.emit(AnnotateEvent::Changed);
            cx.notify();
        }
    }

    fn row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[ix];
        let a = &row.annotation;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let selector = a.element.selector.clone();
        let shot = a.include_screenshot;
        let has_shot = a.screenshot.is_some();
        let summary = element_summary(a);
        let source = source_line(a);
        let intent_ix = Intent::ALL.iter().position(|i| *i == a.intent).unwrap_or(0);
        v_flex()
            .gap_2()
            .p_3()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(number_badge(a.n, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .font_family(theme.mono_font_family.clone())
                                    .child(a.element.label.clone()),
                            )
                            .child(div().truncate().text_xs().text_color(muted).child(summary))
                            .children(source.map(|s| {
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(muted)
                                    .font_family(theme.mono_font_family.clone())
                                    .child(s)
                            })),
                    ),
            )
            .child(
                TabBar::new(("annotate-intent", ix))
                    .segmented()
                    .xsmall()
                    .w_full()
                    .selected_index(intent_ix)
                    .children(Intent::ALL.iter().map(|i| {
                        Tab::new()
                            .label(intent_name(*i))
                            .aria_label(format!("Kind: {}", intent_name(*i)))
                            .flex_1()
                    }))
                    .on_click(cx.listener(move |this, at: &usize, _, cx| {
                        if let Some(i) = Intent::ALL.get(*at) {
                            this.set_intent(ix, *i, cx);
                        }
                    })),
            )
            .child(
                Textarea::new(&row.comment)
                    .h(px(64.))
                    .aria_label(format!("Comment for annotation {}", a.n)),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new(("annotate-show", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Eye)
                            .label("Show")
                            .tooltip("Scroll the page to the element and flash it")
                            .accessibility_label(format!("Show annotation {} on the page", a.n))
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.emit(AnnotateEvent::Show(selector.clone()));
                            })),
                    )
                    .child(
                        Button::new(("annotate-shot", ix))
                            .ghost()
                            .xsmall()
                            .icon(if shot { Lucide::Check } else { Lucide::Camera })
                            .label("Screenshot")
                            .selected(shot && has_shot)
                            .disabled(!has_shot)
                            .tooltip(if !has_shot {
                                "No screenshot was taken of this element"
                            } else if shot {
                                "The screenshot goes with the note; click to leave it out"
                            } else {
                                "Click to send the screenshot with the note"
                            })
                            .accessibility_label(if shot {
                                "Leave the screenshot out"
                            } else {
                                "Include the screenshot"
                            })
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if let Some(r) = this.rows.get_mut(ix) {
                                    r.annotation.include_screenshot =
                                        !r.annotation.include_screenshot;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new(("annotate-delete", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Trash)
                            .label("Delete")
                            .accessibility_label(format!("Delete annotation {}", a.n))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove(ix, cx);
                            })),
                    ),
            )
    }

    /// The line under the header: whether the page picks, and how to start again.
    fn status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (dot, text) = match self.picking {
            Picking::On => (theme.primary, "Click an element on the page to annotate it"),
            Picking::Waiting => (theme.muted_foreground, "Waiting for the page to load…"),
            Picking::Off => (theme.muted_foreground, "Picking is off"),
        };
        h_flex()
            .flex_none()
            .h(px(36.))
            .px_3()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(div().size(px(6.)).flex_none().rounded_full().bg(dot))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(if self.picking == Picking::On {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child(text),
            )
            .when(self.picking == Picking::Off, |el| {
                el.child(
                    Button::new("annotate-pick")
                        .outline()
                        .xsmall()
                        .icon(Lucide::MousePointer2)
                        .label("Pick element")
                        .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                            cx.emit(AnnotateEvent::Pick);
                        })),
                )
            })
            .when(self.picking == Picking::On, |el| {
                el.child(panel::key("Esc", cx)).child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("stops"),
                )
            })
    }

    /// How to start, and the keys, while there is nothing yet.
    fn empty_state(&self, window: &Window, cx: &App) -> impl IntoElement {
        let annotate = crate::keymap::keys_for(&super::ToggleAnnotate, "Browser", window);
        let muted = cx.theme().muted_foreground;
        let line = |keys: Vec<Div>, what: &'static str| {
            h_flex()
                .gap_1()
                .items_center()
                .text_xs()
                .children(keys)
                .child(div().pl_1().text_color(muted).child(what))
        };
        v_flex()
            .gap_4()
            .child(panel::empty(
                "No annotations yet",
                "Click an element on the page, say what should change, and press Return. \
                 Notes go to your agent with the element's selector, styles, source file and \
                 a screenshot.",
                None,
                cx,
            ))
            .child(panel::section(
                "Keys",
                None,
                v_flex()
                    .gap_1p5()
                    .child(line(
                        vec![panel::key("↑", cx), panel::key("↓", cx)],
                        "Walk to the parent or back to the child",
                    ))
                    .child(line(
                        vec![panel::key("⌥", cx), panel::key("Click", cx)],
                        "Pick the parent of what's under the pointer",
                    ))
                    .child(line(
                        vec![panel::key("↩", cx)],
                        "Save the note; ⇧↩ for a new line",
                    ))
                    .child(line(
                        vec![panel::key("Esc", cx)],
                        "Cancel the note; again to stop picking",
                    ))
                    .child(line(
                        vec![panel::key("⌫", cx)],
                        "Remove the note whose marker has focus",
                    ))
                    .when(!annotate.is_empty(), |el| {
                        el.child(line(
                            vec![panel::key(annotate.clone(), cx)],
                            "Annotate on or off",
                        ))
                    }),
                cx,
            ))
    }

    /// The group header: the page's title, and its path with the host in a tooltip.
    fn group_header(&self, key: &str, title: &str, cx: &App) -> impl IntoElement {
        let (path, host) = split_url(key);
        let muted = cx.theme().muted_foreground;
        let title = if title.trim().is_empty() {
            host.clone()
        } else {
            title.to_string()
        };
        v_flex()
            .id(SharedString::from(format!("annotate-page-{key}")))
            .gap_0p5()
            .tooltip(move |window, cx| Tooltip::new(key_tooltip(&host, &path)).build(window, cx))
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                div()
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(split_url(key).0),
            )
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_rows = !self.rows.is_empty();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let errors_label = match self.errors {
            0 => "Include console errors".to_string(),
            n => format!("Include console errors ({n})"),
        };
        let undo = !self.removed.is_empty();
        v_flex()
            .flex_none()
            .gap_2()
            .p_3()
            .border_t_1()
            .border_color(theme.border)
            // What goes with the notes, and the destructive action, apart from sending them.
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Checkbox::new("annotate-errors")
                                .label(errors_label)
                                .checked(self.include_errors)
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.include_errors = *checked;
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(
                        Button::new("annotate-clear")
                            .debug_selector(|| "annotate-clear".into())
                            .xsmall()
                            .danger()
                            .outline()
                            .icon(Lucide::Trash)
                            .label("Clear all")
                            .disabled(!has_rows)
                            .accessibility_label("Clear all annotations")
                            .tooltip("Remove every annotation; you can undo it")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.request_clear(window, cx);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("annotate-send")
                            .debug_selector(|| "annotate-send".into())
                            .small()
                            .primary()
                            .flex_1()
                            .icon(Lucide::SquareTerminal)
                            .label("Send to terminal")
                            .disabled(!has_rows)
                            .tooltip(
                                "Paste the notes into the agent's terminal, without pressing Return",
                            )
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                                cx.emit(AnnotateEvent::Send);
                            })),
                    )
                    .child(
                        Button::new("annotate-copy")
                            .debug_selector(|| "annotate-copy".into())
                            .small()
                            .outline()
                            .icon(Lucide::Copy)
                            .label("Copy")
                            .disabled(!has_rows)
                            .tooltip("Copy the notes as Markdown")
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                                cx.emit(AnnotateEvent::Copy);
                            })),
                    ),
            )
            .children(self.note.clone().map(|note| {
                h_flex()
                    .min_h(px(22.))
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().text_xs().text_color(muted).child(note))
                    .when(undo, |el| {
                        el.child(
                            Button::new("annotate-undo")
                                .xsmall()
                                .outline()
                                .icon(Lucide::Undo2)
                                .label("Undo")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.undo(cx);
                                })),
                        )
                    })
            }))
    }
}

impl Render for AnnotatePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items: Vec<Annotation> = self.rows.iter().map(|r| r.annotation.clone()).collect();
        let groups = group_order(&self.current, &items);
        let list = if self.rows.is_empty() {
            self.empty_state(window, cx).into_any_element()
        } else {
            let mut sections: Vec<AnyElement> = Vec::new();
            for key in groups {
                let title = if key == page_key(&self.current) {
                    self.title.clone()
                } else {
                    items
                        .iter()
                        .find(|a| page_key(&a.url) == key)
                        .map(|a| a.title.clone())
                        .unwrap_or_default()
                };
                let mut rows: Vec<AnyElement> = Vec::new();
                for ix in 0..self.rows.len() {
                    if page_key(&self.rows[ix].annotation.url) == key {
                        rows.push(self.row(ix, cx).into_any_element());
                    }
                }
                sections.push(
                    v_flex()
                        .gap_2()
                        .child(self.group_header(&key, &title, cx))
                        .children(rows)
                        .into_any_element(),
                );
            }
            v_flex().gap_6().children(sections).into_any_element()
        };
        v_flex()
            .size_full()
            .child(self.status(cx))
            .child(panel::scroll("annotate-list").child(list))
            .child(self.footer(cx))
    }
}

/// The number on a note's marker.
fn number_badge(n: u32, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .flex_none()
        .min_w(px(20.))
        .h(px(20.))
        .px_1()
        .rounded_full()
        .bg(theme.primary)
        .text_color(theme.primary_foreground)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(n.to_string())
}

/// What the element is, under its label: `button “Shop the harvest” · 180×40`.
pub(super) fn element_summary(a: &Annotation) -> String {
    let el = &a.element;
    let size = format!("{}×{}", el.rect.width.round(), el.rect.height.round());
    match el.role.as_deref().filter(|r| !r.is_empty()) {
        Some(role) => match el.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => format!("{role} “{}” · {size}", cut(name, 40)),
            None => format!("{role} · {size}"),
        },
        None => match el.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            Some(text) => format!("“{}” · {size}", cut(text, 40)),
            None => size,
        },
    }
}

/// The source line under a card's label: `src/Hero.tsx:24 · <HeroActions>`.
pub(super) fn source_line(a: &Annotation) -> Option<String> {
    let src = a.element.source.as_ref()?;
    let mut parts = Vec::new();
    if !src.file.is_empty() {
        parts.push(match src.line {
            Some(line) => format!("{}:{line}", src.file),
            None => src.file.clone(),
        });
    }
    if let Some(c) = src.component.as_deref().filter(|c| !c.is_empty()) {
        parts.push(format!("<{c}>"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

/// The confirmation button of Clear, naming what goes.
fn clear_label(count: usize) -> String {
    if count == 1 {
        "Clear annotation".into()
    } else {
        format!("Clear {count} annotations")
    }
}

/// The group header's tooltip: where the page is.
fn key_tooltip(host: &str, path: &str) -> String {
    format!("{host}{path}")
}

/// What a prompt says is lost with `n` notes.
pub fn notes_lost(n: usize) -> String {
    format!(
        "{n} note{} on the page {} been copied or sent.",
        if n == 1 { "" } else { "s" },
        if n == 1 { "hasn't" } else { "haven't" }
    )
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
    use super::{
        AnnotatePanel, clear_label, element_summary, group_order, padded_rect, source_line,
        split_url,
    };
    use sawit_core::annotations::{Annotation, ElementDetails, Intent, Rect, SourceHint};

    fn at(url: &str, n: u32) -> Annotation {
        Annotation {
            n,
            url: url.into(),
            title: String::new(),
            element: ElementDetails {
                selector: "a".into(),
                label: "a".into(),
                tag: "a".into(),
                rect: Rect {
                    x: 0.,
                    y: 0.,
                    width: 1.,
                    height: 1.,
                },
                ..ElementDetails::default()
            },
            comment: String::new(),
            intent: Intent::Fix,
            screenshot: None,
            include_screenshot: true,
        }
    }

    fn panel(
        cx: &mut gpui_kit::TestAppContext,
    ) -> (
        gpui_kit::Entity<AnnotatePanel>,
        &mut gpui_kit::VisualTestContext,
    ) {
        use gpui_kit::AppContext as _;
        cx.update(gpui_kit::init);
        let cx = cx.add_empty_window();
        let panel = cx.update(|window, cx| cx.new(|cx| AnnotatePanel::new(window, cx)));
        (panel, cx)
    }

    /// Clear asks before it drops notes that weren't copied, sent or read by an agent, and
    /// before several; a lone note already sent goes at once, with Undo.
    #[gpui_kit::test]
    fn clear_asks_before_dropping_notes_not_sent(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::VisualTestContext;
        let (panel, cx) = panel(cx);
        let rows =
            |cx: &mut VisualTestContext| cx.update(|_, cx| panel.read(cx).annotations(cx).len());
        let add = |note: &str, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                panel.update(cx, |p, cx| p.add(at("http://a/1", 1), false, window, cx));
                let comment = panel.read(cx).rows.last().unwrap().comment.clone();
                comment.update(cx, |c, cx| c.set_value(note.to_string(), window, cx));
            });
        };
        let clear = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| panel.update(cx, |p, cx| p.request_clear(window, cx)));
            cx.run_until_parked();
        };

        add("Make it blue", cx);
        assert_eq!(cx.update(|_, cx| panel.read(cx).unsent(cx)), 1);
        clear(cx);
        assert!(cx.has_pending_prompt(), "a note would be lost");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(rows(cx), 1);
        clear(cx);
        cx.simulate_prompt_answer("Clear annotation");
        cx.run_until_parked();
        assert_eq!(rows(cx), 0);

        add("Make it blue", cx);
        cx.update(|_, cx| panel.update(cx, |p, cx| p.mark_sent(cx)));
        assert_eq!(cx.update(|_, cx| panel.read(cx).unsent(cx)), 0);
        // A note edited after it was sent is unsent again.
        cx.update(|window, cx| {
            let comment = panel.read(cx).rows[0].comment.clone();
            comment.update(cx, |c, cx| {
                c.set_value("Make it red".to_string(), window, cx)
            });
        });
        assert_eq!(cx.update(|_, cx| panel.read(cx).unsent(cx)), 1);
        cx.update(|_, cx| panel.update(cx, |p, cx| p.mark_sent(cx)));
        clear(cx);
        assert!(!cx.has_pending_prompt(), "one note, and it was sent");
        assert_eq!(rows(cx), 0);
        // Undo brings it back.
        cx.update(|_, cx| panel.update(cx, |p, cx| p.undo(cx)));
        assert_eq!(rows(cx), 1);

        // Several ask first, sent or not.
        add("Make it green", cx);
        cx.update(|_, cx| panel.update(cx, |p, cx| p.mark_sent(cx)));
        clear(cx);
        assert!(cx.has_pending_prompt(), "two notes go at once");
        cx.simulate_prompt_answer("Clear 2 annotations");
        cx.run_until_parked();
        assert_eq!(rows(cx), 0);
    }

    /// What the popover saves and removes on the page reaches the row with its number, on its
    /// page only; a delete can be undone into the same place.
    #[gpui_kit::test]
    fn the_page_and_the_panel_keep_in_step(cx: &mut gpui_kit::TestAppContext) {
        let (panel, cx) = panel(cx);
        cx.update(|window, cx| {
            panel.update(cx, |p, cx| {
                p.add(at("http://a/1", 1), false, window, cx);
                p.add(at("http://a/1", 2), false, window, cx);
                p.add(at("http://a/2", 1), false, window, cx);
            })
        });
        let updated = cx.update(|window, cx| {
            panel.update(cx, |p, cx| {
                p.update_from_page("http://a/1#x", 2, "Bigger", Intent::Change, window, cx)
            })
        });
        assert!(updated);
        let markers = cx.update(|_, cx| panel.read(cx).markers_for("http://a/1", cx));
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[1].comment, "Bigger");
        assert_eq!(markers[1].intent, Intent::Change);
        // No note 3 on that page.
        let missing = cx.update(|window, cx| {
            panel.update(cx, |p, cx| {
                p.update_from_page("http://a/1", 3, "x", Intent::Fix, window, cx)
            })
        });
        assert!(!missing);

        let removed =
            cx.update(|_, cx| panel.update(cx, |p, cx| p.remove_from_page("http://a/1", 1, cx)));
        assert!(removed);
        let left: Vec<(String, u32)> = cx.update(|_, cx| {
            panel
                .read(cx)
                .annotations(cx)
                .into_iter()
                .map(|a| (a.url, a.n))
                .collect()
        });
        assert_eq!(left, [("http://a/1".into(), 2), ("http://a/2".into(), 1)]);
        cx.update(|_, cx| panel.update(cx, |p, cx| p.undo(cx)));
        let back: Vec<u32> =
            cx.update(|_, cx| panel.read(cx).annotations(cx).iter().map(|a| a.n).collect());
        assert_eq!(back, [1, 2, 1]);
    }

    /// The footer's actions fit the panel's width inside its inset: Clear on the console
    /// errors' line, apart from Send and Copy on the line under it.
    #[gpui_kit::test]
    fn the_footer_fits_the_panel(cx: &mut gpui_kit::TestAppContext) {
        use crate::browser::panel::PANEL_WIDTH;
        use gpui_kit::{VisualTestContext, px, size};
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(PANEL_WIDTH), px(700.)), |window, cx| {
            let mut panel = AnnotatePanel::new(window, cx);
            panel.add(at("http://a/1", 1), false, window, cx);
            panel
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let [send, copy, clear] = ["annotate-send", "annotate-copy", "annotate-clear"].map(|s| {
            cx.debug_bounds(s)
                .unwrap_or_else(|| panic!("{s} isn't drawn"))
        });
        assert!(send.left() >= px(12.), "{send:?}");
        assert!(send.right() <= copy.left(), "Send runs into Copy");
        assert!(
            copy.right() <= px(PANEL_WIDTH - 12.),
            "Copy is clipped: {copy:?}"
        );
        assert!(
            clear.right() <= px(PANEL_WIDTH - 12.),
            "Clear is clipped: {clear:?}"
        );
        assert_eq!(send.top(), copy.top(), "Send and Copy wrapped");
        assert!(clear.bottom() <= send.top(), "Clear sits with Send");
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

    /// A card says what the element is in words before its size, and where its source is.
    #[test]
    fn cards_name_the_element_and_its_source() {
        let mut a = at("http://a/1", 1);
        a.element.rect.width = 180.4;
        a.element.rect.height = 40.;
        assert_eq!(element_summary(&a), "180×40");
        a.element.text = Some("  Shop  ".into());
        assert_eq!(element_summary(&a), "“Shop” · 180×40");
        a.element.role = Some("button".into());
        a.element.name = Some("Shop the harvest".into());
        assert_eq!(element_summary(&a), "button “Shop the harvest” · 180×40");
        assert_eq!(source_line(&a), None);
        a.element.source = Some(SourceHint {
            file: "src/Hero.tsx".into(),
            line: Some(24),
            framework: "React".into(),
            component: Some("HeroActions".into()),
        });
        assert_eq!(
            source_line(&a).as_deref(),
            Some("src/Hero.tsx:24 · <HeroActions>")
        );
        assert_eq!(clear_label(1), "Clear annotation");
        assert_eq!(clear_label(3), "Clear 3 annotations");
    }
}
