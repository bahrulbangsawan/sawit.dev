//! Design extraction, orchestrated: the page's `scan`, Sawit's fetch of the sheets the page
//! couldn't read (fed back through `rules`), manifest icons, then tokens — cached per page.
//! Inspect asks the page's `style` op; exports run in the background (assets, screenshot,
//! files) with progress.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::channel::oneshot;
use futures::future::Shared;
use gpui_kit::{AsyncApp, Context, SharedString, Task, WeakEntity};
use sawit_core::annotations::page_key;
use sawit_core::site_design::assets::{
    MAX_TOTAL, download_all, fetch_text, font_assets, is_skipped_video, may_fetch_for,
    sheet_for_page,
};
use sawit_core::site_design::export::write_folder;
use sawit_core::site_design::inspect::{StyleInfo, contrast, css_rule};
use sawit_core::site_design::scan::{AssetKind, AssetOutcome, AssetRef, Scan, SheetRules};
use sawit_core::site_design::tailwind::classes;
use sawit_core::site_design::tokens::{Tokens, derive};
use serde_json::{Value, json};

use super::{ExportDone, ExportShown};
use crate::browser::{BrowserView, OpResult};
use crate::rt;

pub const NOTHING: &str = "Nothing to extract on this page.";
/// What a dropped (superseded) inspect answers; the panel ignores it.
pub const STALE_INSPECT: &str = "A newer inspect replaced this one.";
const NO_EXPORT: &str = "No export has run in this browser tab.";
const PARTS: [&str; 6] = [
    "summary",
    "colors",
    "fonts",
    "assets",
    "tokens",
    "variables",
];
const FORMATS: [&str; 4] = ["json", "css", "tailwind", "html"];
/// Tool output cap.
const MAX_OUTPUT: usize = 60_000;
const CUT_NOTE: &str = "Cut at 60 KB; ask for one part.";
const STATUS_CUT_NOTE: &str =
    "Cut at 60 KB; the full lists are in the folder's README.md and assets.json.";
/// All cross-origin sheets and the manifest, together.
/// From the start of the scan, so `Design` answers within the 30 s control timeout.
const FETCH_BUDGET: Duration = Duration::from_secs(20);
/// A page answer (or a screenshot) that hasn't come by then won't: the job ends with an error,
/// so a lost callback can't keep a scan or an export running forever.
const PAGE_ANSWER_TIMEOUT: Duration = Duration::from_secs(25);
const FETCHES_AT_ONCE: usize = 8;

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

pub struct ExportJob {
    pub folder: PathBuf,
    pub done: usize,
    pub total: usize,
    pub result: Option<Result<ExportReport, String>>,
}

pub struct ExportReport {
    pub folder: PathBuf,
    /// Why `screenshot.png` was left out.
    pub screenshot_error: Option<String>,
    pub files: Vec<String>,
    /// Downloads that failed; skipped videos aren't among them.
    pub failed: Vec<AssetOutcome>,
    /// Videos left out because they weren't asked for.
    pub skipped_videos: usize,
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
    /// The last export, for the Design panel.
    pub(crate) fn export_job(&self) -> Option<&ExportJob> {
        self.design.export.as_ref()
    }

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

