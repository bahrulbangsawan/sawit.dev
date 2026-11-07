//! The prompt library's sync beyond Back Up Now and Restore: what a restore brought in or deleted
//! put into the store, a backup held back by prompts that look like they hold secrets (with Back
//! Up Anyway), the pull on launch when automatic backup is on, and whether the repository exists,
//! so an empty library on a new Mac can offer Restore from GitHub.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::instances::AppPaths;
use sawit_core::prompt_sync::{self, BackupSettings, SyncOutcome};
use sawit_core::prompts::Library;

use super::panel::PromptsPanel;
use crate::git::account;
use crate::rt;
use crate::states::Banner;

/// How many secret-looking lines the held backup's notice lists.
const SHOWN: usize = 4;

impl PromptsPanel {
    /// A restore's work, into the store: the prompts it brought in replace or join the ones here,
    /// and the ones deleted on GitHub go.
    pub(super) fn apply_restored(
        &mut self,
        done: &SyncOutcome,
        restored: Library,
        cx: &mut Context<Self>,
    ) {
        self.store.update(cx, |s, cx| {
            for p in restored.items {
                if done.synced_ids.contains(&p.id) {
                    s.put(p, cx);
                }
            }
            for id in &done.deleted {
                s.remove(id, cx);
            }
        });
    }

    /// The notice of a backup held back: which lines look like secrets, and Back Up Anyway.
    pub(super) fn render_held(
        &self,
        secrets: Vec<String>,
        keep: Vec<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let mut lines: Vec<String> = secrets.iter().take(SHOWN).cloned().collect();
        if secrets.len() > SHOWN {
            lines.push(format!("and {} more", secrets.len() - SHOWN));
        }
        let message = format!(
            "Not backed up: these lines look like secrets, and would go to your private repository: {}.",
            lines.join("; ")
        );
        Banner::error("prompts-held", message)
            .action(
                Button::new("prompts-back-up-anyway")
                    .xsmall()
                    .outline()
                    .icon(Lucide::Upload)
                    .label("Back Up Anyway")
                    .on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.back_up_with(keep.clone(), true, cx)
                        }),
                    ),
            )
            .on_dismiss(cx.listener(|this, _, _, cx| {
                this.github.held = None;
                cx.notify();
            }))
    }

    /// Once signed in: with automatic backup on, brings in what changed on GitHub (never over a
    /// prompt changed here, which stays listed); with an empty library, asks whether the
    /// repository exists, for the empty state's Restore from GitHub.
    pub(super) fn on_launch(&mut self, cx: &mut Context<Self>) {
        if self.github.launched || self.github.busy.is_some() {
            return;
        }
        let Some(token) = account::global(cx).read(cx).token().map(String::from) else {
            return;
        };
        self.github.launched = true;
        let settings: BackupSettings = self.github.settings.clone();
        let empty = self.store.read(cx).library.items.is_empty();
        let pull = settings.auto;
        if !pull && !empty {
            return;
        }
        let mut library = self.store.read(cx).library.clone();
        let state = AppPaths::default_macos().root.join(prompt_sync::SYNC_STATE);
        let rx = rt::spawn(cx, async move {
            let exists = prompt_sync::fetch_backup(&token, &settings.repo)
                .await
                .map(|(_, found)| found.is_some());
            let pulled = match exists {
                Ok(true) if pull => {
                    prompt_sync::restore(&token, &mut library, &settings, &state, &[], false)
                        .await
                        .ok()
                        .map(|done| (done, library))
                }
                _ => None,
            };
            (exists.ok(), pulled)
        });
        cx.spawn(async move |this, cx| {
            let Ok((exists, pulled)) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.github.repo_exists = exists;
                if let Some((done, restored)) = pulled
                    && (!done.synced.is_empty() || !done.deleted.is_empty())
                {
                    this.apply_restored(&done, restored, cx);
                    this.github.synced_library = Some(this.store.read(cx).library.clone());
                    let n = done.synced.len() + done.deleted.len();
                    this.github.message = Some((
                        match n {
                            1 => "Brought in 1 change from GitHub.".to_string(),
                            n => format!("Brought in {n} changes from GitHub."),
                        },
                        false,
                    ));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The empty library's second action: Restore from GitHub, when the repository exists.
    pub(super) fn restore_offer(&self, cx: &mut Context<Self>) -> Option<Button> {
        let signed_in = account::global(cx).read(cx).is_signed_in();
        (signed_in && self.github.repo_exists == Some(true)).then(|| {
            Button::new("empty-restore-prompts")
                .small()
                .outline()
                .icon(Lucide::Download)
                .label("Restore from GitHub")
                .tooltip("Bring In the Prompts Backed Up from Another Mac")
                .on_click(cx.listener(|this, _, window, cx| this.restore(Vec::new(), window, cx)))
        })
    }
}
