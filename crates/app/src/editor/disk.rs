//! The open file on disk: looked at every few seconds, reloaded or warned about when it changes.

use std::time::SystemTime;

use gpui_kit::component::input::Position;
use gpui_kit::{Context, Window};
use sawit_core::files::read_text;

use super::{DISK_POLL, FileEditor, FileEditorEvent, Save, error, mtime, press};
use crate::rt;

impl FileEditor {
    pub(super) fn watch_disk(&self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, window| {
            loop {
                window.background_executor().timer(DISK_POLL).await;
                let Ok(path) = this.read_with(window, |this, _| this.path.clone()) else {
                    return;
                };
                let rx = window
                    .update(|_, cx| rt::spawn_blocking(cx, move || mtime(&path)))
                    .ok();
                let Some(Ok(now)) = (match rx {
                    Some(rx) => Some(rx.await),
                    None => None,
                }) else {
                    return;
                };
                if this
                    .update_in(window, |this, window, cx| {
                        this.on_disk_mtime(now, window, cx)
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }

    pub(super) fn on_disk_mtime(
        &mut self,
        now: Option<SystemTime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.is_none() {
            return;
        }
        if now == self.disk_mtime {
            // Back as it was loaded or saved, as a file the Files tab put back from the Trash
            // (⌘Z) is: what the user was told of it is out of date.
            if now.is_some()
                && let Some(warning) = self.disk_warning.take()
            {
                if self.notice.as_ref().is_some_and(|n| n.text == warning) {
                    self.notice = None;
                }
                cx.notify();
            }
            return;
        }
        let Some(now) = now else {
            self.warn_disk(
                format!(
                    "This file was deleted from disk. {} to save it again.",
                    press(&Save, window)
                ),
                cx,
            );
            return;
        };
        if self.dirty {
            self.warn_disk(
                format!(
                    "This file changed on disk while you have unsaved changes. {} to overwrite the version on disk.",
                    press(&Save, window)
                ),
                cx,
            );
            return;
        }
        // Clean buffer: take the new contents, like Zed does.
        self.disk_mtime = Some(now);
        let path = self.path.clone();
        let rx = rt::spawn_blocking(cx, move || read_text(&path));
        cx.spawn_in(window, async move |this, window| {
            let Ok(Ok(file)) = rx.await else { return };
            let _ = this.update_in(window, |this, window, cx| {
                let (Some(state), None) = (this.state.clone(), file.read_only_reason) else {
                    return;
                };
                if this.dirty || *state.read(cx).text() == file.text {
                    return;
                }
                state.update(cx, |s, cx| {
                    let pos = s.cursor_position();
                    s.set_value(file.text.clone(), window, cx);
                    s.set_cursor_position(Position::new(pos.line, pos.character), window, cx);
                });
                // set_value doesn't emit Change, so refresh the preview here.
                this.preview = file.text.clone().into();
                this.refresh_diagrams(cx);
                this.saved_text = file.text;
                this.dirty = false;
                this.disk_warning = None;
                this.flash("Reloaded because the file changed on disk.".into(), cx);
                cx.emit(FileEditorEvent::Changed);
                cx.notify();
            });
        })
        .detach();
    }

    /// Tells the user `warning` of the file on disk, once until it is saved, reloaded or back.
    fn warn_disk(&mut self, warning: String, cx: &mut Context<Self>) {
        if self.disk_warning.is_none() {
            self.notice = Some(error(warning.clone()));
            self.disk_warning = Some(warning);
            cx.notify();
        }
    }
}
