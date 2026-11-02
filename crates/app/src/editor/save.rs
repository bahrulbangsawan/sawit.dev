//! Saving the buffer (⌘S, and Save in the questions that name the file) and formatting it (⇧⌘I).

use gpui_kit::component::input::{EditorState, Position};
use gpui_kit::{App, Context, Entity, Task, Window};
use sawit_core::CoreError;
use sawit_core::files::write_text_atomic;
use sawit_core::format::{format_text, tool_for};
use sawit_core::shell_config::write_text_as_admin;

use super::{FileEditor, FileEditorEvent, Format, Save, error, mtime};
use crate::rt;

impl FileEditor {
    /// ⌘S: writes the buffer, unless the tab is read-only.
    pub(super) fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        self.save_task(cx).detach();
    }

    /// Writes the buffer; resolves to whether the file is now saved. A read-only tab is written
    /// too: but for ⌘S, which keeps to the mark ([`Self::save`]), the callers are the user's
    /// Save in a question that names this file (closing its tab, quitting, Restart to Update,
    /// closing its window). That answer is the user's own: refused, it did nothing and said
    /// nothing, and quitting looked stuck (site M1).
    pub fn save_task(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        let Some(state) = &self.state else {
            return Task::ready(false);
        };
        let text = state.read(cx).value().to_string();
        let path = self.path.clone();
        let to_write = text.clone();
        let rx = rt::spawn(cx, async move {
            let written = match write_text_atomic(&path, &to_write) {
                // Root-owned (e.g. /etc/zshrc): ask for the admin password.
                Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    write_text_as_admin(&path, &to_write).await
                }
                other => other,
            };
            written.map(|()| mtime(&path))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            this.update(cx, |this, cx| {
                let saved = matches!(result, Ok(Ok(_)));
                match result {
                    Ok(Ok(written)) => {
                        this.saved_text = text;
                        this.dirty = false;
                        this.notice = None;
                        // Our own write is not an outside change.
                        this.disk_mtime = written;
                        this.disk_warning = None;
                    }
                    Ok(Err(e)) => this.notice = Some(error(format!("Unable to save. {e}"))),
                    Err(_) => this.notice = Some(error("Unable to save. Try again.".into())),
                }
                cx.emit(FileEditorEvent::Changed);
                cx.notify();
                saved
            })
            .unwrap_or(false)
        })
    }

    pub(super) fn format(&mut self, _: &Format, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.state.clone() else {
            return;
        };
        if self.read_only || self.formatting {
            return;
        }
        let Some(tool) = tool_for(&self.path) else {
            self.notice = Some(error("No formatter for this file type yet.".into()));
            cx.notify();
            return;
        };
        self.formatting = true;
        self.notice = None;
        cx.notify();
        let text = state.read(cx).value().to_string();
        let path = self.path.clone();
        let rx = rt::spawn(cx, async move { format_text(&path, &text).await });
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                this.formatting = false;
                match result {
                    Ok(Ok(formatted)) => {
                        let unchanged = *state.read(cx).text() == formatted;
                        if !unchanged {
                            apply_formatted(&state, formatted, window, cx);
                        }
                        this.flash(
                            if unchanged {
                                format!("Already formatted ({}).", tool.name())
                            } else {
                                format!("Formatted with {}.", tool.name())
                            },
                            cx,
                        );
                    }
                    Ok(Err(e)) => this.notice = Some(error(e.to_string())),
                    Err(_) => {
                        this.notice = Some(error(
                            "The formatter stopped unexpectedly. Try again.".into(),
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Replaces the whole buffer as one undo step and puts the cursor back on
/// the same line and column (clamped by the editor).
fn apply_formatted(
    state: &Entity<EditorState>,
    formatted: String,
    window: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |s, cx| {
        let pos = s.cursor_position();
        s.select_all(window, cx);
        s.replace(formatted, window, cx);
        s.set_cursor_position(Position::new(pos.line, pos.character), window, cx);
    });
}
