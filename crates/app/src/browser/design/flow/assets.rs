//! Assets saved from the panel into the export folder's `assets/`, and the Assets tab's
//! previews, fetched by the same rules.

use std::path::Path;

use futures::StreamExt as _;
use gpui_kit::Context;
use sawit_core::site_design::assets::{MAX_TOTAL, download_all};
use sawit_core::site_design::scan::AssetRef;

use super::export::export_host;
use super::{NOTHING, ScanState, extractable};
use crate::browser::BrowserView;
use crate::rt;

/// The largest file the Assets tab downloads for one preview.
const THUMB_MAX: u64 = 5 * 1024 * 1024;

impl BrowserView {
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
            let outcomes = download_all(assets, &dir, &url, &url, true, MAX_TOTAL, |_, _| {}).await;
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

    /// Fetches previews of `assets` for the Assets tab, by the same rules as a save (a page on
    /// the web gets nothing from this Mac or its network), into a folder of the app's cache that
    /// is gone once they are read.
    pub(in crate::browser) fn fetch_thumbnails(
        &mut self,
        assets: Vec<AssetRef>,
        cx: &mut Context<Self>,
    ) {
        let url = self.current_url();
        let generation = self.design.generation;
        let keys: Vec<String> = assets.iter().filter_map(|a| a.url.clone()).collect();
        static BATCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let batch = BATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = sawit_core::instances::AppPaths::default_macos()
            .cache_dir()
            .join("design-previews")
            .join(format!("{}-{batch}", std::process::id()));
        let job = rt::spawn(cx, async move {
            // One download each, each with its own small budget: a large file doesn't hold
            // what the others need while they run at once.
            let got: Vec<(String, Option<Vec<u8>>)> =
                futures::stream::iter(assets.into_iter().enumerate().map(|(i, a)| {
                    let dir = dir.join(i.to_string());
                    let url = url.clone();
                    async move {
                        let key = a.url.clone()?;
                        let out =
                            download_all(vec![a], &dir, &url, &url, false, THUMB_MAX, |_, _| {})
                                .await;
                        let bytes = out
                            .first()
                            .and_then(|o| o.file.as_ref())
                            .and_then(|f| std::fs::read(dir.join(f)).ok());
                        Some((key, bytes))
                    }
                }))
                .buffer_unordered(6)
                .filter_map(|got| async move { got })
                .collect()
                .await;
            let _ = std::fs::remove_dir_all(&dir);
            got
        });
        cx.spawn(async move |this, cx| {
            let got = job.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                // A preview of a page left meanwhile lands nowhere.
                if this.design.generation != generation {
                    return;
                }
                // What didn't come shows its kind's icon, not a tile loading for ever.
                let mut all: Vec<(String, Option<Vec<u8>>)> = keys
                    .into_iter()
                    .filter(|k| !got.iter().any(|(g, _)| g == k))
                    .map(|k| (k, None))
                    .collect();
                all.extend(got);
                this.design_panel
                    .update(cx, |p, cx| p.set_thumbnails(all, cx));
            });
        })
        .detach();
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
        let assets = super::super::assets::save_all_set(&ready.scan, include_videos);
        if assets.is_empty() {
            return self.design_note("No assets to save.".into(), cx);
        }
        self.save_assets(assets, cx);
    }
}

/// The footer note after a save of `total` files, `saved` of which arrived.
pub(super) fn save_note(
    saved: usize,
    total: usize,
    first_error: Option<String>,
    dir: &Path,
) -> String {
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
