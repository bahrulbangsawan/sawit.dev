//! The Annotate panel: one card per element the user picked, with a comment, an intent and a
//! screenshot, grouped by page. Notes are written in a popover on the page itself
//! (`browser_overlay.js`); the panel is their list, kept in step both ways. `BrowserView` owns
//! the flow around it (pick, capture, markers, copy); the panel owns the rows, and where the
//! notes go: an agent running in a terminal, or a new one ([`super::agents`]).

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::annotations::{Annotation, Intent, Rect, page_key};
use sawit_core::terminal::{AgentKind, PaneId};

use super::agents::{self, AgentSource};
use super::panel::{self, note_timer, note_timer_for};
use crate::terminal::RunningAgent;

/// How long Undo stays after a delete or a clear.
const UNDO_FOR: Duration = Duration::from_secs(8);
/// How often the open panel looks again for the agents running in the terminals.
const AGENT_POLL: Duration = Duration::from_secs(1);
/// From how many notes on, picking another collapses the ones before it.
const KEEP_OPEN: usize = 3;
/// How wide an agent menu may be: it opens over the panel, never over the page, whose webview
/// draws above it.
const MENU_WIDTH: f32 = 236.;

/// What the panel asks of the browser.
pub enum AnnotateEvent {
    /// Flash this selector on the page.
    Show(String),
    /// Rows were added, removed or edited: the page's markers need redrawing.
    Changed,
    Copy,
    /// Send the notes to this agent's chat.
    SendTo(RunningAgent),
    /// Start this agent in a new terminal with the notes as its first message.
    Open(AgentKind),
    /// Pick again (after Esc stopped it).
    Pick,
}

