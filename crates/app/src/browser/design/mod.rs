//! Design extraction in the browser tab: the scan, inspect and export flow ([`flow`]), over
//! `sawit_core::site_design`, and the Design panel's views.
//!
//! The panel is five tabs on a rule, kept in view while the tab under them scrolls. Each tab
//! is built from the side panel's pieces (`super::panel`): titled sections, label/value rows on
//! one column, actions as buttons that say what they do, and a loading and an empty state that
//! say what is missing and what to do.

mod assets;
mod colors;
pub(crate) mod flow;
mod fonts;
mod inspect;
mod system;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::site_design::color::Rgba;
use sawit_core::site_design::inspect::StyleInfo;
use sawit_core::site_design::scan::{AssetKind, AssetRef};

use self::flow::{NOTHING, ScanState};
use super::panel::{self, note_timer};

/// The panel's segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Inspect,
    Colors,
    Fonts,
    Assets,
    System,
}

impl Segment {
    const ALL: [Segment; 5] = [
        Segment::Inspect,
        Segment::Colors,
        Segment::Fonts,
        Segment::Assets,
        Segment::System,
    ];

    fn title(self) -> &'static str {
        match self {
            Segment::Inspect => "Inspect",
            Segment::Colors => "Colors",
            Segment::Fonts => "Fonts",
            Segment::Assets => "Assets",
            Segment::System => "System",
        }
    }

    /// The name scripts use (`browser` `panel` op): `inspect`, `colors`…
    pub fn name(self) -> &'static str {
        match self {
            Segment::Inspect => "inspect",
            Segment::Colors => "colors",
            Segment::Fonts => "fonts",
            Segment::Assets => "assets",
            Segment::System => "system",
        }
    }

    /// The segment a script names, in any case; `colours` too.
    pub fn named(name: &str) -> Option<Segment> {
        let name = name.trim().to_ascii_lowercase();
        let name = if name == "colours" { "colors" } else { &name };
        Self::ALL.into_iter().find(|s| s.name() == name)
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
    /// Download every asset (videos only when asked).
    SaveAll { include_videos: bool },
    /// Write the export folder.
    Export { include_videos: bool },
    /// Show this file in Finder.
    Reveal(std::path::PathBuf),
    /// This segment was chosen (again, maybe): Inspect picks, the others don't.
    Segment(Segment),
    /// Read the page again (it changed since, or the last read failed).
    Rescan,
    /// Fetch previews of these assets for the Assets tab.
    Thumbnails(Vec<AssetRef>),
    /// Inspect and pin this selector again.
    Pin(String),
}

/// What an export that ended wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportDone {
    pub files: usize,
    pub failed: usize,
    /// Videos left out (not asked for); not failures.
    pub skipped_videos: usize,
    pub screenshot_error: Option<String>,
}

/// The export as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportShown {
    pub folder: std::path::PathBuf,
    pub done: usize,
    pub total: usize,
    /// None while it runs.
    pub outcome: Option<Result<ExportDone, String>>,
}

/// An asset's preview in the Assets tab.
#[derive(Clone)]
pub enum Thumb {
    Loading,
    Ready(Arc<Image>),
    /// Not an image GPUI draws, or the download failed: the kind's icon stands in.
    None,
}

