//! The SEO panel: the scan of the page on show (`sawit_core::web::seo`), drawn natively. Its tabs
//! are an overview (the scores, what to fix, what the page says about itself), previews of how
//! the link looks when shared, and every search and sharing check with its evidence and fix.
//! Built from the side panel's pieces (`super::panel`), like Annotate and Design.

mod agents;
pub(super) mod flow;
mod overview;
mod previews;
mod rows;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::web::seo::{CheckResult, Report, Status};

use super::panel::{self, note_timer};

/// The panel's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeoTab {
    Overview,
    Previews,
    Checks,
    Agents,
}

impl SeoTab {
    const ALL: [SeoTab; 4] = [
        SeoTab::Overview,
        SeoTab::Previews,
        SeoTab::Checks,
        SeoTab::Agents,
    ];

    fn title(self) -> &'static str {
        match self {
            SeoTab::Overview => "Overview",
            SeoTab::Previews => "Previews",
            SeoTab::Checks => "Checks",
            SeoTab::Agents => "Agents",
        }
    }

    /// The name scripts use (`browser_panel`'s `tab`).
    pub fn name(self) -> &'static str {
        match self {
            SeoTab::Overview => "overview",
            SeoTab::Previews => "previews",
            SeoTab::Checks => "checks",
            SeoTab::Agents => "agents",
        }
    }

    pub fn named(name: &str) -> Option<SeoTab> {
        let name = name.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// Where the scan stands.
#[derive(Clone)]
pub enum ScanState {
    /// The tab has no web page.
    Idle,
    Loading {
        host: SharedString,
    },
    Ready(Arc<Report>),
    Failed {
        host: SharedString,
        error: SharedString,
    },
}

/// What the panel asks of the browser.
pub enum SeoEvent {
    /// Scan the page again.
    Rescan,
    /// Scan again for another kind of site.
    ScanType(sawit_core::web::seo::ScanType),
    /// Ask the outside scanner too, or stop asking.
    External(bool),
}

impl EventEmitter<SeoEvent> for SeoPanel {}

pub struct SeoPanel {
    tab: SeoTab,
    state: ScanState,
    /// The checks shown open, by id.
    open: HashSet<String>,
    scanned_at: Option<Instant>,
    /// The share image and the favicon, for the previews.
    picture: Option<Arc<RenderImage>>,
    favicon: Option<Arc<RenderImage>>,
    /// The external score is asked for.
    external: bool,
    note: Option<SharedString>,
    _note_timer: Option<Task<()>>,
}

impl SeoPanel {
    pub fn new() -> Self {
        SeoPanel {
            tab: SeoTab::Overview,
            state: ScanState::Idle,
            open: HashSet::new(),
            scanned_at: None,
            picture: None,
            favicon: None,
            external: false,
            note: None,
            _note_timer: None,
        }
    }

    pub fn tab(&self) -> SeoTab {
        self.tab
    }

    pub fn set_tab(&mut self, tab: SeoTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    pub fn set_loading(&mut self, host: String, cx: &mut Context<Self>) {
        self.state = ScanState::Loading { host: host.into() };
        cx.notify();
    }

    pub fn set_idle(&mut self, cx: &mut Context<Self>) {
        self.state = ScanState::Idle;
        self.picture = None;
        cx.notify();
    }

    pub fn set_report(&mut self, report: Arc<Report>, cx: &mut Context<Self>) {
        self.state = match &report.error {
            Some(error) => ScanState::Failed {
                host: host_of(&report.url).into(),
                error: error.clone().into(),
            },
            None => ScanState::Ready(report),
        };
        self.scanned_at = Some(Instant::now());
        self.picture = None;
        cx.notify();
    }

    pub fn set_external(&mut self, on: bool, cx: &mut Context<Self>) {
        self.external = on;
        cx.notify();
    }

    pub fn set_pictures(
        &mut self,
        picture: Option<Arc<RenderImage>>,
        favicon: Option<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        self.picture = picture;
        self.favicon = favicon;
        cx.notify();
    }

    fn set_note(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.note = Some(text.into());
        self._note_timer = Some(note_timer(cx, |this: &mut Self| this.note = None));
        cx.notify();
    }

    fn copy(&mut self, text: String, note: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.set_note(note.to_string(), cx);
    }

    fn toggle_open(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.open.remove(id) {
            self.open.insert(id.to_string());
        }
        cx.notify();
    }

    /// The tabs on a rule, with Rescan at their end.
    fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = SeoTab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        let loading = matches!(self.state, ScanState::Loading { .. });
        let idle = matches!(self.state, ScanState::Idle);
        h_flex()
            .flex_none()
            .w_full()
            .pl_3()
            .pr_1()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                TabBar::new("seo-tabs")
                    .underline()
                    .small()
                    .flex_1()
                    .min_w_0()
                    .selected_index(current)
                    .last_empty_space(div())
                    .children(SeoTab::ALL.iter().map(|t| {
                        let name = t.name();
                        Tab::new()
                            .label(t.title())
                            .debug_selector(move || format!("seo-tab-{name}"))
                    }))
                    .on_click(cx.listener(|this, ix: &usize, _, cx| {
                        if let Some(t) = SeoTab::ALL.get(*ix) {
                            this.set_tab(*t, cx);
                        }
                    })),
            )
            .child(
                Button::new("seo-rescan")
                    .debug_selector(|| "seo-rescan".into())
                    .ghost()
                    .xsmall()
                    .icon(Lucide::RefreshCw)
                    .loading(loading)
                    .disabled(loading || idle)
                    .tooltip(if loading { "Scanning…" } else { "Scan again" })
                    .accessibility_label("Scan again")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SeoEvent::Rescan))),
            )
    }

    fn body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match self.state.clone() {
            ScanState::Idle => panel::empty(
                "No page to scan",
                "Open a web page in this tab and its search, sharing and agent checks show here.",
                None,
                cx,
            )
            .into_any_element(),
            ScanState::Loading { host } => {
                panel::loading(5, format!("Scanning {host}…"), cx).into_any_element()
            }
            ScanState::Failed { host, error } => panel::empty(
                format!("Couldn't scan {host}"),
                format!("{error} Check that the site is up, then scan again."),
                Some(
                    Button::new("seo-retry")
                        .small()
                        .icon(Lucide::RefreshCw)
                        .label("Scan again")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(SeoEvent::Rescan)))
                        .into_any_element(),
                ),
                cx,
            )
            .into_any_element(),
            ScanState::Ready(report) => match self.tab {
                SeoTab::Overview => self.overview(&report, cx).into_any_element(),
                SeoTab::Previews => {
                    previews::previews(&report, self.picture.clone(), self.favicon.clone(), cx)
                        .into_any_element()
                }
                SeoTab::Checks => self.checks(&report, cx).into_any_element(),
                SeoTab::Agents => self.agents(&report, cx).into_any_element(),
            },
        }
    }

    fn checks(&self, report: &Report, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list: Vec<&CheckResult> = report.checks.iter().filter(|c| c.seo).collect();
        list.sort_by_key(|c| rows::status_rank(c.status));
        let scored = list.iter().filter(|c| c.status.scored()).count();
        let passed = list.iter().filter(|c| c.status == Status::Pass).count();
        let to_fix = list
            .iter()
            .filter(|c| matches!(c.status, Status::Fail | Status::Warn))
            .count();
        let prompt = (to_fix > 0).then(|| sawit_core::web::seo::fix_prompt_all(report, true));
        let url = report.url.clone();
        let rows: Vec<AnyElement> = list
            .into_iter()
            .map(|c| self.check_row(c, &url, cx).into_any_element())
            .collect();
        panel::section(
            "Search and sharing",
            Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{passed} of {scored} pass")),
                    )
                    .when_some(prompt, |el, prompt| {
                        el.child(
                            Button::new("seo-checks-copy-all")
                                .xsmall()
                                .ghost()
                                .icon(Lucide::Copy)
                                .label("Copy Prompt")
                                .tooltip(format!("Copy One Prompt for the {to_fix} to Fix"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.copy(prompt.clone(), "Copied every fix as one prompt", cx)
                                })),
                        )
                    })
                    .into_any_element(),
            ),
            v_flex().gap_1().children(rows),
            cx,
        )
    }

    /// A check: its status, name, finding and points; open, its evidence and fix.
    fn check_row(&self, c: &CheckResult, url: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.open.contains(&c.id);
        let id = c.id.clone();
        let prompt = sawit_core::web::seo::fix_prompt(url, c);
        let spec = c.spec.clone();
        let theme = cx.theme();
        let header = Button::new(SharedString::from(format!("seo-check-{}", c.id)))
            .ghost()
            .w_full()
            .h_auto()
            .py_1()
            .px_1()
            .debug_selector({
                let id = c.id.clone();
                move || format!("seo-check-{id}")
            })
            .accessibility_label(format!("{}: {}", c.name, c.status.as_str()))
            .child(rows::check_line(c, open, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_open(&id, cx)));
        v_flex().child(header).when(open, |el| {
            el.child(
                v_flex()
                    .ml(px(26.))
                    .mb_1()
                    .p_2()
                    .gap_1p5()
                    .rounded(theme.radius)
                    .bg(theme.muted)
                    .text_xs()
                    .children(
                        c.evidence
                            .iter()
                            .take(8)
                            .map(|e| rows::evidence_line(e, cx)),
                    )
                    .when(!c.fix.is_empty(), |el| {
                        el.child(div().text_color(theme.foreground).child(c.fix.clone()))
                    })
                    .when(c.fix.is_empty() && c.evidence.is_empty(), |el| {
                        el.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(c.details.clone()),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .pt_1()
                            .flex_wrap()
                            .when(matches!(c.status, Status::Fail | Status::Warn), |el| {
                                el.child(
                                    Button::new(SharedString::from(format!("seo-prompt-{}", c.id)))
                                        .xsmall()
                                        .outline()
                                        .icon(Lucide::Copy)
                                        .label("Copy fix prompt")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.copy(
                                                prompt.clone(),
                                                "Copied a fix prompt for an agent",
                                                cx,
                                            )
                                        })),
                                )
                            })
                            .when(!spec.is_empty(), |el| {
                                el.child(
                                    Button::new(SharedString::from(format!("seo-spec-{}", c.id)))
                                        .xsmall()
                                        .ghost()
                                        .icon(Lucide::ExternalLink)
                                        .label("Open spec")
                                        .tooltip(spec.clone())
                                        .on_click(move |_, _, cx| cx.open_url(&spec)),
                                )
                            }),
                    ),
            )
        })
    }
}

/// The host of `url`, for headings.
fn host_of(url: &str) -> String {
    sawit_core::web::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str().map(|h| match u.port() {
                Some(p) => format!("{h}:{p}"),
                None => h.to_string(),
            })
        })
        .unwrap_or_else(|| url.to_string())
}

/// "Scanned just now", "Scanned 3 min ago".
pub(super) fn scanned_ago(secs: u64) -> String {
    match secs {
        0..=9 => "Scanned just now".into(),
        10..=59 => format!("Scanned {secs} s ago"),
        60..=3599 => format!("Scanned {} min ago", secs / 60),
        _ => format!("Scanned {} h ago", secs / 3600),
    }
}

impl Render for SeoPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let body = self.body(cx);
        v_flex()
            .size_full()
            .child(self.tabs(cx))
            .child(panel::scroll("seo-body").child(body))
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
