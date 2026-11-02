//! Design extraction, orchestrated: the page's `scan`, Sawit's fetch of the sheets the page
//! couldn't read (fed back through `rules`), manifest icons, then tokens — cached per page.
//! Inspect asks the page's `style` op; exports run in the background (assets, screenshot,
//! files) with progress.

mod assets;
mod export;
mod inspect;
mod ops;
mod scan;

#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::time::Duration;

use futures::channel::oneshot;
use futures::future::Shared;
use gpui_kit::{AsyncApp, Context, SharedString, Task};
use sawit_core::annotations::page_key;
use sawit_core::site_design::scan::Scan;
use sawit_core::site_design::tokens::Tokens;

use self::export::ExportJob;
use crate::browser::{BrowserView, OpResult};

pub const NOTHING: &str = "Nothing to extract on this page.";
/// A page answer (or a screenshot) that hasn't come by then won't: the job ends with an error,
/// so a lost callback can't keep a scan or an export running forever.
const PAGE_ANSWER_TIMEOUT: Duration = Duration::from_secs(25);

pub struct Ready {
    pub scan: Scan,
    pub tokens: Tokens,
}

#[derive(Clone)]
pub enum ScanState {
    Idle,
    Scanning,
    Ready(Rc<Ready>),
    Failed(SharedString),
}

type ScanJob = Shared<Task<Result<Rc<Ready>, String>>>;

/// The browser tab's design state.
pub(crate) struct DesignFlow {
    state: ScanState,
    /// The page (`page_key`) `state` and `job` belong to.
    key: String,
    /// Bumped when the page changes, so a scan of the old page lands nowhere.
    generation: u64,
    job: Option<ScanJob>,
    /// The latest inspect; older answers are dropped.
    inspect_seq: u64,
    /// The element the last hover asked about.
    hover_req: Option<String>,
    /// Pins asked for and not answered yet.
    pins_pending: u32,
    export: Option<ExportJob>,
}

impl Default for DesignFlow {
    fn default() -> Self {
        Self {
            state: ScanState::Idle,
            key: String::new(),
            generation: 0,
            job: None,
            inspect_seq: 0,
            hover_req: None,
            pins_pending: 0,
            export: None,
        }
    }
}

impl DesignFlow {
    /// The page changed: forget its scan.
    fn reset(&mut self) {
        self.generation += 1;
        self.state = ScanState::Idle;
        self.key.clear();
        self.job = None;
        self.hover_req = None;
    }

    fn export_running(&self) -> bool {
        self.export.as_ref().is_some_and(|j| j.result.is_none())
    }
}

impl BrowserView {
    /// Hands the scan state to the Design panel.
    pub(in crate::browser) fn sync_design(&mut self, cx: &mut Context<Self>) {
        let state = self.design.state.clone();
        self.design_panel
            .update(cx, |panel, cx| panel.set_scan(state, cx));
        self.sync_export(cx);
    }

    /// With the Design panel open, scans the page (a cached scan answers at once).
    pub(in crate::browser) fn scan_if_design_open(&mut self, cx: &mut Context<Self>) {
        if self.panel == Some(crate::browser::Panel::Design) {
            self.scan_design(cx).detach();
        }
        self.sync_design(cx);
    }

    /// A load started: the next scan starts over.
    pub(in crate::browser) fn design_load_started(&mut self) {
        self.design.reset();
    }

    /// The URL changed without a load: a scan of another page is dropped.
    pub(in crate::browser) fn design_url_changed(&mut self) {
        if self.design.key != page_key(&self.current_url()) {
            self.design.reset();
        }
    }

    /// Shows `text` in the Design panel's footer.
    pub(in crate::browser) fn design_note(&mut self, text: String, cx: &mut Context<Self>) {
        self.design_panel.update(cx, |p, cx| p.set_note(text, cx));
    }
}

fn gone<E>(_: E) -> String {
    "The browser tab was closed.".into()
}

fn cancelled<E>(_: E) -> String {
    "The scan was cancelled.".into()
}

/// The page's answer to an op, within [`PAGE_ANSWER_TIMEOUT`].
async fn page_answer(answer: oneshot::Receiver<OpResult>, cx: &AsyncApp) -> OpResult {
    let timer = cx.background_executor().timer(PAGE_ANSWER_TIMEOUT);
    answer_or_timeout(answer, timer).await
}

/// `answer`, unless `timer` fires first.
async fn answer_or_timeout(
    answer: oneshot::Receiver<OpResult>,
    timer: impl std::future::Future<Output = ()> + Unpin,
) -> OpResult {
    match futures::future::select(answer, timer).await {
        futures::future::Either::Left((answer, _)) => {
            answer.unwrap_or_else(|_| Err("The page didn't answer.".into()))
        }
        futures::future::Either::Right(_) => Err(format!(
            "The page didn't answer within {} s.",
            PAGE_ANSWER_TIMEOUT.as_secs()
        )),
    }
}

/// A page answer read strictly as the core type; a mismatch is the contract breaking.
fn parse<T>(op: &str, read: serde_json::Result<T>) -> Result<T, String> {
    read.map_err(|e| format!("The page's {op} answer couldn't be read: {e}"))
}

/// Only web pages and local files have styles to read.
fn extractable(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("file://")
}
