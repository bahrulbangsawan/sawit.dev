//! The user's saved commands for every project; writes to disk shortly
//! after the last change.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::Context;
use sawit_core::instances::AppPaths;
use sawit_core::scripts::{SavedCommand, SavedCommands};

use crate::rt;

/// Wait this long after the last edit before writing the file.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

pub struct RunStore {
    file: PathBuf,
    pub saved: SavedCommands,
    /// Set when the file exists but can't be read: nothing is written then,
    /// so a damaged file is never replaced by an empty list.
    pub load_error: Option<String>,
    pub notice: Option<String>,
    save_scheduled: bool,
}

impl RunStore {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let file = AppPaths::default_macos().root.join("run-commands.json");
        let rx = rt::spawn(cx, {
            let file = file.clone();
            async move { SavedCommands::load(&file) }
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(saved)) => this.saved = saved,
                    Ok(Err(e)) => this.load_error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            file,
            saved: SavedCommands::default(),
            load_error: None,
            notice: None,
            save_scheduled: false,
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        if self.load_error.is_some() || self.save_scheduled {
            return;
        }
        self.save_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let Ok((saved, file)) = this.update(cx, |this, _| {
                this.save_scheduled = false;
                (this.saved.clone(), this.file.clone())
            }) else {
                return;
            };
            let rx = cx.update(|cx| rt::spawn(cx, async move { saved.save(&file) }));
            if let Ok(Err(e)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.notice = Some(format!("Unable to save commands. {e}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Adds `command` to the project at `root`, or replaces it (same id).
    pub fn upsert(&mut self, root: &Path, command: SavedCommand, cx: &mut Context<Self>) {
        self.saved.upsert(root, command);
        self.changed(cx);
    }

    /// Reads the commands again after another process (`sawit-mcp`)
    /// changed them.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        match SavedCommands::load(&self.file) {
            Ok(saved) => {
                self.saved = saved;
                self.load_error = None;
            }
            Err(e) => self.notice = Some(format!("Unable to read saved commands. {e}")),
        }
        cx.notify();
    }

    pub fn remove(&mut self, root: &Path, id: &str, cx: &mut Context<Self>) {
        if self.saved.remove(root, id) {
            self.changed(cx);
        }
    }
}
