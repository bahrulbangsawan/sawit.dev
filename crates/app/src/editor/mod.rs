//! A text file open in a center tab: code editor with highlighting, soft
//! wrap, dirty tracking, Cmd-S to save, Shift-Cmd-I to format, and a live
//! preview (side by side or alone) for Markdown, HTML, SVG and Mermaid.

mod diagrams;
pub mod kept;
pub(crate) use diagrams::palette;
#[cfg(test)]
pub(crate) mod testing;

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Editor, EditorState, InputEvent, Position, TabSize};
use gpui_kit::component::text::TextView;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::CoreError;
use sawit_core::files::{
    FileKind, TextFile, classify, language_for, language_label, read_text, write_text_atomic,
};
use sawit_core::format::{format_text, tool_for};
use sawit_core::shell_config::write_text_as_admin;

use crate::code_actions::code_block_actions;
use crate::rt;

actions!(file_editor, [Save, Format]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-s", Save, Some("FileEditor")),
        // Zed's binding for editor::Format.
        KeyBinding::new("cmd-shift-i", Format, Some("FileEditor")),
    ]);
}

pub enum FileEditorEvent {
    Changed,
}

/// Every open file editor, with the window it is in: what quitting, Restart to Update and closing
/// a window ask about ([`unsaved`]).
#[derive(Default)]
struct Editors(Vec<(AnyWindowHandle, WeakEntity<FileEditor>)>);

impl Global for Editors {}

/// The open files with unsaved changes, each with its window, in the order they were opened.
pub fn unsaved(cx: &App) -> Vec<(AnyWindowHandle, Entity<FileEditor>)> {
    let Some(editors) = cx.try_global::<Editors>() else {
        return Vec::new();
    };
    editors
        .0
        .iter()
        .filter_map(|(window, editor)| Some((*window, editor.upgrade()?)))
        .filter(|(_, editor)| editor.read(cx).dirty)
        .collect()
}

impl EventEmitter<FileEditorEvent> for FileEditor {}

/// How a previewable file (Markdown, HTML, SVG) is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Code,
    Preview,
    Split,
}

/// How often an open file is checked for changes made outside the editor.
const DISK_POLL: Duration = Duration::from_secs(2);

/// Wait this long after the last keystroke before re-rendering a preview.
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Clone)]
struct Notice {
    text: String,
    error: bool,
}

pub struct FileEditor {
    pub path: PathBuf,
    /// Project folder, for the breadcrumb.
    root: Option<PathBuf>,
    kind: FileKind,
    language: String,
    state: Option<Entity<EditorState>>,
    saved_text: String,
    pub dirty: bool,
    /// Set from the tab menu ("Make Tab Read-Only"): no edits, and ⌘S doesn't save. Save in a
    /// question that names the file still does ([`Self::save_task`]).
    pub read_only: bool,
    /// The user chose Don't Save for these changes as the app quit or restarted, so quitting
    /// doesn't keep them. The next edit clears it.
    discarded: bool,
    /// Changes the app kept when it last quit with this file unsaved, and the file that holds
    /// them, to put back once the file has loaded ([`Self::restore`]).
    restoring: Option<(kept::Kept, PathBuf)>,
    /// Kept changes put back over a file that had changed on disk after they were made
    /// ([`Self::put_back`]): the file's text as it loaded then, and the text the changes were made
    /// to (None: Sawit doesn't know it). While `saved_text` is still the former, neither saved
    /// nor reloaded since, a quit keeps the changes as made to the latter, so the next launch
    /// warns again.
    restored_over: Option<(String, Option<String>)>,
    notice: Option<Notice>,
    loading: bool,
    formatting: bool,
    mode: ViewMode,
    /// Text the preview renders (lags typing by `PREVIEW_DEBOUNCE`).
    preview: SharedString,
    preview_scheduled: bool,
    /// The SVG preview image and the text it was built from.
    svg_image: Option<(SharedString, Arc<Image>)>,
    /// Mermaid diagrams rendered from `preview`.
    diagrams: diagrams::Diagrams,
    /// Modification time of the file as last loaded or saved.
    disk_mtime: Option<SystemTime>,
    /// What the user was told of the file on disk (it changed under unsaved edits, or went),
    /// until it is saved, reloaded or back as it was.
    disk_warning: Option<String>,
    /// 1-based line/column from `go_to` before the text finished loading.
    pending_cursor: Option<(u32, u32)>,
    /// Cursor line/column the toolbar last showed.
    shown_cursor: Option<(u32, u32)>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    /// A quit that couldn't ask keeps the unsaved changes ([`Self::keep_unsaved`]).
    _quit: Subscription,
}

