//! The commands a config file's context menu and the list's keys run on the selected file:
//! duplicate it, move it to the Trash, or delete it, each off the main thread.

use std::path::{Path, PathBuf};

use gpui_kit::*;
use sawit_core::files;
use sawit_core::recent::tildify;

use super::ShellConfigPanel;
use crate::files::ops::Op;
use crate::rt;

/// Commands from the context menu and their shortcuts.
impl ShellConfigPanel {
    /// The selected file, if it is still listed and saving it needs no password.
    fn selected_writable(&self) -> Option<PathBuf> {
        let path = self.selected.as_ref()?;
        let shell = self.configs.iter().map(|c| (&c.path, c.writable));
        let agents = self.agents.iter().map(|c| (&c.path, c.writable));
        shell
            .chain(agents)
            .find(|(p, writable)| *p == path && *writable)
            .map(|(p, _)| p.clone())
    }

    /// Copies the file next to itself (`.zshrc copy`), as a backup.
    pub(super) fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(from) = self.selected_writable() else {
            return;
        };
        let (Some(dir), Some(name)) = (from.parent(), from.file_name()) else {
            return;
        };
        let to = files::free_name(dir, &name.to_string_lossy(), false);
        self.run(move || Op::Copy { from, to }.run().map(drop), cx);
    }

    /// Moves the selected file to the Trash, with `ask` once the user says so (⌫; ⌘⌫ doesn't
    /// ask), as in the Files tab: nothing here undoes it.
    pub(super) fn trash(&mut self, ask: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected_writable() else {
            return;
        };
        if !ask {
            self.run(move || Op::Trash(path).run().map(drop), cx);
            return;
        }
        let detail = "You can put it back from the Trash in Finder.";
        let answer = crate::files::ask_to_trash(&path, detail, window, cx);
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    this.run(move || Op::Trash(path).run().map(drop), cx)
                });
            }
        })
        .detach();
    }

    /// Deletes the selected file for good, after asking.
    pub(super) fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected_writable() else {
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &delete_question(
                &path,
                std::env::var_os("HOME").map(PathBuf::from).as_deref(),
            ),
            Some("It won't go to the Trash, and this can't be undone."),
            &["Delete", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    this.run(move || files::remove(&path).map_err(|e| e.to_string()), cx)
                });
            }
        })
        .detach();
    }

    /// Runs a file change off the main thread, then looks again.
    fn run(
        &mut self,
        f: impl FnOnce() -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        let rx = rt::spawn_blocking(cx, f);
        cx.spawn(async move |this, cx| {
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(Err(e)) = outcome {
                    this.error = Some(e);
                }
                this.refresh(cx);
            });
        })
        .detach();
    }
}

/// The question before deleting `path`: by its path, as the list shows it,
/// since many files there share a name (every profile's `settings.json`).
pub(super) fn delete_question(path: &Path, home: Option<&Path>) -> String {
    format!("Delete “{}” permanently?", tildify(path, home))
}