pub struct DesignPanel {
    segment: Segment,
    scan: ScanState,
    hovered: Option<StyleInfo>,
    pinned: Option<StyleInfo>,
    /// The element pinned last, after Unpin: Inspect's empty state offers it again.
    last_pinned: Option<StyleInfo>,
    /// The page picks for Inspect (it doesn't after Esc).
    inspecting: bool,
    asset_kind: Option<AssetKind>,
    include_videos: bool,
    export: Option<ExportShown>,
    /// Previews by asset key ([`assets::thumb_key`]).
    thumbs: HashMap<String, Thumb>,
    /// Fonts whose every size and line height is shown.
    expanded: HashSet<String>,
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
            last_pinned: None,
            inspecting: false,
            asset_kind: None,
            include_videos: false,
            export: None,
            thumbs: HashMap::new(),
            expanded: HashSet::new(),
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
            self.last_pinned = None;
            self.asset_kind = None;
            self.thumbs.clear();
        }
        self.scan = scan;
        self.request_thumbnails(cx);
        cx.notify();
    }

    /// The export's progress, when it changed.
    pub fn set_export(&mut self, export: Option<ExportShown>, cx: &mut Context<Self>) {
        if self.export != export {
            self.export = export;
            cx.notify();
        }
    }

    pub fn segment(&self) -> Segment {
        self.segment
    }

    /// Shows `segment`, as a click on its tab does.
    pub fn set_segment(&mut self, segment: Segment, cx: &mut Context<Self>) {
        self.segment = segment;
        cx.emit(DesignEvent::Segment(segment));
        self.request_thumbnails(cx);
        cx.notify();
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned.is_some()
    }

    /// Whether the page picks for Inspect.
    pub fn set_inspecting(&mut self, inspecting: bool, cx: &mut Context<Self>) {
        if self.inspecting != inspecting {
            self.inspecting = inspecting;
            cx.notify();
        }
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
        self.last_pinned = self.pinned.take();
        self.hovered = None;
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

    /// Puts `text` on the clipboard, saying what was copied.
    fn copy(&mut self, text: String, cx: &mut Context<Self>) {
        let shown = copied_note(&text);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.set_note(shown, cx);
    }

    /// Previews fetched for the Assets tab: `(key, bytes)`, `None` where none came.
    pub fn set_thumbnails(&mut self, got: Vec<(String, Option<Vec<u8>>)>, cx: &mut Context<Self>) {
        for (key, bytes) in got {
            let thumb = bytes
                .and_then(|b| assets::image_format(&b).map(|f| Arc::new(Image::from_bytes(f, b))))
                .map_or(Thumb::None, Thumb::Ready);
            self.thumbs.insert(key, thumb);
        }
        cx.notify();
    }

    /// The Assets tab is on show: previews of what it lists that are on the web and not yet
    /// fetched are asked for; inline SVG and `data:` ones are made here.
    fn request_thumbnails(&mut self, cx: &mut Context<Self>) {
        if self.segment != Segment::Assets {
            return;
        }
        let ScanState::Ready(ready) = &self.scan else {
            return;
        };
        let kind = self
            .asset_kind
            .unwrap_or_else(|| assets::first_kind(&ready.scan));
        let mut wanted = Vec::new();
        for a in assets::shown(&ready.scan, kind)
            .into_iter()
            .take(assets::MAX_THUMBS)
        {
            let key = assets::thumb_key(a);
            if self.thumbs.contains_key(&key) {
                continue;
            }
            match assets::local_thumb(a) {
                Some(thumb) => {
                    self.thumbs.insert(key, thumb);
                }
                None if assets::fetchable(a) => {
                    self.thumbs.insert(key, Thumb::Loading);
                    wanted.push(a.clone());
                }
                None => {
                    self.thumbs.insert(key, Thumb::None);
                }
            }
        }
        if !wanted.is_empty() {
            cx.emit(DesignEvent::Thumbnails(wanted));
        }
    }

    /// The tabs, on a rule, with the rescan button at their end.
    fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = Segment::ALL
            .iter()
            .position(|s| *s == self.segment)
            .unwrap_or(0);
        let scanning = matches!(self.scan, ScanState::Scanning);
        h_flex()
            .flex_none()
            .w_full()
            .pl_3()
            .pr_1()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                TabBar::new("design-tabs")
                    .underline()
                    .small()
                    .flex_1()
                    .min_w_0()
                    .selected_index(current)
                    .last_empty_space(div())
                    .children(Segment::ALL.iter().map(|s| {
                        let name = s.name();
                        Tab::new()
                            .label(s.title())
                            .debug_selector(move || format!("design-tab-{name}"))
                    }))
                    .on_click(cx.listener(|this, ix: &usize, _, cx| {
                        if let Some(s) = Segment::ALL.get(*ix) {
                            this.set_segment(*s, cx);
                        }
                    })),
            )
            .child(
                div().child(
                    Button::new("design-rescan")
                        .debug_selector(|| "design-rescan".into())
                        .ghost()
                        .xsmall()
                        .icon(Lucide::RefreshCw)
                        .loading(scanning)
                        .tooltip("Read the page again")
                        .accessibility_label("Read the page again")
                        .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                            cx.emit(DesignEvent::Rescan);
                        })),
                ),
            )
    }

    fn body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.segment == Segment::Inspect {
            let pinned = self.pinned.is_some();
            let info = self.pinned.clone().or_else(|| self.hovered.clone());
            return inspect::render(
                info.as_ref(),
                pinned,
                self.last_pinned.as_ref(),
                self.inspecting,
                cx,
            )
            .into_any_element();
        }
        match &self.scan {
            ScanState::Idle => panel::empty(
                NOTHING,
                "Open a web page in this tab; the panel reads its colors, fonts, assets and \
                 design tokens.",
                None,
                cx,
            )
            .into_any_element(),
            ScanState::Scanning => panel::loading(6, "Reading the page…", cx).into_any_element(),
            ScanState::Failed(e) => panel::empty(
                "Couldn't read the page",
                e.clone(),
                Some(
                    Button::new("design-retry")
                        .outline()
                        .xsmall()
                        .icon(Lucide::RefreshCw)
                        .label("Try again")
                        .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                            cx.emit(DesignEvent::Rescan);
                        }))
                        .into_any_element(),
                ),
                cx,
            )
            .into_any_element(),
            ScanState::Ready(ready) => {
                let ready = ready.clone();
                match self.segment {
                    Segment::Colors => colors::render(&ready.scan, cx).into_any_element(),
                    Segment::Assets => {
                        let kind = self
                            .asset_kind
                            .unwrap_or_else(|| assets::first_kind(&ready.scan));
                        assets::render(&ready.scan, kind, self.include_videos, &self.thumbs, cx)
                            .into_any_element()
                    }
                    Segment::System => {
                        let export = self.export.clone();
                        system::render(&ready.tokens, export.as_ref(), self.include_videos, cx)
                            .into_any_element()
                    }
                    _ => fonts::render(&ready.scan, &ready.tokens, &self.expanded, cx)
                        .into_any_element(),
                }
            }
        }
    }
}

