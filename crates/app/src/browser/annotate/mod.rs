//! The Annotate panel: one card per element the user picked, with a comment, an intent and a
//! screenshot, grouped by page. Notes are written in a popover on the page itself
//! (`browser/page/overlay.js`); the panel is their list, kept in step both ways. `BrowserView` owns
//! the flow around it (pick, capture, markers, copy); the panel owns the rows, and where the
//! notes go: an agent running in a terminal, or a new one ([`super::agents`]).

mod card;
mod rows;
mod send;
mod view;

#[cfg(test)]
mod tests;

use gpui_kit::component::input::TextareaState;
use gpui_kit::*;
use sawit_core::annotations::{Annotation, Intent, Rect, page_key};
use sawit_core::terminal::{AgentKind, PaneId};

use super::agents::AgentSource;
use super::panel::note_timer;
use crate::terminal::RunningAgent;

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

    pub fn include_errors(&self) -> bool {
        self.include_errors
    }
}

/// What a prompt says is lost with `n` notes.
pub fn notes_lost(n: usize) -> String {
    format!(
        "{n} note{} on the page {} been copied or sent.",
        if n == 1 { "" } else { "s" },
        if n == 1 { "hasn't" } else { "haven't" }
    )
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

/// The page keys of the notes at `urls`: `current` first, then in the order they were first
/// annotated.
pub(super) fn group_order<'a>(
    current: &str,
    urls: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let current = page_key(current);
    let mut keys: Vec<String> = Vec::new();
    for url in urls {
        let key = page_key(url);
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
