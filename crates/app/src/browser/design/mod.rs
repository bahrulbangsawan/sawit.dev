//! Design extraction in the browser tab: the scan, inspect and export flow ([`flow`]), over
//! `sawit_core::site_design`, and the Design panel's views.

mod colors;
pub(crate) mod flow;
mod fonts;
mod inspect;

use gpui_kit::component::button::*;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::site_design::inspect::StyleInfo;
use sawit_core::site_design::scan::AssetRef;

use self::flow::{NOTHING, ScanState};
use super::panel::note_timer;

/// The panel's segments. Assets and System join in the next step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Inspect,
    Colors,
    Fonts,
}

impl Segment {
    const ALL: [Segment; 3] = [Segment::Inspect, Segment::Colors, Segment::Fonts];

    fn title(self) -> &'static str {
        match self {
            Segment::Inspect => "Inspect",
            Segment::Colors => "Colors",
            Segment::Fonts => "Fonts",
        }
    }
}

/// What the panel asks of the browser.
pub enum DesignEvent {
    /// Flash this selector on the page.
    Flash(String),
    /// Copy this element's markup with computed styles inlined.
    CopyHtml(String),
    /// Download these files.
    SaveAssets(Vec<AssetRef>),
}

pub struct DesignPanel {
    segment: Segment,
    scan: ScanState,
    hovered: Option<StyleInfo>,
    pinned: Option<StyleInfo>,
    note: Option<SharedString>,
    _note_timer: Option<Task<()>>,
}

impl EventEmitter<DesignEvent> for DesignPanel {}

impl DesignPanel {
    pub fn new() -> Self {
        Self {
            segment: Segment::Inspect,
            scan: ScanState::Idle,
            hovered: None,
            pinned: None,
            note: None,
            _note_timer: None,
        }
    }

    /// The browser's scan state, when it changed. A new page forgets what was inspected.
    pub fn set_scan(&mut self, scan: ScanState, cx: &mut Context<Self>) {
        let same = match (&self.scan, &scan) {
            (ScanState::Idle, ScanState::Idle) | (ScanState::Scanning, ScanState::Scanning) => true,
            (ScanState::Ready(a), ScanState::Ready(b)) => std::rc::Rc::ptr_eq(a, b),
            (ScanState::Failed(a), ScanState::Failed(b)) => a == b,
            _ => false,
        };
        if same {
            return;
        }
        if matches!(scan, ScanState::Idle | ScanState::Scanning) {
            self.hovered = None;
            self.pinned = None;
        }
        self.scan = scan;
        cx.notify();
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned.is_some()
    }

    pub fn set_hovered(&mut self, info: StyleInfo, cx: &mut Context<Self>) {
        self.hovered = Some(info);
        cx.notify();
    }

    /// A pick in inspect mode: shows `info` until unpinned.
    pub fn pin(&mut self, info: StyleInfo, cx: &mut Context<Self>) {
        self.segment = Segment::Inspect;
        self.pinned = Some(info);
        cx.notify();
    }

    pub fn unpin(&mut self, cx: &mut Context<Self>) {
        self.pinned = None;
        cx.notify();
    }

    /// Shows `text` in the footer for a few seconds.
    pub fn set_note(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.note = Some(text.into());
        self._note_timer = Some(note_timer(cx, |this: &mut Self| this.note = None));
        cx.notify();
    }

    /// Shows `text` until the next note replaces it (work in progress).
    pub fn hold_note(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.note = Some(text.into());
        self._note_timer = None;
        cx.notify();
    }

    /// Puts `text` on the clipboard.
    fn copy(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.set_note("Copied", cx);
    }

    fn body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        if self.segment == Segment::Inspect {
            let pinned = self.pinned.is_some();
            let info = self.pinned.clone().or_else(|| self.hovered.clone());
            return inspect::render(info.as_ref(), pinned, cx).into_any_element();
        }
        match &self.scan {
            ScanState::Idle => div()
                .text_sm()
                .text_color(muted)
                .child(NOTHING)
                .into_any_element(),
            ScanState::Scanning => h_flex()
                .gap_1p5()
                .items_center()
                .text_sm()
                .text_color(muted)
                .child(Spinner::new().xsmall().color(muted))
                .child("Reading the page…")
                .into_any_element(),
            ScanState::Failed(e) => div()
                .text_sm()
                .text_color(muted)
                .child(e.clone())
                .into_any_element(),
            ScanState::Ready(ready) => {
                let ready = ready.clone();
                match self.segment {
                    Segment::Colors => colors::render(&ready.scan, cx).into_any_element(),
                    _ => fonts::render(&ready.scan, &ready.tokens, cx).into_any_element(),
                }
            }
        }
    }
}

impl Render for DesignPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let current = self.segment;
        let segments = h_flex()
            .gap_1()
            .children(Segment::ALL.into_iter().enumerate().map(|(ix, s)| {
                Button::new(("design-segment", ix))
                    .xsmall()
                    .label(s.title())
                    .selected(current == s)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.segment = s;
                        cx.notify();
                    }))
            }));
        let body = self.body(cx);
        v_flex().gap_3().child(segments).child(body).children(
            self.note
                .clone()
                .map(|note| div().text_xs().text_color(muted).child(note)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Segment;

    #[test]
    fn segments_keep_their_order() {
        let titles: Vec<&str> = Segment::ALL.iter().map(|s| s.title()).collect();
        assert_eq!(titles, ["Inspect", "Colors", "Fonts"]);
    }
}