    /// The page's scan with fetched sheets, manifest icons and tokens; cached per page.
    pub(crate) fn scan_design(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Task<Result<Rc<Ready>, String>> {
        let url = self.current_url();
        if !extractable(&url) {
            self.design.state = ScanState::Failed(NOTHING.into());
            self.sync_design(cx);
            return Task::ready(Err(NOTHING.into()));
        }
        let key = page_key(&url).to_string();
        if self.design.key != key {
            self.design.reset();
            self.design.key = key;
        }
        let job = match (&self.design.state, &self.design.job) {
            (ScanState::Ready(ready), _) => return Task::ready(Ok(ready.clone())),
            (ScanState::Scanning, Some(job)) => job.clone(),
            _ => {
                let job = self.start_scan(url, cx).shared();
                self.design.job = Some(job.clone());
                job
            }
        };
        cx.spawn(async move |_, _| job.await)
    }

    fn start_scan(
        &mut self,
        url: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<Rc<Ready>, String>> {
        let generation = self.design.generation;
        let fetch_deadline = std::time::Instant::now() + FETCH_BUDGET;
        let scan = self.page_op(json!({ "op": "scan" }));
        self.design.state = ScanState::Scanning;
        self.sync_design(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = run_scan(&this, scan, url, fetch_deadline, cx).await;
            let _ = this.update(cx, |this, cx| {
                if this.design.generation == generation {
                    this.design.state = match &result {
                        Ok(ready) => ScanState::Ready(ready.clone()),
                        Err(e) => ScanState::Failed(e.clone().into()),
                    };
                    this.sync_design(cx);
                    cx.notify();
                }
            });
            result
        })
    }

    /// One element's styles; `html` adds its outerHTML with styles inlined.
    pub(in crate::browser) fn style_info(
        &self,
        target: Value,
        html: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<StyleInfo, String>> {
        let answer = self.page_op(style_op(target, html));
        cx.spawn(async move |_, cx| {
            parse(
                "style",
                serde_json::from_value(page_answer(answer, cx).await?),
            )
        })
    }

    /// Like [`Self::style_info`], latest only: an answer overtaken by a newer inspect errs
    /// with [`STALE_INSPECT`].
    pub(crate) fn inspect(
        &mut self,
        target: Value,
        html: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<StyleInfo, String>> {
        self.design.inspect_seq += 1;
        let seq = self.design.inspect_seq;
        let info = self.style_info(target, html, cx);
        cx.spawn(async move |this, cx| {
            let info = info.await;
            let latest = this
                .read_with(cx, |this, _| is_latest(seq, this.design.inspect_seq))
                .unwrap_or(false);
            if !latest {
                return Err(STALE_INSPECT.into());
            }
            info
        })
    }

    /// The pointer is on an element in inspect mode (`pin: false`), or a click picked it
    /// (`pin: true`): reads its styles for the Design panel.
    pub(in crate::browser) fn inspect_element(
        &mut self,
        selector: String,
        pin: bool,
        cx: &mut Context<Self>,
    ) {
        // Hovers still on their way after picking stopped are dropped.
        if self.panel != Some(crate::browser::Panel::Design)
            || self.pick != crate::browser::PickMode::Inspect
        {
            return;
        }
        let pinned = self.design_panel.read(cx).is_pinned();
        if !should_inspect(
            pinned,
            self.design.pins_pending,
            self.design.hover_req.as_deref(),
            &selector,
            pin,
        ) {
            return;
        }
        if pin {
            self.design.pins_pending += 1;
            self.design.hover_req = None;
        } else {
            self.design.hover_req = Some(selector.clone());
        }
        let info = self.inspect(json!({ "selector": selector }), false, cx);
        cx.spawn(async move |this, cx| {
            let info = info.await;
            let _ = this.update(cx, |this, cx| {
                if pin {
                    this.design.pins_pending = this.design.pins_pending.saturating_sub(1);
                }
                match info {
                    Ok(info) => this.design_panel.update(cx, |panel, cx| {
                        if pin {
                            panel.pin(info, cx);
                        } else {
                            panel.set_hovered(info, cx);
                        }
                    }),
                    // Overtaken: the newer ask answers. A failure lets the same element be
                    // asked again.
                    Err(e) => {
                        if e != STALE_INSPECT && !pin && this.design.hover_req.is_some() {
                            this.design.hover_req = None;
                        }
                    }
                }
            });
        })
        .detach();
    }

    /// Downloads `assets` into the export folder's `assets/` (other files there stay), then
    /// tells the Design panel.
    pub(crate) fn save_assets(&mut self, assets: Vec<AssetRef>, cx: &mut Context<Self>) {
        let url = self.current_url();
        let folder = match extractable(&url)
            .then_some(())
            .ok_or_else(|| NOTHING.to_string())
            .and_then(|()| self.export_folder(&export_host(&url)))
        {
            Ok(folder) => folder.join("assets"),
            Err(e) => return self.design_note(e, cx),
        };
        self.design_panel
            .update(cx, |p, cx| p.hold_note("Saving…", cx));
        let dir = folder.clone();
        let shown = self
            .project
            .as_deref()
            .and_then(|p| folder.strip_prefix(p).ok())
            .unwrap_or(&folder)
            .to_path_buf();
        let job = rt::spawn(cx, async move {
            // `assets` is what was chosen, videos included.
            let file_page = is_file_url(&url);
            let outcomes =
                download_all(assets, &dir, &url, file_page, true, MAX_TOTAL, |_, _| {}).await;
            let saved = outcomes.iter().filter(|o| o.error.is_none()).count();
            let first_error = outcomes.iter().find_map(|o| o.error.clone());
            (saved, outcomes.len(), first_error)
        });
        cx.spawn(async move |this, cx| {
            let note = match job.await {
                Ok((saved, total, error)) => save_note(saved, total, error, &shown),
                Err(_) => "The save was cancelled.".to_string(),
            };
            let _ = this.update(cx, |this, cx| this.design_note(note, cx));
        })
        .detach();
    }

    /// Hands the export's progress to the Design panel.
    fn sync_export(&mut self, cx: &mut Context<Self>) {
        let host = export_host(&self.current_url());
        let shown = self
            .export_job()
            .filter(|job| job.folder.file_name().is_some_and(|n| *n == *host))
            .map(|job| ExportShown {
                folder: job.folder.clone(),
                done: job.done,
                total: job.total,
                outcome: job.result.as_ref().map(|r| {
                    r.as_ref()
                        .map(|report| ExportDone {
                            files: report.files.len(),
                            failed: report.failed.len(),
                            skipped_videos: report.skipped_videos,
                            screenshot_error: report.screenshot_error.clone(),
                        })
                        .map_err(Clone::clone)
                }),
            });
        self.design_panel
            .update(cx, |p, cx| p.set_export(shown, cx));
    }

    /// Writes the export folder from the panel, or says why not.
    pub(in crate::browser) fn export_from_panel(
        &mut self,
        include_videos: bool,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = self.start_export(include_videos, cx) {
            self.design_note(e, cx);
        }
    }

    /// Downloads every asset of the scanned page into the export folder's `assets/`.
    pub(in crate::browser) fn save_all_assets(
        &mut self,
        include_videos: bool,
        cx: &mut Context<Self>,
    ) {
        let ScanState::Ready(ready) = &self.design.state else {
            return self.design_note(NOTHING.into(), cx);
        };
        let assets = super::assets::save_all_set(&ready.scan, include_videos);
        if assets.is_empty() {
            return self.design_note("No assets to save.".into(), cx);
        }
        self.save_assets(assets, cx);
    }

    /// Shows `text` in the Design panel's footer.
    pub(in crate::browser) fn design_note(&mut self, text: String, cx: &mut Context<Self>) {
        self.design_panel.update(cx, |p, cx| p.set_note(text, cx));
    }

    /// Starts the export in the background and returns its folder; while one runs, returns
    /// that one's folder.
    pub(crate) fn start_export(
        &mut self,
        include_videos: bool,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, String> {
        if let Some(job) = &self.design.export
            && job.result.is_none()
        {
            return Ok(job.folder.clone());
        }
        let url = self.current_url();
        if !extractable(&url) {
            return Err(NOTHING.into());
        }
        let folder = self.export_folder(&export_host(&url))?;
        let scan = self.scan_design(cx);
        self.design.export = Some(ExportJob {
            folder: folder.clone(),
            done: 0,
            total: 0,
            result: None,
        });
        self.sync_export(cx);
        let (progress, mut progress_rx) = unbounded::<(usize, usize)>();
        cx.spawn(async move |this, cx| {
            while let Some((done, total)) = progress_rx.next().await {
                let alive = this.update(cx, |this, cx| {
                    if let Some(job) = &mut this.design.export
                        && job.result.is_none()
                    {
                        (job.done, job.total) = (done, total);
                        cx.notify();
                    }
                    this.sync_export(cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        let job_folder = folder.clone();
        cx.spawn(async move |this, cx| {
            let result =
                run_export(&this, scan, job_folder, include_videos, url, progress, cx).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(job) = &mut this.design.export {
                    job.result = Some(result.map(|(report, total)| {
                        // Progress messages may still be on their way.
                        (job.done, job.total) = (total, total);
                        report
                    }));
                    cx.notify();
                }
                this.sync_export(cx);
            });
        })
        .detach();
        Ok(folder)
    }

    /// `.nyawit/design/<host>` (made, with `.nyawit/`'s ignore file) or
    /// `~/Downloads/sawit-design/<host>`; never a symbolic link, since an export clears it.
    fn export_folder(&self, host: &str) -> Result<PathBuf, String> {
        let folder = folder_for(self.project.as_deref(), host);
        if let (Some(project), Some(parent)) = (&self.project, folder.parent()) {
            sawit_core::nyawit::ensure(project, parent)
                .map_err(|e| format!("Couldn't make {}: {e}", parent.display()))?;
        }
        if std::fs::symlink_metadata(&folder).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!(
                "{} is a symbolic link; Sawit won't replace it.",
                folder.display()
            ));
        }
        Ok(folder)
    }

    /// `ExportDesign`: `{"export": "started"|"running", "folder"}`.
    pub(in crate::browser) fn export_op(
        &mut self,
        include_videos: bool,
        cx: &mut Context<Self>,
    ) -> OpResult {
        let running = self.design.export_running();
        let folder = self.start_export(include_videos, cx)?;
        Ok(json!({
            "export": if running { "running" } else { "started" },
            "folder": folder,
        }))
    }

    /// `ExportStatus`.
    pub(in crate::browser) fn export_status_op(&self) -> OpResult {
        self.design
            .export
            .as_ref()
            .map(export_status)
            .ok_or_else(|| NO_EXPORT.to_string())
    }

    /// `Design`: the scan, then one part of it.
    pub(in crate::browser) fn design_op(
        &mut self,
        part: Option<String>,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        let part = part.unwrap_or_else(|| "summary".into());
        if !PARTS.contains(&part.as_str()) {
            let _ = reply.send(design_part(&empty_ready(), &part));
            return;
        }
        let ready = self.scan_design(cx);
        cx.spawn(async move |_, _| {
            let _ = reply.send(ready.await.and_then(|r| design_part(&r, &part)));
        })
        .detach();
    }

    /// `Inspect`: an agent's ask, answered whatever the panel inspects meanwhile.
    pub(in crate::browser) fn inspect_op(
        &mut self,
        target: Value,
        format: Option<String>,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        let format = format.unwrap_or_else(|| "json".into());
        if !FORMATS.contains(&format.as_str()) {
            let _ = reply.send(Err(format!(
                "Unknown format “{format}”. Use json, css, tailwind or html."
            )));
            return;
        }
        let info = self.style_info(target, format == "html", cx);
        cx.spawn(async move |_, _| {
            let _ = reply.send(info.await.and_then(|i| inspect_answer(&i, &format)));
        })
        .detach();
    }
}

/// The footer note after a save of `total` files, `saved` of which arrived.
fn save_note(saved: usize, total: usize, first_error: Option<String>, dir: &Path) -> String {
    let failed = total - saved;
    if saved == 0 {
        return first_error.unwrap_or_else(|| "Nothing was saved.".to_string());
    }
    let mut note = format!(
        "Saved {saved} file{} to {}",
        if saved == 1 { "" } else { "s" },
        dir.display()
    );
    if failed > 0 {
        note.push_str(&format!(
            ". {failed} of {total} assets couldn't be downloaded"
        ));
    }
    note
}

fn empty_ready() -> Ready {
    Ready {
        scan: Scan::default(),
        tokens: Tokens::default(),
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

async fn run_scan(
    this: &WeakEntity<BrowserView>,
    scan: oneshot::Receiver<OpResult>,
    url: String,
    fetch_deadline: std::time::Instant,
    cx: &mut AsyncApp,
) -> Result<Rc<Ready>, String> {
    let mut scan: Scan = parse("scan", serde_json::from_value(page_answer(scan, cx).await?))?;
    if scan.colors.is_empty() && scan.texts.is_empty() {
        return Err(NOTHING.into());
    }
    let referer = if scan.url.is_empty() {
        url.clone()
    } else {
        scan.url.clone()
    };
    let extras = fetch_extras(
        scan.blocked_sheets.clone(),
        scan.manifest.clone(),
        referer,
        url,
        fetch_deadline,
    );
    let extras = this
        .update(cx, |_, cx| rt::spawn(cx, extras))
        .map_err(gone)?
        .await
        .map_err(cancelled)?;
    if !extras.sheets.is_empty() {
        // `rules` replaces the page's fetched sheets: all of them in one call.
        let sheets: Vec<Value> = extras
            .sheets
            .iter()
            .map(|(href, text)| json!({ "href": href, "text": text }))
            .collect();
        let rules = this
            .update(cx, |this, _| {
                this.page_op(json!({ "op": "rules", "sheets": sheets }))
            })
            .map_err(gone)?;
        let rules: SheetRules = parse(
            "rules",
            serde_json::from_value(page_answer(rules, cx).await?),
        )?;
        scan.merge_rules(rules);
    }
    add_icons(&mut scan.assets, extras.icons);
    let date = sawit_core::date::local_date();
    let (scan, tokens) = this
        .update(cx, |_, cx| {
            rt::spawn_blocking(cx, move || {
                let tokens = derive(&scan, &date);
                (scan, tokens)
            })
        })
        .map_err(gone)?
        .await
        .map_err(cancelled)?;
    Ok(Rc::new(Ready { scan, tokens }))
}

struct Extras {
    /// (href, text) in the page's order.
    sheets: Vec<(String, String)>,
    icons: Vec<AssetRef>,
}

fn is_http(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

/// Fetches the sheets the page couldn't read and its manifest's icons, within
/// `deadline` (the scan's start + [`FETCH_BUDGET`]); what fails or is late is left out.
/// A sheet's text goes back to the page, whose scripts can read it: only CSS, and from
/// this Mac or its network only for a page of that host (`page`, the tab's URL as Sawit
/// knows it), so a page can't read the dev server or the LAN through Sawit.
async fn fetch_extras(
    hrefs: Vec<String>,
    manifest: Option<String>,
    referer: String,
    page: String,
    deadline: std::time::Instant,
) -> Extras {
    let deadline = tokio::time::Instant::from_std(deadline);
    let (referer, page) = (referer.as_str(), page.as_str());
    let sheets = async {
        let wanted = hrefs.into_iter().filter(|h| {
            let ok = may_fetch_for(h, page);
            if is_http(h) && !ok {
                eprintln!("sawit: left out the stylesheet {h}: it is on this Mac or its network");
            }
            is_http(h) && ok
        });
        let mut stream = futures::stream::iter(wanted.enumerate().map(|(i, href)| async move {
            let text = fetch_text(&href, referer).await;
            (i, href, text)
        }))
        .buffer_unordered(FETCHES_AT_ONCE);
        let mut got = Vec::new();
        while let Ok(Some((i, href, text))) = tokio::time::timeout_at(deadline, stream.next()).await
        {
            match text.map(|f| sheet_for_page(&href, f, page)) {
                Ok(Ok(text)) => got.push((i, href, text)),
                Ok(Err(why)) => eprintln!("sawit: left out the stylesheet {href}: {why}"),
                Err(e) => eprintln!("sawit: couldn't fetch the stylesheet {href}: {e}"),
            }
        }
        got.sort_by_key(|(i, _, _)| *i);
        got.into_iter().map(|(_, h, t)| (h, t)).collect::<Vec<_>>()
    };
    let icons = async {
        let Some(manifest) = manifest.filter(|m| is_http(m)) else {
            return Vec::new();
        };
        match tokio::time::timeout_at(deadline, fetch_text(&manifest, referer)).await {
            Ok(Ok(fetched)) => manifest_icons(&fetched.text, &manifest),
            Ok(Err(e)) => {
                eprintln!("sawit: couldn't fetch the manifest {manifest}: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    };
    let (sheets, icons) = futures::join!(sheets, icons);
    Extras { sheets, icons }
}

/// Adds manifest icons the page didn't already list.
fn add_icons(assets: &mut Vec<AssetRef>, icons: Vec<AssetRef>) {
    for icon in icons {
        if !assets.iter().any(|a| a.url == icon.url) {
            assets.push(icon);
        }
    }
}

/// The manifest's `icons[].src`, resolved against the manifest's URL.
fn manifest_icons(text: &str, manifest_url: &str) -> Vec<AssetRef> {
    let Ok(manifest) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(icons) = manifest["icons"].as_array() else {
        return Vec::new();
    };
    icons
        .iter()
        .filter_map(|icon| {
            let url = resolve_url(manifest_url, icon["src"].as_str()?)?;
            let size = icon["sizes"]
                .as_str()
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| {
                    s.to_ascii_lowercase()
                        .split_once('x')
                        .map(|(w, h)| (w.parse().ok(), h.parse().ok()))
                });
            let (width, height) = match size {
                Some((Some(w), Some(h))) => (Some(w), Some(h)),
                _ => (None, None),
            };
            let label = match (width, height) {
                (Some(w), Some(h)) => format!("app icon {w}x{h}"),
                _ => "app icon".to_string(),
            };
            Some(AssetRef {
                kind: AssetKind::Icon,
                url: Some(url),
                svg: None,
                label,
                width,
                height,
                used_on: vec![],
                logo: false,
            })
        })
        .collect()
}

/// `href` against `base` (an absolute http(s) URL); `None` for an empty `href` or a base
/// without an authority.
fn resolve_url(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    let has_scheme = href.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if has_scheme {
        return Some(href.to_string());
    }
    let (scheme, rest) = base.split_once("://")?;
    if let Some(rest) = href.strip_prefix("//") {
        return Some(format!("{scheme}://{rest}"));
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let path = tail.split(['?', '#']).next().unwrap_or("");
    let origin = format!("{scheme}://{authority}");
    if href.starts_with('/') {
        return Some(format!("{origin}{href}"));
    }
    if href.starts_with(['?', '#']) {
        let path = if path.is_empty() { "/" } else { path };
        return Some(format!("{origin}{path}{href}"));
    }
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    Some(format!("{origin}{dir}{href}"))
}

async fn run_export(
    this: &WeakEntity<BrowserView>,
    scan: Task<Result<Rc<Ready>, String>>,
    folder: PathBuf,
    include_videos: bool,
    url: String,
    progress: UnboundedSender<(usize, usize)>,
    cx: &mut AsyncApp,
) -> Result<(ExportReport, usize), String> {
    let ready = scan.await?;
    // Local files are copied for a local page by the tab's own URL, not the page's word.
    let file_page = is_file_url(&url);
    let referer = if ready.scan.url.is_empty() {
        url
    } else {
        ready.scan.url.clone()
    };

    // The screenshot before the downloads (which can take minutes), and only of the
    // page that was scanned.
    let current = this
        .read_with(cx, |this, _| this.current_url())
        .map_err(gone)?;
    let (png, screenshot_error) = if page_key(&current) != page_key(&referer) {
        (
            None,
            Some(format!(
                "The tab moved to {current} after the scan; screenshot.png was left out."
            )),
        )
    } else {
        let (tx, rx) = oneshot::channel();
        this.update(cx, |this, cx| this.shoot(None, tx, cx))
            .map_err(gone)?;
        match page_answer(rx, cx).await {
            Ok(shot) => match shot["png_base64"]
                .as_str()
                .and_then(sawit_core::browser::base64_decode)
            {
                Some(png) => (Some(png), None),
                None => (None, Some("The capture has no readable image.".into())),
            },
            Err(e) => (None, Some(e)),
        }
    };
    if let Some(e) = &screenshot_error {
        eprintln!("sawit: no screenshot for the design export: {e}");
    }

    let mut items = ready.scan.assets.clone();
    items.extend(font_assets(&ready.scan.font_faces));
    let total = items.len();
    let assets_dir = folder.join("assets");
    let download = async move {
        // Only `assets/` is cleared here; `write_folder` replaces the rest.
        match tokio::fs::remove_dir_all(&assets_dir).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Couldn't clear {}: {e}", assets_dir.display())),
        }
        let report = move |done, total| {
            let _ = progress.unbounded_send((done, total));
        };
        Ok(download_all(
            items,
            &assets_dir,
            &referer,
            file_page,
            include_videos,
            MAX_TOTAL,
            report,
        )
        .await)
    };
    let outcomes = this
        .update(cx, |_, cx| rt::spawn(cx, download))
        .map_err(gone)?
        .await
        .map_err(|_| "The export was cancelled.".to_string())??;

    let (scan, tokens) = (ready.scan.clone(), ready.tokens.clone());
    let dir = folder.clone();
    let written = this
        .update(cx, |_, cx| {
            rt::spawn_blocking(cx, move || {
                write_folder(&dir, &tokens, &scan, png.as_deref(), &outcomes)
                    .map(|files| (files, outcomes))
                    .map_err(|e| format!("Couldn't write {}: {e}", dir.display()))
            })
        })
        .map_err(gone)?
        .await
        .map_err(|_| "The export was cancelled.".to_string())?;
    let (files, outcomes) = written?;
    let skipped_videos = outcomes.iter().filter(|o| is_skipped_video(o)).count();
    let report = ExportReport {
        folder,
        screenshot_error,
        files,
        failed: outcomes
            .into_iter()
            .filter(|o| o.error.is_some() && !is_skipped_video(o))
            .collect(),
        skipped_videos,
    };
    Ok((report, total))
}

fn is_file_url(url: &str) -> bool {
    url.trim().to_ascii_lowercase().starts_with("file://")
}

/// Only web pages and local files have styles to read.
fn extractable(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("file://")
}

/// Whether a hover (`pin: false`) or a pick (`pin: true`) of `selector` asks the page. A pick
/// always does. A hover waits while an element is pinned or a pick is pending (it would
/// overtake the pick's answer), and repeats nothing: the last element asked about is the one
/// the answer in flight, or on show, is for.
fn should_inspect(
    pinned: bool,
    pins_pending: u32,
    last_hover: Option<&str>,
    selector: &str,
    pin: bool,
) -> bool {
    pin || (!pinned && pins_pending == 0 && last_hover != Some(selector))
}

fn is_latest(seq: u64, current: u64) -> bool {
    seq == current
}

fn style_op(target: Value, html: bool) -> Value {
    let mut op = match target {
        Value::Object(map) => Value::Object(map),
        _ => json!({}),
    };
    op["op"] = "style".into();
    op["html"] = html.into();
    op
}

/// The URL's host without port, as a folder name: `[a-z0-9.-]`, else `page`.
fn export_host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    let host: String = host
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // No `.`/`..` and no hidden folder.
    let host = host.trim_matches(['-', '.']);
    if host.is_empty() {
        "page".into()
    } else {
        host.into()
    }
}

/// Where an export of `host` goes (see [`BrowserView::export_folder`]).
fn folder_for(project: Option<&Path>, host: &str) -> PathBuf {
    match project {
        Some(project) => sawit_core::nyawit::dir(project).join("design").join(host),
        None => {
            let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
            home.join("Downloads").join("sawit-design").join(host)
        }
    }
}

/// One part of the design, at most [`MAX_OUTPUT`] bytes.
fn design_part(ready: &Ready, part: &str) -> Result<Value, String> {
    let (s, t) = (&ready.scan, &ready.tokens);
    let mut v = match part {
        "summary" => json!({
            "url": s.url,
            "title": s.title,
            "truncated": s.truncated,
            "counts": {
                "colors": s.colors.len(),
                "text_styles": s.texts.len(),
                "font_faces": s.font_faces.len(),
                "assets": s.assets.len(),
                "variables": s.variables.len(),
                "components": s.components.len(),
            },
            "tokens": t,
        }),
        "colors" => json!({ "colors": s.colors }),
        "fonts" => json!({
            "families": t.families,
            "font_faces": s.font_faces,
            "text_styles": s.texts,
        }),
        "assets" => json!({ "assets": s.assets }),
        "tokens" => json!({ "tokens": t }),
        "variables" => json!({ "variables": s.variables }),
        _ => {
            return Err(format!(
                "Unknown part “{part}”. Use summary, colors, fonts, assets, tokens or variables."
            ));
        }
    };
    cap(&mut v, CUT_NOTE);
    Ok(v)
}

fn size_of(v: &Value) -> usize {
    serde_json::to_vec(v).map_or(0, |b| b.len())
}

/// The JSON pointer and serialized size of the biggest non-empty array in `v`.
fn biggest_array(v: &Value, path: &str, best: &mut Option<(String, usize)>) {
    match v {
        Value::Array(items) => {
            if !items.is_empty() {
                let size = size_of(v);
                if best.as_ref().is_none_or(|(_, s)| size > *s) {
                    *best = Some((path.to_string(), size));
                }
            }
            for (i, item) in items.iter().enumerate() {
                biggest_array(item, &format!("{path}/{i}"), best);
            }
        }
        Value::Object(map) => {
            for (k, item) in map {
                let k = k.replace('~', "~0").replace('/', "~1");
                biggest_array(item, &format!("{path}/{k}"), best);
            }
        }
        _ => {}
    }
}

/// Cuts the biggest lists of the object `v` until it fits [`MAX_OUTPUT`] with `note` added.
fn cap(v: &mut Value, note: &str) {
    if size_of(v) <= MAX_OUTPUT {
        return;
    }
    let budget = MAX_OUTPUT - note.len() - 16;
    loop {
        let size = size_of(v);
        if size <= budget {
            break;
        }
        let mut best = None;
        biggest_array(v, "", &mut best);
        let Some((path, array_size)) = best else {
            // Nothing left to cut (one huge value): say so instead.
            *v = json!({});
            break;
        };
        let Some(Value::Array(items)) = v.pointer_mut(&path) else {
            break;
        };
        let per_item = (array_size / items.len()).max(1);
        let remove = (size - budget).div_ceil(per_item);
        items.truncate(items.len().saturating_sub(remove.max(1)));
    }
    v["note"] = note.into();
}

fn inspect_answer(info: &StyleInfo, format: &str) -> OpResult {
    match format {
        "json" => {
            let mut v = serde_json::to_value(info).map_err(|e| e.to_string())?;
            if let Some(map) = v.as_object_mut() {
                map.remove("html");
            }
            v["contrast"] = contrast(info).map_or(Value::Null, |c| {
                json!({
                    "ratio": (c.ratio * 100.).round() / 100.,
                    "large": c.large,
                    "aa": c.aa,
                    "aaa": c.aaa,
                })
            });
            Ok(v)
        }
        "css" => Ok(json!({ "selector": info.selector, "css": css_rule(info) })),
        "tailwind" => Ok(json!({ "selector": info.selector, "tailwind": classes(info).join(" ") })),
        "html" => Ok(json!({ "selector": info.selector, "html": info.html })),
        _ => Err(format!(
            "Unknown format “{format}”. Use json, css, tailwind or html."
        )),
    }
}

fn export_status(job: &ExportJob) -> Value {
    let (files, failed, skipped_videos, error) = match &job.result {
        Some(Ok(report)) => (
            report.files.as_slice(),
            report.failed.as_slice(),
            report.skipped_videos,
            None,
        ),
        Some(Err(e)) => (&[][..], &[][..], 0, Some(e.as_str())),
        None => (&[][..], &[][..], 0, None),
    };
    let screenshot_error = match &job.result {
        Some(Ok(report)) => report.screenshot_error.as_deref(),
        _ => None,
    };
    let folder = match &job.result {
        Some(Ok(report)) => &report.folder,
        _ => &job.folder,
    };
    let mut v = json!({
        "done": job.result.is_some(),
        "progress": [job.done, job.total],
        "folder": folder,
        "files": files,
        "failed": failed,
        "skipped_videos": skipped_videos,
        "screenshot_error": screenshot_error,
        "error": error,
    });
    cap(&mut v, STATUS_CUT_NOTE);
    v
}

#[cfg(test)]
mod tests {
    #[test]
    fn save_note_reports_failures() {
        let d = std::path::Path::new("a/assets");
        assert_eq!(super::save_note(3, 3, None, d), "Saved 3 files to a/assets");
        assert_eq!(
            super::save_note(28, 40, Some("x".into()), d),
            "Saved 28 files to a/assets. 12 of 40 assets couldn't be downloaded"
        );
        assert_eq!(super::save_note(0, 2, Some("403".into()), d), "403");
    }

    use super::*;
    use sawit_core::site_design::scan::{ColorRole, ColorUse};

    fn ready_with_colors(n: usize) -> Ready {
        let colors = (0..n)
            .map(|i| ColorUse {
                value: format!("rgb({}, {}, {})", i % 256, (i / 256) % 256, 7),
                role: ColorRole::Background,
                count: 1,
                on_button: 0,
                on_link: 0,
            })
            .collect();
        Ready {
            scan: Scan {
                url: "https://a.dev/".into(),
                colors,
                ..Default::default()
            },
            tokens: Tokens::default(),
        }
    }

    #[test]
    fn design_part_caps_output() {
        let ready = ready_with_colors(5_000);
        let v = design_part(&ready, "colors").unwrap();
        let text = serde_json::to_string(&v).unwrap();
        assert!(text.len() <= 60_000, "{} bytes", text.len());
        assert_eq!(v["note"], "Cut at 60 KB; ask for one part.");
        assert!(!v["colors"].as_array().unwrap().is_empty());
        // Small parts are whole and carry no note.
        let small = design_part(&ready_with_colors(3), "colors").unwrap();
        assert!(small.get("note").is_none());
        assert_eq!(small["colors"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn unknown_part_is_explained() {
        let ready = ready_with_colors(1);
        assert_eq!(
            design_part(&ready, "x").unwrap_err(),
            "Unknown part “x”. Use summary, colors, fonts, assets, tokens or variables."
        );
        for part in [
            "summary",
            "colors",
            "fonts",
            "assets",
            "tokens",
            "variables",
        ] {
            assert!(design_part(&ready, part).is_ok(), "{part}");
        }
    }

    #[test]
    fn file_pages_by_their_url() {
        assert!(is_file_url("file:///Users/a/index.html"));
        assert!(is_file_url(" FILE:///x"));
        assert!(!is_file_url("https://a.dev/file://x"));
    }

    #[test]
    fn extractable_pages() {
        assert!(extractable("https://a.dev"));
        assert!(extractable("http://localhost:5173/x"));
        assert!(extractable("file:///x.html"));
        assert!(!extractable("about:blank"));
        assert!(!extractable("data:text/html,x"));
    }

    #[test]
    fn hover_a_b_a_asks_again() {
        // A on show, B asked, back to A: A must be asked, B's answer is then stale.
        assert!(should_inspect(false, 0, None, "a", false));
        assert!(should_inspect(false, 0, Some("a"), "b", false));
        assert!(should_inspect(false, 0, Some("b"), "a", false));
        // The same element twice in a row isn't asked twice.
        assert!(!should_inspect(false, 0, Some("a"), "a", false));
    }

    #[test]
    fn hover_waits_for_a_pending_pin_and_a_pinned_element() {
        assert!(should_inspect(false, 0, Some("a"), "a", true));
        assert!(!should_inspect(false, 1, None, "b", false));
        assert!(!should_inspect(true, 0, None, "b", false));
        // A second pick while one is pending still goes through.
        assert!(should_inspect(false, 1, None, "b", true));
    }

    #[test]
    fn copy_during_hover_does_not_use_the_latest_only_gate() {
        // Copies call `style_info`, which never touches the sequence.
        let mut seq = 0u64;
        seq += 1; // hover asks
        let hover = seq;
        // (copy runs here without bumping)
        assert!(is_latest(hover, seq));
    }

    #[test]
    fn inspect_drops_stale() {
        assert!(is_latest(3, 3));
        assert!(!is_latest(2, 3));
    }

    #[test]
    fn export_folder_without_project() {
        let host = export_host("https://www.apple.com:443/mac/");
        assert_eq!(host, "www.apple.com");
        let dir = folder_for(None, &host);
        assert!(
            dir.ends_with("Downloads/sawit-design/www.apple.com"),
            "{}",
            dir.display()
        );
        let project = std::path::Path::new("/p");
        assert_eq!(
            folder_for(Some(project), "a.dev"),
            std::path::Path::new("/p/.nyawit/design/a.dev")
        );
    }

    #[test]
    fn export_hosts_are_safe_folder_names() {
        assert_eq!(export_host("http://LocalHost:5173/"), "localhost");
        assert_eq!(export_host("https://user:pw@x.dev/a"), "x.dev");
        assert_eq!(export_host("file:///Users/me/a.html"), "page");
        assert_eq!(export_host("http://[::1]:8080/"), "1");
        assert_eq!(export_host("http://../"), "page");
        assert_eq!(export_host("http://a_b.dev/"), "a-b.dev");
    }

    #[test]
    fn manifest_icons_resolve_against_the_manifest() {
        let manifest = r#"{"icons":[
            {"src":"/i/192.png","sizes":"192x192"},
            {"src":"icon-512.png","sizes":"512x512 1024x1024"},
            {"src":"https://cdn.x/abs.png"},
            {"src":""}
        ]}"#;
        let icons = manifest_icons(manifest, "https://x.dev/app/site.webmanifest");
        let urls: Vec<_> = icons.iter().map(|a| a.url.clone().unwrap()).collect();
        assert_eq!(
            urls,
            [
                "https://x.dev/i/192.png",
                "https://x.dev/app/icon-512.png",
                "https://cdn.x/abs.png"
            ]
        );
        assert!(icons.iter().all(|a| a.kind == AssetKind::Icon));
        assert_eq!((icons[0].width, icons[0].height), (Some(192), Some(192)));
        assert_eq!(icons[2].width, None);
        assert!(manifest_icons("not json", "https://x.dev/m.json").is_empty());
    }

    fn style() -> StyleInfo {
        serde_json::from_str(include_str!(
            "../../../../core/src/site_design/fixtures/style_button.json"
        ))
        .unwrap()
    }

    #[test]
    fn inspect_formats() {
        let info = style();
        let v = inspect_answer(&info, "json").unwrap();
        assert_eq!(v["selector"], info.selector.as_str());
        assert!(v.get("html").is_none());
        assert!(v["contrast"]["ratio"].as_f64().unwrap() > 1.0);
        let css = inspect_answer(&info, "css").unwrap();
        assert_eq!(css["css"], css_rule(&info).as_str());
        let tw = inspect_answer(&info, "tailwind").unwrap();
        assert_eq!(tw["tailwind"], classes(&info).join(" ").as_str());
        assert!(inspect_answer(&info, "html").unwrap().get("html").is_some());
        assert_eq!(
            inspect_answer(&info, "yaml").unwrap_err(),
            "Unknown format “yaml”. Use json, css, tailwind or html."
        );
    }

    #[test]
    fn style_ops_carry_the_target() {
        assert_eq!(
            style_op(json!({"ref": "e4"}), true),
            json!({"op": "style", "ref": "e4", "html": true})
        );
        assert_eq!(
            style_op(Value::Null, false),
            json!({"op": "style", "html": false})
        );
    }

    #[test]
    fn export_status_reports_the_job() {
        let mut job = ExportJob {
            folder: "/p/.nyawit/design/a.dev".into(),
            done: 3,
            total: 9,
            result: None,
        };
        let v = export_status(&job);
        assert_eq!(v["done"], false);
        assert_eq!(v["progress"], json!([3, 9]));
        assert_eq!(v["folder"], "/p/.nyawit/design/a.dev");
        assert_eq!(v["error"], Value::Null);
        job.result = Some(Err("disk full".into()));
        let v = export_status(&job);
        assert_eq!(
            (v["done"].clone(), v["error"].clone()),
            (json!(true), json!("disk full"))
        );
        job.result = Some(Ok(ExportReport {
            folder: job.folder.clone(),
            screenshot_error: Some("The tab moved.".into()),
            files: (0..5_000)
                .map(|i| format!("assets/images/image-{i}.png"))
                .collect(),
            failed: vec![],
            skipped_videos: 2,
        }));
        let v = export_status(&job);
        assert_eq!(v["skipped_videos"], 2);
        assert!(serde_json::to_string(&v).unwrap().len() <= 60_000);
        assert!(v["note"].as_str().unwrap().starts_with("Cut at 60 KB"));
        assert_eq!(v["folder"], "/p/.nyawit/design/a.dev");
        assert_eq!(v["screenshot_error"], "The tab moved.");
    }

    #[test]
    fn page_answers_time_out() {
        let (_keep, pending) = oneshot::channel::<OpResult>();
        let late =
            futures::executor::block_on(answer_or_timeout(pending, futures::future::ready(())));
        assert_eq!(late.unwrap_err(), "The page didn't answer within 25 s.");
        let (tx, rx) = oneshot::channel::<OpResult>();
        tx.send(Ok(json!({"n": 1}))).unwrap();
        let on_time =
            futures::executor::block_on(answer_or_timeout(rx, futures::future::pending()));
        assert_eq!(on_time.unwrap(), json!({"n": 1}));
    }

    #[test]
    fn icons_already_listed_are_kept_once() {
        let icon = |u: &str| AssetRef {
            kind: AssetKind::Icon,
            url: Some(u.into()),
            svg: None,
            label: "icon".into(),
            width: None,
            height: None,
            used_on: vec![],
            logo: false,
        };
        let mut assets = vec![icon("https://x.dev/a.png")];
        add_icons(
            &mut assets,
            vec![icon("https://x.dev/a.png"), icon("https://x.dev/b.png")],
        );
        assert_eq!(assets.len(), 2);
    }

    #[test]
    fn urls_resolve() {
        let base = "https://x.dev/a/b.json?v=1";
        assert_eq!(resolve_url(base, "c.png").unwrap(), "https://x.dev/a/c.png");
        assert_eq!(resolve_url(base, "/c.png").unwrap(), "https://x.dev/c.png");
        assert_eq!(
            resolve_url(base, "//cdn.x/c.png").unwrap(),
            "https://cdn.x/c.png"
        );
        assert_eq!(
            resolve_url(base, "data:image/png;base64,AA").unwrap(),
            "data:image/png;base64,AA"
        );
        assert_eq!(resolve_url(base, ""), None);
    }
}
