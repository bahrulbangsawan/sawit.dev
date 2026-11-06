//! The tabs kept between runs ([`sawit_core::browser::SavedTabs`]): written a second after they
//! change (a tab opened, closed, moved or shown, a page loaded), off the main thread.

use std::path::Path;
use std::time::Duration;

use gpui_kit::*;
use sawit_core::browser::{SavedTab, SavedTabs, base64_encode, load_tabs, save_tabs, tabs_file};
use sawit_core::instances::AppPaths;

use super::BrowserTabs;

/// How long after a change the tabs are written: one write for a burst of changes.
const SAVE_AFTER: Duration = Duration::from_secs(1);

fn file(project: Option<&Path>) -> std::path::PathBuf {
    tabs_file(project, &AppPaths::default_macos().root)
}

/// The tabs the browser of `project` had last time.
pub(super) fn load(project: Option<&Path>) -> SavedTabs {
    load_tabs(&file(project))
}

/// The browser was closed: its tabs aren't brought back.
pub(super) fn forget(project: Option<&Path>) {
    let _ = std::fs::remove_file(file(project));
}

impl BrowserTabs {
    /// Writes the tabs [`SAVE_AFTER`] from now, once for any changes meanwhile.
    pub(super) fn save_soon(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.saving = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            let Ok((project, saved)) = this.update(cx, |this, cx| {
                this.saving = false;
                (this.project.clone(), this.saved(cx))
            }) else {
                return;
            };
            if saved.tabs.is_empty() {
                return;
            }
            let written = cx.background_executor().spawn(async move {
                save_tabs(&file(project.as_deref()), project.as_deref(), &saved)
            });
            if let Err(e) = written.await {
                eprintln!("sawit: the browser's tabs weren't saved: {e}");
            }
        })
        .detach();
    }

    fn saved(&self, cx: &App) -> SavedTabs {
        SavedTabs {
            active: self.active,
            tabs: self
                .tabs
                .iter()
                .map(|t| {
                    let v = t.view.read(cx);
                    SavedTab {
                        url: v.current_url(),
                        title: v.title.clone(),
                        viewport: v.viewport.clone(),
                        history: v.history().map(|h| base64_encode(&h)),
                    }
                })
                .filter(|t| !t.url.is_empty())
                .collect(),
        }
    }
}