/// What the send button does, from the agents running and those installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SendAction {
    /// Sends to the agent at this index; with more than one running, a menu lists them all.
    Send { default: usize },
    /// Starts this agent CLI in a new terminal; with more than one installed, a menu lists them.
    Open { kind: AgentKind },
    /// No agent runs and none was found to start, or the panel is still looking.
    NoAgent { looking: bool },
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
    /// Names the row's elements for as long as the panel lives: a row's index changes as
    /// others are removed, and its toggle keeps its focus by this.
    key: u64,
    annotation: Annotation,
    comment: Entity<TextareaState>,
    /// Shown as one line, until the user opens it again.
    collapsed: bool,
    /// The card's head, which folds and opens it, keeps the keyboard through both.
    head: FocusHandle,
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
    /// The next row's key.
    next_key: u64,
    /// The agents running in the terminals, as last looked at.
    agents: Vec<RunningAgent>,
    /// The agent pane last sent to, remembered while its agent runs.
    last_target: Option<PaneId>,
    /// The agent CLIs on this Mac that Open agent can start; `None` until looked for.
    installed: Option<Vec<AgentKind>>,
    /// The agent last sent to or opened, which Open agent starts again.
    last_kind: Option<AgentKind>,
    /// Where the running agents are found; none in tests, which set them.
    source: Option<AgentSource>,
    /// The panel is open, so it keeps the agents up to date.
    shown: bool,
    _poll: Option<Task<()>>,
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
            next_key: 0,
            agents: Vec::new(),
            last_target: None,
            installed: None,
            last_kind: None,
            source: None,
            shown: false,
            _poll: None,
        }
    }

    /// Where the agents running in the terminals are found. While the panel is open it looks
    /// again every [`AGENT_POLL`], as an agent can start or end in a terminal at any time.
    pub fn set_agent_source(&mut self, source: AgentSource, cx: &mut Context<Self>) {
        self.source = Some(source);
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AGENT_POLL).await;
                let alive = this.update(cx, |this, cx| {
                    if this.shown {
                        this.refresh_agents(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
        self.refresh_agents(cx);
        self.find_installed(cx);
    }

    /// The panel opened or closed. Opened, it looks at once for the agents running, and for the
    /// agent CLIs installed since it last looked.
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.shown = shown;
        if shown && self.source.is_some() {
            self.refresh_agents(cx);
            self.find_installed(cx);
        }
    }

    /// Looks again for the agents running in the terminals. A remembered target whose agent has
    /// ended is forgotten.
    pub fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        if let Some(source) = self.source.clone() {
            let agents = source(cx);
            self.set_agents(agents, cx);
        }
    }

    /// The agents running in the terminals.
    pub(super) fn set_agents(&mut self, agents: Vec<RunningAgent>, cx: &mut Context<Self>) {
        if let Some(pane) = self.last_target
            && !agents.iter().any(|a| a.pane == pane)
        {
            self.last_target = None;
        }
        if !agents::same_agents(&agents, &self.agents) {
            self.agents = agents;
            cx.notify();
        }
    }

    /// The agent CLIs Open agent can start (for tests; the panel looks for them itself).
    #[cfg(test)]
    pub(super) fn set_installed(&mut self, installed: Vec<AgentKind>, cx: &mut Context<Self>) {
        self.installed = Some(installed);
        cx.notify();
    }

    /// Looks for the agent CLIs on this Mac, off the UI thread.
    fn find_installed(&mut self, cx: &mut Context<Self>) {
        let found = crate::rt::spawn_blocking(cx, agents::installed_agents);
        cx.spawn(async move |this, cx| {
            if let Ok(found) = found.await {
                let _ = this.update(cx, |this, cx| {
                    if this.installed.as_ref() != Some(&found) {
                        this.installed = Some(found);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// What the send button does now.
    pub(super) fn send_action(&self) -> SendAction {
        if let Some(default) = agents::default_target(&self.agents, self.last_target) {
            return SendAction::Send { default };
        }
        match &self.installed {
            None => SendAction::NoAgent { looking: true },
            Some(found) => match agents::default_open(found, self.last_kind) {
                Some(kind) => SendAction::Open { kind },
                None => SendAction::NoAgent { looking: false },
            },
        }
    }

    /// Sends the notes to the agent in `pane`, which is remembered as the target while it runs.
    fn send_to(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let Some(agent) = self.agents.iter().find(|a| a.pane == pane).cloned() else {
            return;
        };
        self.last_target = Some(pane);
        self.last_kind = Some(agent.kind);
        cx.emit(AnnotateEvent::SendTo(agent));
    }

    /// Starts `kind` in a new terminal with the notes, and starts it again next time.
    fn open_agent(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.last_kind = Some(kind);
        cx.emit(AnnotateEvent::Open(kind));
    }

    /// Appends `a`. Its comment, written in the page's popover, is the row's; without one the
    /// row's comment field takes the keyboard (`focus`), as when the page couldn't ask. The new
    /// row opens; past [`KEEP_OPEN`] notes the others fold to a line each, so the list stays
    /// short enough to scan.
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
        if self.rows.len() >= KEEP_OPEN {
            for row in &mut self.rows {
                row.collapsed = true;
            }
        }
        self.rows.push(Row {
            key: self.next_key,
            annotation: a,
            comment,
            collapsed: false,
            head: cx.focus_handle(),
            _changed: changed,
        });
        self.next_key += 1;
        cx.emit(AnnotateEvent::Changed);
        cx.notify();
    }

    /// Folds row `ix` to one line, or opens it.
    fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get_mut(ix) {
            row.collapsed = !row.collapsed;
            cx.notify();
        }
    }

    /// Folds every row on the page `key`, or opens them all.
    fn collapse_page(&mut self, key: &str, collapsed: bool, cx: &mut Context<Self>) {
        for row in &mut self.rows {
            if page_key(&row.annotation.url) == key {
                row.collapsed = collapsed;
            }
        }
        cx.notify();
    }

    /// Whether each row is folded, in order (for tests).
    #[cfg(test)]
    fn collapsed(&self) -> Vec<bool> {
        self.rows.iter().map(|r| r.collapsed).collect()
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

    /// A note's card: one line while folded (its number, kind, element and the first line of the
    /// comment), else the whole card. Its head folds and opens it, by click, ↩ or Space.
    fn row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[ix];
        let theme = cx.theme();
        let collapsed = row.collapsed;
        v_flex()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .child(self.card_head(ix, cx))
            .when(!collapsed, |el| el.child(self.card_body(ix, cx)))
    }

    /// The part of a card that folds it: a button that says whether the card is open.
    fn card_head(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[ix];
        let a = &row.annotation;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let collapsed = row.collapsed;
        let comment = row.comment.read(cx).value().to_string();
        let first = first_line(&comment);
        let kind = intent_name(a.intent);
        let inner = theme.radius - px(1.);
        let label = if first.is_empty() {
            format!("Annotation {}, {kind}, no note yet", a.n)
        } else {
            format!("Annotation {}, {kind}: {first}", a.n)
        };
        let head = gpui_kit::base::Button::new(("annotate-card", row.key))
            .track_focus(&row.head)
            .debug_selector(move || format!("annotate-card-{ix}"))
            .accessibility_label(label)
            .aria_expanded(!collapsed)
            .w_full()
            .flex()
            .gap_2()
            .px_3()
            .border_1()
            .border_color(transparent_black())
            .map(|el| {
                if collapsed {
                    el.py_1p5().items_center().rounded(inner)
                } else {
                    el.pt_3().pb_2().items_start().rounded_t(inner)
                }
            })
            .line_height(relative(1.4))
            .cursor_pointer()
            .hover(|s| s.bg(theme.list_hover))
            .focus_visible(|s| s.border_color(theme.ring))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle(ix, cx)))
            .child(number_badge(a.n, cx));
        let chevron = Icon::new(if collapsed {
            Lucide::ChevronRight
        } else {
            Lucide::ChevronDown
        })
        .xsmall()
        .flex_none()
        .text_color(muted);
        if collapsed {
            let full = comment.trim().to_string();
            head.when(!full.is_empty(), |el| {
                el.tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
            })
            .child(panel::tag(kind, cx))
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w(px(24.))
                    .max_w(px(112.))
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .font_family(mono)
                    .child(a.element.label.clone()),
            )
            .child(div().flex_1().min_w_0().truncate().text_xs().map(|el| {
                if first.is_empty() {
                    el.text_color(muted).child("No note yet")
                } else {
                    el.child(first)
                }
            }))
            .child(chevron)
            .into_any_element()
        } else {
            let summary = element_summary(a);
            let source = source_line(a);
            head.child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .font_family(mono.clone())
                            .child(a.element.label.clone()),
                    )
                    .child(div().truncate().text_xs().text_color(muted).child(summary))
                    .children(source.map(|s| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .font_family(mono)
                            .child(s)
                    })),
            )
            .child(div().pt_0p5().child(chevron))
            .into_any_element()
        }
    }

    /// An open card under its head: the kind, the comment and the card's actions.
    fn card_body(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[ix];
        let a = &row.annotation;
        let selector = a.element.selector.clone();
        let shot = a.include_screenshot;
        let has_shot = a.screenshot.is_some();
        let intent_ix = Intent::ALL.iter().position(|i| *i == a.intent).unwrap_or(0);
        v_flex()
            .gap_2()
            .px_3()
            .pb_3()
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

    /// The group header: the page's title, its path under it when that says more than the
    /// title does (the home page's `/` doesn't), with the host in a tooltip; and with two notes
    /// or more, Collapse all, or Expand all when every one is folded.
    fn group_header(&self, key: &str, title: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let (path, host) = split_url(key);
        let muted = cx.theme().muted_foreground;
        let title = if title.trim().is_empty() {
            host.clone()
        } else {
            title.to_string()
        };
        let shown_path = route_line(&path);
        let rows: Vec<&Row> = self
            .rows
            .iter()
            .filter(|r| page_key(&r.annotation.url) == key)
            .collect();
        let fold = (rows.len() >= 2).then(|| rows.iter().all(|r| r.collapsed));
        let page = key.to_string();
        h_flex()
            .gap_2()
            .items_center()
            .child(
                v_flex()
                    .id(SharedString::from(format!("annotate-page-{key}")))
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .tooltip(move |window, cx| {
                        Tooltip::new(key_tooltip(&host, &path)).build(window, cx)
                    })
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .children(shown_path.map(|path| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(path)
                    })),
            )
            .children(fold.map(|all_folded| {
                Button::new(SharedString::from(format!("annotate-fold-{key}")))
                    .debug_selector(|| "annotate-fold".into())
                    .ghost()
                    .xsmall()
                    .icon(if all_folded {
                        Lucide::UnfoldVertical
                    } else {
                        Lucide::FoldVertical
                    })
                    .label(if all_folded {
                        "Expand all"
                    } else {
                        "Collapse all"
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.collapse_page(&page, !all_folded, cx);
                    }))
            }))
    }

    fn footer(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_rows = !self.rows.is_empty();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let errors_label = match self.errors {
            0 => "Include console errors".to_string(),
            n => format!("Include console errors ({n})"),
        };
        let undo = !self.removed.is_empty();
        let reason = self.send_blocked(has_rows);
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
                    .child(self.send_control(has_rows, window, cx))
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
            .children(self.note.clone().or(reason).map(|note| {
                h_flex()
                    .min_h(px(22.))
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .debug_selector(|| "annotate-note".into())
                            .flex_1()
                            .text_xs()
                            .text_color(muted)
                            .child(note),
                    )
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

    /// Why the send button can't be used now, said under it: there are no notes, or no agent
    /// to send them to.
    fn send_blocked(&self, has_rows: bool) -> Option<SharedString> {
        if !has_rows {
            return Some("Pick an element and write a note to send it to an agent.".into());
        }
        match self.send_action() {
            SendAction::NoAgent { looking: true } => Some("Looking for agent CLIs…".into()),
            SendAction::NoAgent { looking: false } => Some(
                "No agent is running in a terminal, and no agent CLI was found. Install Claude \
                 Code, Codex or Gemini CLI from Dev Tools."
                    .into(),
            ),
            _ => None,
        }
    }

    /// The send button: Send to the agent running in a terminal, with a menu of the others when
    /// there are several; else Open, which starts one with the notes, with a menu of the agent
    /// CLIs installed when there are several. Each shows the agent's logo.
    fn send_control(
        &self,
        has_rows: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let tint = theme.primary_foreground;
        let menu_tint = theme.popover_foreground;
        let action = self.send_action();
        let this = cx.entity().downgrade();
        let (mode, control): (&'static str, AnyElement) = match action {
            SendAction::Send { default } => {
                let agent = self.agents[default].clone();
                let name = agents::target_name(&self.agents, default);
                let pane = agent.pane;
                let primary = Button::new("annotate-send-primary")
                    .small()
                    .primary()
                    .flex_1()
                    .child(logo_slot(agent.kind, tint, !has_rows, window, cx))
                    .child(button_text(format!("Send to {}", agent.kind.name())))
                    .accessibility_label(format!("Send the notes to {name}"))
                    .tooltip(format!(
                        "Paste the notes into the chat of {name} and send them"
                    ))
                    .disabled(!has_rows)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.send_to(pane, cx);
                    }));
                if self.agents.len() < 2 {
                    ("annotate-send-one", primary.w_full().into_any_element())
                } else {
                    let entries: Rc<Vec<(RunningAgent, String, bool)>> = Rc::new(
                        self.agents
                            .iter()
                            .enumerate()
                            .map(|(ix, a)| {
                                (
                                    a.clone(),
                                    agents::target_name(&self.agents, ix),
                                    ix == default,
                                )
                            })
                            .collect(),
                    );
                    let control = DropdownButton::new("annotate-send-split")
                        .w_full()
                        .small()
                        .primary()
                        .button(primary)
                        .disabled(!has_rows)
                        .menu_accessibility_label("Choose the agent to send the notes to")
                        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                            let mut menu = menu.max_w(px(MENU_WIDTH)).label("Send the notes to");
                            for (agent, name, ticked) in entries.iter().cloned() {
                                let this = this.clone();
                                let pane = agent.pane;
                                menu = menu.item(
                                    PopupMenuItem::element(move |window, cx| {
                                        menu_entry(agent.kind, name.clone(), menu_tint, window, cx)
                                    })
                                    .checked(ticked)
                                    .on_click(
                                        move |_, _, cx| {
                                            let _ = this.update(cx, |p, cx| p.send_to(pane, cx));
                                        },
                                    ),
                                );
                            }
                            menu
                        });
                    ("annotate-send-split", control.into_any_element())
                }
            }
            SendAction::Open { kind } => {
                let name = kind.name();
                let primary = Button::new("annotate-open-primary")
                    .small()
                    .primary()
                    .flex_1()
                    .child(logo_slot(kind, tint, !has_rows, window, cx))
                    .child(button_text(format!("Open {name}")))
                    .accessibility_label(format!("Open {name} in a new terminal with the notes"))
                    .tooltip(format!(
                        "No agent is running in a terminal: start {name} in a new one, with the \
                         notes as its first message"
                    ))
                    .disabled(!has_rows)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_agent(kind, cx);
                    }));
                let installed = self.installed.clone().unwrap_or_default();
                if installed.len() < 2 {
                    ("annotate-open-one", primary.w_full().into_any_element())
                } else {
                    let control = DropdownButton::new("annotate-open-split")
                        .w_full()
                        .small()
                        .primary()
                        .button(primary)
                        .disabled(!has_rows)
                        .menu_accessibility_label("Choose the agent to open")
                        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                            let mut menu = menu.max_w(px(MENU_WIDTH)).label("Open with the notes");
                            for other in installed.iter().copied() {
                                let this = this.clone();
                                menu = menu.item(
                                    PopupMenuItem::element(move |window, cx| {
                                        menu_entry(
                                            other,
                                            other.name().to_string(),
                                            menu_tint,
                                            window,
                                            cx,
                                        )
                                    })
                                    .checked(other == kind)
                                    .on_click(
                                        move |_, _, cx| {
                                            let _ =
                                                this.update(cx, |p, cx| p.open_agent(other, cx));
                                        },
                                    ),
                                );
                            }
                            menu
                        });
                    ("annotate-open-split", control.into_any_element())
                }
            }
            SendAction::NoAgent { .. } => (
                "annotate-send-none",
                Button::new("annotate-open-none")
                    .small()
                    .primary()
                    .w_full()
                    .icon(Lucide::Bot)
                    .label("Open agent")
                    .disabled(true)
                    .into_any_element(),
            ),
        };
        div()
            .debug_selector(|| "annotate-send".into())
            .flex_1()
            .min_w_0()
            .child(
                h_flex()
                    .debug_selector(move || mode.into())
                    .w_full()
                    .child(control),
            )
    }
}

