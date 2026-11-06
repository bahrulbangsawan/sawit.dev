//! What the terminal's menus do to one pane (backlog item 22): Copy, Paste, Select All, Clear,
//! Reset Terminal, Export Output…, and the link or path a right-click landed on (Open Link, Copy
//! Link, Open File).

use std::path::PathBuf;

use gpui_kit::*;
use sawit_core::terminal::{Target, target_in};

use super::TerminalView;

impl TerminalView {
    /// Whether text is selected: Copy is on.
    pub fn has_selection(&self) -> bool {
        self.session.selection_text().is_some()
    }

    /// Copies the selection, if there is one.
    pub fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.session.selection_text() {
            self.copy(text, cx);
        }
    }

    /// Pastes the clipboard's text, as typed.
    pub fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            self.paste(&text);
            cx.notify();
        }
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.session.select_all();
        cx.notify();
    }

    /// Clears the scrollback and the screen; a shell at its prompt draws it again at the top.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.command_mark.set(None);
        self.session.clear(self.at_prompt());
        cx.notify();
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.command_mark.set(None);
        self.session.reset(self.at_prompt());
        cx.notify();
    }

    /// Asks where to save the scrollback and screen as text, then writes it there.
    pub fn export_output(&mut self, cx: &mut Context<Self>) {
        let text = self.session.all_text();
        let dir = self
            .cwd()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let name = format!("{}.txt", sanitize(&self.title));
        let path = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let note = match std::fs::write(&path, text.as_bytes()) {
                Ok(()) => format!(
                    "Saved {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
                Err(e) => format!("Unable to save: {e}"),
            };
            let _ = this.update(cx, |this, cx| {
                this.copy_saying(String::new(), note.into(), cx)
            });
        })
        .detach();
    }

    /// What a right-click at `position` landed on: a link or a path, else nothing.
    pub(super) fn target_at(&self, position: Point<Pixels>) -> Option<Target> {
        let (col, row, _) = self.cell_at(position)?;
        target_in(&self.session.token_at(col, row))
    }

    /// A path a right-click landed on, made absolute against the shell's folder (`~` too), if
    /// that file or folder exists.
    pub fn resolve_path(&self, path: &str) -> Option<PathBuf> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let path = match path.strip_prefix("~/") {
            Some(rest) => home?.join(rest),
            None => PathBuf::from(path),
        };
        let path = if path.is_absolute() {
            path
        } else {
            self.cwd()?.join(path)
        };
        path.exists().then_some(path)
    }

    /// Puts `text` (a link) on the clipboard.
    pub fn copy_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.copy(text, cx);
    }
}

/// A file name from a terminal's title: no slashes or colons, never empty.
fn sanitize(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| if "/:\\".contains(c) { '-' } else { c })
        .collect::<String>()
        .trim()
        .to_string();
    if name.is_empty() {
        "Terminal Output".into()
    } else {
        name
    }
}