impl FileEditor {
    pub fn open(
        path: PathBuf,
        root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let read_path = path.clone();
        let rx = rt::spawn_blocking(cx, move || {
            let modified = mtime(&read_path);
            // `sawit new.rs` opens an empty buffer; saving creates the file.
            if !read_path.exists() {
                return Ok((
                    TextFile {
                        text: String::new(),
                        read_only_reason: None,
                    },
                    modified,
                ));
            }
            read_text(&read_path).map(|file| (file, modified))
        });
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                match result {
                    Ok(Ok((file, modified))) => {
                        this.disk_mtime = modified;
                        this.loaded(file, window, cx)
                    }
                    Ok(Err(e)) => {
                        this.notice = Some(error(format!("Unable to open this file. {e}")))
                    }
                    Err(_) => {}
                }
                this.loading = false;
                cx.notify();
            });
        })
        .detach();
        let (me, here) = (cx.weak_entity(), window.window_handle());
        let editors = cx.default_global::<Editors>();
        editors.0.retain(|(_, editor)| editor.upgrade().is_some());
        editors.0.push((here, me));
        let kind = classify(&path);
        Self {
            language: language_for(&path),
            mode: match kind {
                // Markdown and diagrams read best side by side; HTML/SVG
                // start as code.
                FileKind::Markdown | FileKind::Mermaid => ViewMode::Split,
                _ => ViewMode::Code,
            },
            kind,
            path,
            root,
            state: None,
            saved_text: String::new(),
            dirty: false,
            read_only: false,
            discarded: false,
            restoring: None,
            restored_over: None,
            notice: None,
            loading: true,
            formatting: false,
            preview: SharedString::default(),
            preview_scheduled: false,
            svg_image: None,
            diagrams: Default::default(),
            disk_mtime: None,
            disk_warning: None,
            pending_cursor: None,
            shown_cursor: None,
            focus: cx.focus_handle(),
            _subscriptions: Vec::new(),
            _quit: cx.on_app_quit(|this, cx| {
                this.keep_unsaved(cx);
                async {}
            }),
        }
    }

    fn watch_disk(&self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn on_disk_mtime(
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
                if this.dirty || state.read(cx).value().as_ref() == file.text {
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

    pub fn title(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub fn previewable(&self) -> bool {
        matches!(
            self.kind,
            FileKind::Markdown | FileKind::Html | FileKind::Svg | FileKind::Mermaid
        )
    }

    pub fn set_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        if self.previewable() {
            self.mode = mode;
            cx.notify();
        }
    }

    fn loaded(&mut self, file: TextFile, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(reason) = file.read_only_reason {
            self.notice = Some(Notice {
                text: reason,
                error: false,
            });
            return;
        }
        let text = file.text;
        let language = self.language.clone();
        let state = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language(language)
                .line_number(true)
                // No fold column: keeps the gutter tight to the numbers.
                .folding(false)
                .indent_guides(true)
                .tab_size(TabSize {
                    tab_size: 2,
                    hard_tabs: false,
                })
                // Long lines wrap: no horizontal scrolling.
                .soft_wrap(true)
                .default_value(text.clone());
            // Parses off the UI thread; see crate::highlight.
            crate::highlight::install(&mut state, cx);
            // No scrollbar: trackpad/wheel scrolling only, like a clean Zed pane.
            state.set_scrollbar_renderer(Some(Rc::new(|_, _, _| div().into_any_element())));
            state
        });
        self._subscriptions = vec![
            cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value();
                    let dirty =
                        text.len() != this.saved_text.len() || text.as_ref() != this.saved_text;
                    if dirty != this.dirty {
                        this.dirty = dirty;
                        cx.emit(FileEditorEvent::Changed);
                    }
                    this.discarded = false;
                    if !dirty {
                        // Back as the file loaded or saved: what was put back over it is gone.
                        this.restored_over = None;
                    }
                    this.schedule_preview(cx);
                    cx.notify();
                }
            }),
            // Cursor moves re-render the "Ln, Col" in the toolbar. The state
            // also notifies on every cursor blink; those change nothing here.
            cx.observe(&state, |this, state, cx| {
                let pos = state.read(cx).cursor_position();
                let pos = (pos.line, pos.character);
                if this.shown_cursor != Some(pos) {
                    this.shown_cursor = Some(pos);
                    cx.notify();
                }
            }),
        ];
        self.preview = text.clone().into();
        self.refresh_diagrams(cx);
        self.saved_text = text;
        state.update(cx, |s, cx| s.focus(window, cx));
        self.state = Some(state);
        if let Some((line, column)) = self.pending_cursor.take() {
            self.go_to(line, column, window, cx);
        }
        if let Some((kept, from)) = self.restoring.take() {
            self.put_back(kept, &from, window, cx);
        }
        self.watch_disk(window, cx);
    }

    /// Gives this file the keyboard: its text once read, until then the editor itself, whose
    /// text takes it when the read lands (as on any first open).
    pub fn take_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.state.clone() {
            Some(state) => state.update(cx, |s, cx| s.focus(window, cx)),
            None => window.focus(&self.focus, cx),
        }
    }

    /// The keyboard of its text, once read.
    #[cfg(test)]
    pub(crate) fn text_focus(&self, cx: &App) -> Option<FocusHandle> {
        self.state.as_ref().map(|s| s.focus_handle(cx))
    }

    /// The note under the toolbar, and whether it is a warning.
    #[cfg(test)]
    pub(crate) fn note(&self) -> Option<(&str, bool)> {
        self.notice.as_ref().map(|n| (n.text.as_str(), n.error))
    }

    /// Puts the cursor on a 1-based line and column (clamped), now or once
    /// the file has loaded.
    pub fn go_to(&mut self, line: u32, column: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.state.clone() else {
            self.pending_cursor = Some((line, column));
            return;
        };
        state.update(cx, |s, cx| {
            let last = s.value().lines().count().saturating_sub(1) as u32;
            let line = line.saturating_sub(1).min(last);
            s.set_cursor_position(Position::new(line, column.saturating_sub(1)), window, cx);
        });
        cx.notify();
    }

    fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        if !self.previewable() || self.preview_scheduled {
            return;
        }
        self.preview_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                this.preview_scheduled = false;
                if let Some(state) = &this.state {
                    this.preview = state.read(cx).value();
                    this.refresh_diagrams(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The user chose Don't Save for these changes as the app quits or restarts: the quit doesn't
    /// keep them for the next launch.
    pub fn discard(&mut self) {
        self.discarded = true;
    }

    /// Puts back changes the app kept when it last quit with this file unsaved ([`kept`]): the
    /// buffer holds them, unsaved, now or once the file has loaded, and `from`, the file that kept
    /// them, goes once they're in. When the file on disk is no longer the text they were made to,
    /// the note warns that saving writes over it. A file that can't be edited (binary, too large,
    /// unreadable) leaves `from` for the next launch to try again.
    pub fn restore(
        &mut self,
        kept: kept::Kept,
        from: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.is_some() {
            self.put_back(kept, &from, window, cx);
        } else {
            self.restoring = Some((kept, from));
        }
    }

    fn put_back(
        &mut self,
        kept: kept::Kept,
        from: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let kept::Kept { text, base, .. } = kept;
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
    /// ([`kept`]), unless the user chose Don't Save. The file itself isn't written.
    fn keep_unsaved(&mut self, cx: &App) {
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
        let kept = kept::Kept {
            path: self.path.clone(),
            text: state.read(cx).value().to_string(),
            base,
        };
        match kept::keep(&kept::folder(cx), &kept) {
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

    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        cx.emit(FileEditorEvent::Changed);
        cx.notify();
    }

    /// ⌘S: writes the buffer, unless the tab is read-only.
    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
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

    /// A short-lived, non-error note under the toolbar.
    fn flash(&mut self, text: String, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            text: text.clone(),
            error: false,
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            let _ = this.update(cx, |this, cx| {
                if this.notice.as_ref().is_some_and(|n| n.text == text) {
                    this.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn format(&mut self, _: &Format, window: &mut Window, cx: &mut Context<Self>) {
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
                        let unchanged = state.read(cx).value().as_ref() == formatted;
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

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let crumbs = breadcrumb(&self.path, self.root.as_deref());
        let last = crumbs.len().saturating_sub(1);
        let cursor = self.state.as_ref().map(|s| s.read(cx).cursor_position());
        let mode_button =
            |id: &'static str, icon: Lucide, tooltip: &'static str, mode: ViewMode, current| {
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .tooltip(tooltip)
                    .selected(current == mode)
            };
        h_flex()
            .h(px(28.))
            .px_3()
            .gap_2()
            .text_xs()
            .text_color(muted)
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .overflow_x_hidden()
                    .children(crumbs.into_iter().enumerate().map(|(ix, part)| {
                        h_flex()
                            .gap_1()
                            .flex_none()
                            .when(ix > 0, |el| el.child("›"))
                            .child(
                                div()
                                    .when(ix == last, |el| el.text_color(cx.theme().foreground))
                                    .child(part),
                            )
                    })),
            )
            .when(self.previewable(), |el| {
                let current = self.mode;
                el.child(
                    h_flex()
                        .gap_0p5()
                        .child(
                            mode_button("mode-code", Lucide::Code, "Code", ViewMode::Code, current)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.set_mode(ViewMode::Code, cx)),
                                ),
                        )
                        .child(
                            mode_button(
                                "mode-split",
                                Lucide::Columns2,
                                "Side by Side",
                                ViewMode::Split,
                                current,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_mode(ViewMode::Split, cx)),
                            ),
                        )
                        .child(
                            mode_button(
                                "mode-preview",
                                Lucide::Eye,
                                "Preview",
                                ViewMode::Preview,
                                current,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_mode(ViewMode::Preview, cx)),
                            ),
                        ),
                )
            })
            .when_some(cursor, |el, pos| {
                el.child(format!("Ln {}, Col {}", pos.line + 1, pos.character + 1))
            })
            .child(language_label(&self.language))
            .children(self.download_button(cx))
            .when(self.read_only, |el| {
                el.child(Icon::new(Lucide::Lock).xsmall())
            })
            .when(
                tool_for(&self.path).is_some() && self.state.is_some(),
                |el| {
                    el.child(
                        Button::new("format")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::WandSparkles)
                            .tooltip_with_action("Format Document", &Format, Some("FileEditor"))
                            .loading(self.formatting)
                            .disabled(self.read_only)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.format(&Format, window, cx)),
                            ),
                    )
                },
            )
    }

    fn render_preview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let id = ElementId::Name(format!("preview\u{0}{}", self.path.display()).into());
        let dir = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let body = match self.kind {
            FileKind::Svg => {
                // Built once per preview text, not per frame: a fresh Image
                // copies and hashes the whole file every render.
                let image = match &self.svg_image {
                    Some((text, image)) if *text == self.preview => image.clone(),
                    _ => {
                        let image = Arc::new(Image::from_bytes(
                            ImageFormat::Svg,
                            self.preview.as_bytes().to_vec(),
                        ));
                        self.svg_image = Some((self.preview.clone(), image.clone()));
                        image
                    }
                };
                return div()
                    .size_full()
                    .p_6()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(cx.theme().muted)
                    .child(img(image).max_w_full().max_h_full())
                    .into_any_element();
            }
            FileKind::Mermaid => return self.render_diagram(cx),
            FileKind::Html => TextView::html(id, self.preview.clone()),
            _ => TextView::markdown(id, self.diagrams.markdown.clone()),
        };
        let images = self.diagrams.images.clone();
        let this = cx.entity().downgrade();
        div()
            .size_full()
            .bg(cx.theme().background)
            .child(
                body.scrollable(true)
                    .selectable(true)
                    .code_block_actions(code_block_actions)
                    .image_source(move |uri| {
                        diagrams::Diagrams::image(&images, uri)
                            .unwrap_or_else(|| resolve_image(&dir, uri))
                    })
                    .on_link_click(move |url, _, _, cx| {
                        if url.starts_with("http://") || url.starts_with("https://") {
                            cx.open_url(url);
                        } else {
                            let _ = this.update(cx, |this, cx| this.diagram_link(url, cx));
                        }
                    })
                    .p_6(),
            )
            .into_any_element()
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

