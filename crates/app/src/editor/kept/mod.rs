//! Unsaved changes kept when Sawit quits without asking. ⌘Q, Restart to Update and closing a
//! window ask to save unsaved files first ([`crate::terminal::end_all`]), but the Dock's Quit,
//! logging out and the install script's `quit app "Sawit"` end the app at once: macOS asks the
//! app nothing it could refuse (GPUI implements no `applicationShouldTerminate:`). So as the app
//! quits, each file editor with unsaved changes keeps them here, in Sawit's own folder, and the
//! next launch opens each file again with its changes back in, unsaved (SITE-1). Changes the user
//! chose Don't Save for aren't kept. Each keeps the file's text it was made to: a file that changed
//! on disk after that (a `git pull` before the next launch) gets its changes back with a warning
//! that saving writes over the version on disk, not a note inviting the save.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui_kit::{App, Context, Global, Window};
use serde::{Deserialize, Serialize};

use super::{FileEditor, FileEditorEvent, Save, error, press};

/// One file's kept changes: the file, the text its editor held, and the file's text they were
/// made to.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Kept {
    pub path: PathBuf,
    pub text: String,
    /// The file's text the changes were made to: as their editor last loaded or saved it, or, for
    /// changes put back over a file that had changed since, the text they were first made to.
    /// The next launch compares it with the file as it is then. None when Sawit doesn't know it:
    /// changes kept by a build from before this field.
    #[serde(default)]
    pub base: Option<String>,
}

/// A folder for kept changes in place of Sawit's own: the tests'.
pub struct KeptFolder(pub PathBuf);

impl Global for KeptFolder {}

/// Where kept changes go: `unsaved/editor/` in Sawit's own folder, beside the copies a canvas that
/// can't save keeps in `unsaved/`.
pub fn folder(cx: &App) -> PathBuf {
    cx.try_global::<KeptFolder>()
        .map_or_else(default_folder, |kept| kept.0.clone())
}

#[cfg(not(test))]
fn default_folder() -> PathBuf {
    sawit_core::instances::AppPaths::default_macos()
        .root
        .join("unsaved")
        .join("editor")
}

/// A folder of the tests' own in place of Sawit's.
#[cfg(test)]
fn default_folder() -> PathBuf {
    std::env::temp_dir().join(format!("sawit-editor-{}-kept", std::process::id()))
}

/// Keeps `kept` in a file of its own in `folder`, made new there and never over another:
/// `<file name>.json`, numbered when that is taken. The file it made.
pub fn keep(folder: &Path, kept: &Kept) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let name = kept
        .path
        .file_name()
        .map_or_else(|| "file".into(), |name| name.to_string_lossy());
    let json = serde_json::to_vec(kept)?;
    sawit_core::nyawit::write_unique(folder, &format!("{name}.json"), &json)
}

/// Every change kept in `folder`, oldest first, each with the file that holds it. Anything else in
/// the folder is left as it is.
pub fn all(folder: &Path) -> Vec<(PathBuf, Kept)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<(Option<SystemTime>, PathBuf, Kept)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|file| file.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|file| {
            let kept = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
            let made = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
            Some((made, file, kept))
        })
        .collect();
    found.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    found
        .into_iter()
        .map(|(_, file, kept)| (file, kept))
        .collect()
}

impl FileEditor {
    /// The user chose Don't Save for these changes as the app quits or restarts: the quit doesn't
    /// keep them for the next launch.
    pub fn discard(&mut self) {
        self.discarded = true;
    }

    /// Puts back changes the app kept when it last quit with this file unsaved ([`kept`](self)):
    /// the buffer holds them, unsaved, now or once the file has loaded, and `from`, the file that
    /// kept them, goes once they're in. When the file on disk is no longer the text they were made
    /// to, the note warns that saving writes over it. A file that can't be edited (binary, too
    /// large, unreadable) leaves `from` for the next launch to try again, and its note says where
    /// the changes are ([`Self::cannot_edit`]).
    pub fn restore(
        &mut self,
        kept: Kept,
        from: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.is_some() {
            self.put_back(kept, &from, window, cx);
            return;
        }
        self.restoring = Some((kept, from));
        // The file had already failed to load.
        if let Some(why) = self.unreadable.clone() {
            self.cannot_edit(why);
            cx.notify();
        }
    }

    pub(super) fn put_back(
        &mut self,
        kept: Kept,
        from: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let Kept { text, base, .. } = kept;
        state.update(cx, |s, cx| s.set_value(text.clone(), window, cx));
        // set_value doesn't emit Change: the unsaved mark and the preview follow here.
        self.dirty = text != self.saved_text;
        self.discarded = false;
        self.preview = text.into();
        self.refresh_diagrams(cx);
        if let Err(e) = std::fs::remove_file(from) {
            log::warn!("{} stays: {e}", from.display());
        }
        // The file as it loaded now (`saved_text`, empty when it is gone) isn't the text the
        // changes were made to, or Sawit can't tell: it changed after they were made, while Sawit
        // ran or after it quit (a `git pull`, another editor). A save writes over that change, so
        // the note warns, as `on_disk_mtime` does while Sawit runs, and doesn't invite the save.
        let changed = self.dirty && base.as_deref() != Some(self.saved_text.as_str());
        if changed {
            let (what, then) = match self.disk_mtime {
                Some(_) => ("changed on disk", "to overwrite the version on disk"),
                None => ("was deleted from disk", "to save it again"),
            };
            self.notice = Some(error(format!(
                "Unsaved changes Sawit kept when it quit are back, but this file {what} after they were made. {} {then}, or close the tab to drop them.",
                press(&Save, window)
            )));
        } else {
            self.flash(
                "Unsaved changes Sawit kept when it quit are back. Save them, or close the tab to drop them.".into(),
                cx,
            );
        }
        self.restored_over = changed.then(|| (self.saved_text.clone(), base));
        cx.emit(FileEditorEvent::Changed);
        cx.notify();
    }

    /// The app is quitting with this file's changes unsaved. ⌘Q, Restart to Update and closing a
    /// window ask first, but the Dock's Quit, logging out and the install script's quit end the
    /// app with no way to ask: the changes are kept for the next launch to bring back
    /// ([`kept`](self)), unless the user chose Don't Save. The file itself isn't written.
    pub(super) fn keep_unsaved(&mut self, cx: &App) {
        if !self.dirty || self.discarded {
            return;
        }
        let Some(state) = &self.state else {
            return;
        };
        let base = match &self.restored_over {
            // Put back over a file that had changed, and neither saved nor reloaded since: the
            // changes are still made to the older text, whatever the file holds now.
            Some((over, base)) if *over == self.saved_text => base.clone(),
            // When the file changes on disk under unsaved changes while Sawit runs, `saved_text`
            // stays the text they were made to, so the next launch sees that change too.
            _ => Some(self.saved_text.clone()),
        };
        let kept = Kept {
            path: self.path.clone(),
            text: state.read(cx).value().to_string(),
            base,
        };
        match keep(&folder(cx), &kept) {
            Ok(file) => log::warn!(
                "{} has unsaved changes; they're kept in {} for the next launch",
                self.path.display(),
                file.display()
            ),
            Err(e) => log::error!(
                "{}'s unsaved changes couldn't be kept: {e}",
                self.path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod quit_tests;