/// An agent's logo in a button, 14 px, as the terminal draws it, faded with the button when it
/// is disabled; a one-colour logo in the button's `tint`.
fn logo_slot(kind: AgentKind, tint: Hsla, disabled: bool, window: &Window, cx: &App) -> Div {
    div()
        .flex_none()
        .when(disabled, |el| el.opacity(0.5))
        .child(crate::terminal::agent_logo(kind, px(14.), tint, window, cx))
}

/// A button's label beside its logo, cut short when the panel is narrow.
fn button_text(text: String) -> impl IntoElement {
    div().min_w_0().truncate().child(text)
}

/// A menu line: the agent's logo and what tells it apart.
fn menu_entry(kind: AgentKind, name: String, tint: Hsla, window: &Window, cx: &App) -> Div {
    h_flex()
        .gap_2()
        .min_w_0()
        .items_center()
        .child(crate::terminal::agent_logo(kind, px(16.), tint, window, cx))
        .child(div().min_w_0().truncate().child(name))
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
            .child(self.footer(window, cx))
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
    if let Some(c) = src.component.as_deref().filter(|c| real_component(c)) {
        parts.push(format!("<{c}>"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Whether a component's name, as the page's framework reports it, names something the user
/// wrote: `HeroActions`, `nav-bar`, `Card.Header`. A production build minifies names (`$u`,
/// `Xe`, `t`), and a placeholder like `Anonymous` names nothing; a card shows neither.
pub(super) fn real_component(name: &str) -> bool {
    let name = name.trim();
    name.chars().count() >= 3
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '(' | ')'))
        && !matches!(
            name.to_ascii_lowercase().as_str(),
            "anonymous" | "unknown" | "component"
        )
}

/// A note's first line that has words, for its folded card.
fn first_line(comment: &str) -> String {
    comment
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// The path a group header shows under the page's title, when it says more than the title
/// alone: not the home page's bare `/`.
fn route_line(path: &str) -> Option<String> {
    (path != "/").then(|| path.to_string())
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
        AnnotatePanel, SendAction, clear_label, element_summary, first_line, group_order,
        padded_rect, real_component, route_line, source_line, split_url,
    };
    use crate::terminal::RunningAgent;
    use sawit_core::annotations::{Annotation, ElementDetails, Intent, Rect, SourceHint};
    use sawit_core::terminal::AgentKind;

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

    /// A minified or placeholder component name (`<$u>` on a production page) says nothing, so a
    /// card leaves it out and keeps the source file; a name someone wrote stays.
    #[test]
    fn junk_component_names_are_hidden() {
        for junk in ["$u", "$", "Xe", "t", "_c", "$Foo", "Anonymous", "  ", "a b"] {
            assert!(!real_component(junk), "{junk:?}");
        }
        for real in [
            "HeroActions",
            "nav-bar",
            "Card.Header",
            "ForwardRef(Button)",
            "App",
        ] {
            assert!(real_component(real), "{real:?}");
        }
        let mut a = at("http://a/1", 1);
        a.element.source = Some(SourceHint {
            file: String::new(),
            line: None,
            framework: "React".into(),
            component: Some("$u".into()),
        });
        assert_eq!(source_line(&a), None, "nothing worth a line");
        a.element.source = Some(SourceHint {
            file: "src/Hero.tsx".into(),
            line: Some(3),
            framework: "React".into(),
            component: Some("$u".into()),
        });
        assert_eq!(source_line(&a).as_deref(), Some("src/Hero.tsx:3"));
    }

    /// A group header shows the page's path only when it says more than the title: not the home
    /// page's `/`. A folded card shows the first line of its note that has words.
    #[test]
    fn headers_and_folded_cards_say_only_what_adds() {
        assert_eq!(route_line("/"), None);
        assert_eq!(
            route_line("/pricing?plan=pro").as_deref(),
            Some("/pricing?plan=pro")
        );
        assert_eq!(first_line("\n  fix this.\nand that"), "fix this.");
        assert_eq!(first_line(" \n "), "");
    }

    fn agent(pane: u64, kind: AgentKind, focused: bool) -> RunningAgent {
        RunningAgent {
            tab: gpui_kit::WeakEntity::new_invalid(),
            pane,
            kind,
            label: format!("project-{pane}").into(),
            focused,
        }
    }

    /// A card folds and opens from its head, by click or key; Collapse all folds the page's
    /// cards and Expand all opens them. A new note opens; past three, the others fold. Each
    /// card keeps its state through a delete and its undo.
    #[gpui_kit::test]
    fn cards_fold_and_open(cx: &mut gpui_kit::TestAppContext) {
        use crate::browser::panel::PANEL_WIDTH;
        use gpui_kit::{VisualTestContext, px, size};
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(PANEL_WIDTH), px(900.)), |window, cx| {
            let mut panel = AnnotatePanel::new(window, cx);
            panel.add(at("http://a/1", 1), false, window, cx);
            panel.add(at("http://a/1", 2), false, window, cx);
            panel
        });
        let panel = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let folded = |cx: &mut VisualTestContext| panel.read_with(cx, |p, _| p.collapsed());
        assert_eq!(folded(&mut cx), [false, false], "new notes open");
        assert!(cx.debug_bounds("annotate-fold").is_some(), "two notes");

        // A click on a card's head folds it, and opens it again.
        let head = cx.debug_bounds("annotate-card-0").unwrap();
        cx.simulate_click(head.center(), gpui_kit::Modifiers::none());
        assert_eq!(folded(&mut cx), [true, false]);
        let folded_head = cx.debug_bounds("annotate-card-0").unwrap();
        assert!(
            folded_head.size.height < px(40.),
            "folded to a line: {folded_head:?}"
        );
        cx.simulate_click(folded_head.center(), gpui_kit::Modifiers::none());
        assert_eq!(folded(&mut cx), [false, false]);

        // With the keyboard on the head, Return and Space fold and open it.
        cx.update(|window, cx| {
            let head = panel.read(cx).rows[0].head.clone();
            window.focus(&head, cx);
        });
        cx.run_until_parked();
        let press = |key: &str, cx: &mut VisualTestContext| {
            use gpui_kit::{KeyDownEvent, KeyUpEvent, Keystroke};
            let keystroke =
                cx.update(|_, cx| Keystroke::parse_with_os(key, cx.platform_info().os).unwrap());
            cx.simulate_event(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            cx.simulate_event(KeyUpEvent { keystroke });
            cx.run_until_parked();
        };
        press("enter", &mut cx);
        assert_eq!(folded(&mut cx), [true, false], "Return");
        press("space", &mut cx);
        assert_eq!(folded(&mut cx), [false, false], "Space");

        // Collapse all, then Expand all.
        let fold = cx.debug_bounds("annotate-fold").unwrap();
        cx.simulate_click(fold.center(), gpui_kit::Modifiers::none());
        assert_eq!(folded(&mut cx), [true, true]);
        cx.simulate_click(fold.center(), gpui_kit::Modifiers::none());
        assert_eq!(folded(&mut cx), [false, false]);

        // A third note opens with the others; a fourth folds the three before it.
        cx.update(|window, cx| {
            panel.update(cx, |p, cx| {
                p.toggle(1, cx);
                p.add(at("http://a/1", 3), false, window, cx);
            })
        });
        assert_eq!(
            folded(&mut cx),
            [false, true, false],
            "kept as the user left them"
        );
        cx.update(|window, cx| {
            panel.update(cx, |p, cx| p.add(at("http://a/1", 4), false, window, cx))
        });
        assert_eq!(folded(&mut cx), [true, true, true, false]);

        // A deleted card comes back as it was.
        cx.update(|_, cx| {
            panel.update(cx, |p, cx| {
                p.toggle(0, cx);
                p.remove(0, cx);
                p.undo(cx);
            })
        });
        assert_eq!(folded(&mut cx), [false, true, true, false]);
    }

    /// The send button follows the agents running in the terminals: none, it opens one (with a
    /// menu of the CLIs installed, when there are several); one, it sends to it; several, it sends
    /// to the default and its menu lists them all. Without notes it is there, disabled, and says
    /// why.
    #[gpui_kit::test]
    fn the_send_button_follows_the_agents(cx: &mut gpui_kit::TestAppContext) {
        use crate::browser::panel::PANEL_WIDTH;
        use AgentKind::{ClaudeCode, Codex, GeminiCli};
        use gpui_kit::{VisualTestContext, px, size};
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(PANEL_WIDTH), px(700.)), AnnotatePanel::new);
        let panel = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let action = |cx: &mut VisualTestContext| panel.read_with(cx, |p, _| p.send_action());
        let drawn = |cx: &mut VisualTestContext| {
            [
                "annotate-send-one",
                "annotate-send-split",
                "annotate-open-one",
                "annotate-open-split",
                "annotate-send-none",
            ]
            .into_iter()
            .filter(|s| cx.debug_bounds(s).is_some())
            .collect::<Vec<_>>()
        };
        // Still looking for the CLIs, and no notes: disabled, with why.
        assert_eq!(action(&mut cx), SendAction::NoAgent { looking: true });
        assert_eq!(drawn(&mut cx), ["annotate-send-none"]);
        assert!(cx.debug_bounds("annotate-note").is_some(), "says why");

        cx.update(|window, cx| {
            panel.update(cx, |p, cx| {
                p.add(at("http://a/1", 1), false, window, cx);
                p.set_installed(vec![], cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(action(&mut cx), SendAction::NoAgent { looking: false });
        assert!(
            cx.debug_bounds("annotate-note").is_some(),
            "says what to install"
        );

        // None running: Open, Claude Code first; a menu with two CLIs or more.
        cx.update(|_, cx| panel.update(cx, |p, cx| p.set_installed(vec![Codex], cx)));
        cx.run_until_parked();
        assert_eq!(action(&mut cx), SendAction::Open { kind: Codex });
        assert_eq!(drawn(&mut cx), ["annotate-open-one"]);
        cx.update(|_, cx| panel.update(cx, |p, cx| p.set_installed(vec![Codex, ClaudeCode], cx)));
        cx.run_until_parked();
        assert_eq!(action(&mut cx), SendAction::Open { kind: ClaudeCode });
        assert_eq!(drawn(&mut cx), ["annotate-open-split"]);
        assert!(
            cx.debug_bounds("annotate-note").is_none(),
            "nothing in the way"
        );

        // One running: Send to it, no menu.
        cx.update(|_, cx| {
            panel.update(cx, |p, cx| {
                p.set_agents(vec![agent(1, GeminiCli, false)], cx)
            })
        });
        cx.run_until_parked();
        assert_eq!(action(&mut cx), SendAction::Send { default: 0 });
        assert_eq!(drawn(&mut cx), ["annotate-send-one"]);

        // Two: a split button, sending to the one in front.
        cx.update(|_, cx| {
            panel.update(cx, |p, cx| {
                p.set_agents(vec![agent(1, ClaudeCode, false), agent(2, Codex, true)], cx)
            })
        });
        cx.run_until_parked();
        assert_eq!(action(&mut cx), SendAction::Send { default: 1 });
        assert_eq!(drawn(&mut cx), ["annotate-send-split"]);

        // Sending asks for that agent, which stays the default while it runs.
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = asked.clone();
        cx.update(|_, cx| {
            cx.subscribe(&panel, move |_, event: &super::AnnotateEvent, _| {
                if let super::AnnotateEvent::SendTo(agent) = event {
                    seen.borrow_mut().push(agent.pane);
                }
            })
            .detach()
        });
        cx.update(|_, cx| panel.update(cx, |p, cx| p.send_to(1, cx)));
        assert_eq!(*asked.borrow(), [1]);
        cx.update(|_, cx| {
            panel.update(cx, |p, cx| {
                p.set_agents(
                    vec![agent(1, ClaudeCode, false), agent(2, Codex, false)],
                    cx,
                )
            })
        });
        assert_eq!(
            action(&mut cx),
            SendAction::Send { default: 0 },
            "the last used"
        );
        // It ended: forgotten.
        cx.update(|_, cx| panel.update(cx, |p, cx| p.set_agents(vec![agent(2, Codex, false)], cx)));
        cx.update(|_, cx| {
            panel.update(cx, |p, cx| {
                p.set_agents(
                    vec![agent(1, ClaudeCode, false), agent(2, Codex, false)],
                    cx,
                )
            })
        });
        assert_eq!(
            action(&mut cx),
            SendAction::Send { default: 1 },
            "the newest"
        );
    }
}