/// Images in previews: URLs and data URIs as-is, everything else relative
/// to the file's folder.
fn resolve_image(dir: &Path, uri: &SharedUri) -> ImageSource {
    let s = uri.as_ref();
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:") {
        return ImageSource::from(uri.clone());
    }
    let local = s.strip_prefix("file://").unwrap_or(s);
    let path = Path::new(local);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    };
    ImageSource::from(path)
}

/// Path parts shown above the editor: relative to the project when inside it.
fn breadcrumb(path: &Path, root: Option<&Path>) -> Vec<String> {
    let shown = root
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path);
    shown
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|c| c != "/")
        .collect()
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// "Press ⌘S", with the keys that run `action` in the editor now, or "Save" when none does.
fn press(action: &dyn Action, window: &Window) -> String {
    match crate::keymap::keys_for(action, "FileEditor", window) {
        keys if keys.is_empty() => "Save".into(),
        keys => format!("Press {keys}"),
    }
}

fn error(text: String) -> Notice {
    Notice { text, error: true }
}

impl Focusable for FileEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for FileEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.state.clone().map(|state| {
            div().size_full().child(
                Editor::new(&state)
                    .size_full()
                    .bordered(false)
                    .readonly(self.read_only),
            )
        });
        let mode = if self.previewable() {
            self.mode
        } else {
            ViewMode::Code
        };
        let body: AnyElement = match (editor, mode) {
            (None, _) => div().into_any_element(),
            (Some(editor), ViewMode::Code) => editor.into_any_element(),
            (Some(_), ViewMode::Preview) => self.render_preview(cx),
            (Some(editor), ViewMode::Split) => h_flex()
                .size_full()
                .child(div().flex_1().min_w_0().h_full().child(editor))
                .child(div().w(px(1.)).h_full().bg(cx.theme().border))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.render_preview(cx)),
                )
                .into_any_element(),
        };
        v_flex()
            .key_context("FileEditor")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::format))
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_toolbar(cx))
            .when_some(self.notice.clone(), |el, n| {
                el.child(
                    div()
                        .px_3()
                        .py_1p5()
                        .text_xs()
                        .whitespace_normal()
                        .text_color(if n.error {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .when(n.error, |el| {
                            el.font_family(cx.theme().mono_font_family.clone())
                        })
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(n.text),
                )
            })
            .when(self.loading, |el| {
                el.child(
                    div()
                        .p_3()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Opening…"),
                )
            })
            .child(div().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
    use super::{breadcrumb, resolve_image};
    use gpui_kit::{ImageSource, Resource, SharedUri};
    use std::path::Path;

    /// A file moved away and put back where it was, as the Files tab's ⌫ and ⌘Z do, is no
    /// longer said to be deleted. Its tab said "This file was deleted from disk" until the next
    /// save: the file came back with the time it was loaded with, so nothing looked at it again,
    /// and the notice asked for a save that wasn't needed.
    #[gpui_kit::test]
    fn a_file_put_back_where_it_was_is_no_longer_said_to_be_deleted(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::AppContext as _;

        // The file is read and looked at on the runtime's real threads.
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
        });
        let dir = crate::files::Scratch::new("editor-put-back");
        let (file, away) = (dir.0.join("notes.md"), dir.0.join("away.md"));
        std::fs::write(&file, "notes\n").unwrap();
        let path = file.clone();
        let (_, editor) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                cx.new(|cx| super::FileEditor::open(path, None, window, cx))
            })
            .unwrap()
        });
        let notice = |cx: &mut gpui_kit::TestAppContext| {
            cx.update(|cx| editor.read(cx).notice.as_ref().map(|n| n.text.clone()))
        };
        let wait =
            |cx: &mut gpui_kit::TestAppContext, what: &str, ok: &dyn Fn(Option<String>) -> bool| {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
                loop {
                    cx.run_until_parked();
                    if ok(notice(cx)) {
                        return;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "timed out waiting until {what}"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    cx.executor().advance_clock(super::DISK_POLL);
                }
            };
        let loaded =
            |cx: &mut gpui_kit::TestAppContext| cx.update(|cx| editor.read(cx).state.is_some());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !loaded(cx) {
            assert!(
                std::time::Instant::now() < deadline,
                "the file never loaded"
            );
            cx.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        std::fs::rename(&file, &away).unwrap();
        wait(cx, "the tab says the file was deleted", &|n| {
            n.is_some_and(|n| n.contains("deleted"))
        });
        std::fs::rename(&away, &file).unwrap();
        wait(cx, "the notice is gone", &|n| n.is_none());
    }

    #[test]
    fn breadcrumb_is_relative_to_the_project() {
        let root = Path::new("/w/app");
        assert_eq!(
            breadcrumb(Path::new("/w/app/src/a.ts"), Some(root)),
            vec!["src", "a.ts"]
        );
        // Outside the project: the full path, without the leading "/".
        assert_eq!(
            breadcrumb(Path::new("/etc/hosts"), Some(root)),
            vec!["etc", "hosts"]
        );
    }

    fn path_of(source: ImageSource) -> Option<String> {
        match source {
            ImageSource::Resource(Resource::Path(p)) => Some(p.display().to_string()),
            _ => None,
        }
    }

    #[test]
    fn preview_images_resolve_relative_to_the_file() {
        let dir = Path::new("/w/app/docs");
        assert_eq!(
            path_of(resolve_image(dir, &SharedUri::from("img/a.png"))).as_deref(),
            Some("/w/app/docs/img/a.png")
        );
        assert_eq!(
            path_of(resolve_image(dir, &SharedUri::from("/abs/b.png"))).as_deref(),
            Some("/abs/b.png")
        );
        assert_eq!(
            path_of(resolve_image(dir, &SharedUri::from("https://x.dev/c.png"))),
            None
        );
    }
}
