//! The export: the design folder (`DESIGN.md`, tokens, themes, components, the assets and a
//! screenshot), written in the background with its progress, and never through a link.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::channel::oneshot;
use gpui_kit::{AsyncApp, Context, Task, WeakEntity};
use sawit_core::annotations::page_key;
use sawit_core::site_design::assets::{MAX_TOTAL, download_all, font_assets, is_skipped_video};
use sawit_core::site_design::export::write_folder;
use sawit_core::site_design::scan::AssetOutcome;

use super::super::{ExportDone, ExportShown};
use super::{NOTHING, Ready, extractable, gone, page_answer};
use crate::browser::BrowserView;
use crate::rt;

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

impl BrowserView {
    /// The last export, for the Design panel.
    pub(crate) fn export_job(&self) -> Option<&ExportJob> {
        self.design.export.as_ref()
    }

    /// Hands the export's progress to the Design panel.
    pub(super) fn sync_export(&mut self, cx: &mut Context<Self>) {
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

    /// The export folder of `host` (see [`export_folder_for`]).
    pub(super) fn export_folder(&self, host: &str) -> Result<PathBuf, String> {
        export_folder_for(self.project.as_deref(), host)
    }
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
    // What may be downloaded follows the tab's own URL, not the page's word.
    let page = url.clone();
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
            &page,
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

/// The URL's host as a folder name, `[a-z0-9.-]` (else `page`), with its port when it
/// isn't the scheme's own: two dev servers on one host are two apps (`localhost-5173`).
pub(super) fn export_host(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (host, after) = v6.split_once(']').unwrap_or((v6, ""));
        (host, after.strip_prefix(':'))
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    let default = match scheme.to_ascii_lowercase().as_str() {
        "http" => "80",
        "https" => "443",
        _ => "",
    };
    let port =
        port.filter(|p| !p.is_empty() && *p != default && p.bytes().all(|b| b.is_ascii_digit()));
    let host = match port {
        Some(port) if !host.trim_matches(['-', '.', ':']).is_empty() => format!("{host}-{port}"),
        _ => host.to_string(),
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

/// `.nyawit/design/<host>` (made, with `.nyawit/`'s ignore file) or
/// `~/Downloads/sawit-design/<host>`. An export clears that folder and writes into its
/// `assets/`, so none of them is reached through a symbolic link: not `.nyawit`, `design`,
/// the host's folder nor `assets/` (a repository can commit one, pointing anywhere).
pub(super) fn export_folder_for(project: Option<&Path>, host: &str) -> Result<PathBuf, String> {
    let folder = folder_for(project, host);
    let assets = folder.join("assets");
    // From the project down; without one, the host's folder and its `assets/`.
    let base = project.or(folder.parent()).unwrap_or(&folder);
    let mut at = base.to_path_buf();
    for part in assets.strip_prefix(base).unwrap_or(&assets).components() {
        at.push(part);
        if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!(
                "{} is a symbolic link; Sawit won't write through it.",
                at.display()
            ));
        }
    }
    if let (Some(project), Some(parent)) = (project, folder.parent()) {
        sawit_core::nyawit::ensure(project, parent)
            .map_err(|e| format!("Couldn't make {}: {e}", parent.display()))?;
    }
    Ok(folder)
}

/// Why an agent's export into `folder` must say `replace`: the folder holds something, what an
/// earlier export left or not, and a new export replaces all of it, a `DESIGN.md` edited by hand
/// included ([`write_folder`] and `run_export`'s clearing of `assets/`). `None` for a folder that
/// is empty or not there yet.
pub(super) fn earlier_export(folder: &Path) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    if names.is_empty() {
        return None;
    }
    names.sort();
    let mut held = names.iter().take(6).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 6 {
        held.push_str(&format!(" and {} more", names.len() - 6));
    }
    Some(format!(
        "{} already holds an export ({held}). A new export replaces everything in it, a DESIGN.md \
         edited by hand included. Nothing was written: export again with replace: true to \
         replace it.",
        folder.display()
    ))
}

/// Where an export of `host` goes (see [`export_folder_for`]).
pub(super) fn folder_for(project: Option<&Path>, host: &str) -> PathBuf {
    match project {
        Some(project) => sawit_core::nyawit::dir(project).join("design").join(host),
        None => {
            let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
            home.join("Downloads").join("sawit-design").join(host)
        }
    }
}