impl Render for DesignPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let body = self.body(cx);
        v_flex()
            .size_full()
            .child(self.tabs(cx))
            .child(panel::scroll("design-body").child(body))
            .children(self.note.clone().map(|note| {
                h_flex()
                    .flex_none()
                    .min_h(px(32.))
                    .px_3()
                    .gap_2()
                    .items_center()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(panel::glyph(Lucide::Check, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .child(note),
                    )
            }))
    }
}

/// What the footer says after a copy: the text itself when it is short, else its start.
fn copied_note(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    let more = text.lines().nth(1).is_some();
    if line.is_empty() {
        return "Copied".into();
    }
    match line.char_indices().nth(48) {
        Some((i, _)) => format!("Copied {}…", &line[..i]),
        None if more => format!("Copied {line} …"),
        None => format!("Copied {line}"),
    }
}

/// A colour's swatch: the colour over the panel, framed so a colour close to the panel's
/// still shows its edge.
fn swatch(c: &Rgba, size: Pixels, cx: &App) -> Div {
    div()
        .size(size)
        .flex_none()
        .rounded(px(4.))
        .border_1()
        .border_color(cx.theme().border)
        .bg(gpui_kit::rgba(colors::rgba_u32(c)))
}

/// `#132643`, and its opacity apart (`60%`) when it has one: a hex with alpha is hard to read.
pub(super) fn hex_and_alpha(c: &Rgba) -> (String, Option<String>) {
    let hex = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    let alpha = (c.a < 0.995).then(|| format!("{}%", (f64::from(c.a) * 100.).round()));
    (hex, alpha)
}

/// A row whose value copies on click: `label`, then `value`, with a copy mark that shows on
/// hover. `copy` is what goes to the clipboard.
fn copy_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: impl IntoElement,
    copy: String,
    cx: &mut Context<DesignPanel>,
) -> AnyElement {
    copy_row_at(id, label, panel::LABEL_WIDTH, value, copy, cx)
}

/// [`copy_row`] with a label column `label_width` wide, for a section whose names run long
/// (`primary-foreground`), so its values still start on one edge.
fn copy_row_at(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    label_width: f32,
    value: impl IntoElement,
    copy: String,
    cx: &mut Context<DesignPanel>,
) -> AnyElement {
    let id = id.into();
    let group: SharedString = format!("copy-{id:?}").into();
    let theme = cx.theme();
    h_flex()
        .id(id)
        .group(group.clone())
        .min_h(px(24.))
        .px_1()
        .mx(px(-4.))
        .gap_2()
        .items_center()
        .rounded(px(4.))
        .text_xs()
        .cursor_pointer()
        .hover(|el| el.bg(theme.secondary))
        // The label column, wider for a long name (`primary-foreground`) than for the rest.
        .child(
            div()
                .min_w(px(label_width - 4.))
                .max_w(px(180.))
                .flex_none()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(label.into()),
        )
        .child(div().flex_1().min_w_0().child(value))
        .child(
            div()
                .flex_none()
                .invisible()
                .group_hover(group, |s| s.visible())
                .child(panel::glyph(Lucide::Copy, cx)),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.copy(copy.clone(), cx);
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{DesignPanel, Segment, copied_note, hex_and_alpha};
    use crate::browser::panel::PANEL_WIDTH;
    use gpui_kit::{TestAppContext, VisualTestContext, px, size};
    use sawit_core::site_design::color;

    /// The five tabs and the rescan button fit the panel's width side by side: none clipped,
    /// none under another.
    #[gpui_kit::test]
    fn the_tabs_fit_the_panel(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(PANEL_WIDTH), px(600.)), |_, _| DesignPanel::new());
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let mut right = px(0.);
        for selector in [
            "design-tab-inspect",
            "design-tab-colors",
            "design-tab-fonts",
            "design-tab-assets",
            "design-tab-system",
        ] {
            let tab = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} isn't drawn"));
            assert!(tab.left() >= right, "{selector} overlaps the tab before it");
            right = tab.right();
        }
        let rescan = cx.debug_bounds("design-rescan").expect("no rescan button");
        assert!(
            right <= rescan.left(),
            "the last tab runs under the rescan button"
        );
        assert!(
            rescan.right() <= px(PANEL_WIDTH),
            "the rescan button is clipped"
        );
    }

    #[test]
    fn segments_keep_their_order() {
        let titles: Vec<&str> = Segment::ALL.iter().map(|s| s.title()).collect();
        assert_eq!(titles, ["Inspect", "Colors", "Fonts", "Assets", "System"]);
        assert_eq!(Segment::named("Colours"), Some(Segment::Colors));
        assert_eq!(Segment::named(" system "), Some(Segment::System));
        assert_eq!(Segment::named("layers"), None);
        for s in Segment::ALL {
            assert_eq!(Segment::named(s.name()), Some(s));
        }
    }

    /// An alpha hex (`#13264399`) reads as the colour and its opacity apart.
    #[test]
    fn alpha_reads_apart_from_the_hex() {
        let c = color::parse("#13264399").unwrap();
        assert_eq!(hex_and_alpha(&c), ("#132643".into(), Some("60%".into())));
        let c = color::parse("rgb(19, 38, 67)").unwrap();
        assert_eq!(hex_and_alpha(&c), ("#132643".into(), None));
    }

    #[test]
    fn the_footer_names_what_was_copied() {
        assert_eq!(copied_note("#0071e3"), "Copied #0071e3");
        assert_eq!(copied_note(".a {\n  color: red;\n}"), "Copied .a { …");
        assert_eq!(
            copied_note(&"x".repeat(60)),
            format!("Copied {}…", "x".repeat(48))
        );
        assert_eq!(copied_note(""), "Copied");
    }
}
