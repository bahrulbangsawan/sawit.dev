//! A text file open in a center tab: code editor with highlighting, soft
//! wrap, dirty tracking, Cmd-S to save, Shift-Cmd-I to format, and a live
//! preview (side by side or alone) for Markdown, HTML, SVG and Mermaid.

mod diagrams;
mod disk;
pub mod kept;
mod render;
mod save;
pub(crate) use diagrams::palette;
#[cfg(test)]
pub(crate) mod testing;

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gpui_kit::component::input::{EditorState, InputEvent, Position, TabSize};
use gpui_kit::*;
use sawit_core::files::{FileKind, TextFile, classify, language_for, read_text};

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
    /// Why the file can't be edited here, as its load found (binary, too large, unreadable): its
    /// note, before [`Self::cannot_edit`] adds to it.
    unreadable: Option<Notice>,
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
                    Ok(Err(e)) => this.cannot_edit(error(format!("Unable to open this file. {e}"))),
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
            unreadable: None,
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
            self.cannot_edit(Notice {
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
                    // Compared in place: the text as a string would copy the whole file on each
                    // keystroke.
                    let dirty = *state.read(cx).text() != this.saved_text;
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

    /// Notes why the file can't be edited here (`why`, as its load found). Changes kept for it
    /// when Sawit last quit can't be put back then, and stay where they were kept for the next
    /// launch to try again ([`Self::restore`]): the note says where they are, as a warning. They
    /// used to stay there unseen, the tab opening again at every launch (site M3).
    fn cannot_edit(&mut self, why: Notice) {
        self.notice = Some(match &self.restoring {
            Some((_, from)) => {
                let end = if why.text.ends_with('.') { " " } else { ". " };
                error(format!(
                    "{}{end}Sawit kept unsaved changes to this file when it last quit, and can't put them back here. They're in {}.",
                    why.text,
                    from.display()
                ))
            }
            None => why.clone(),
        });
        self.unreadable = Some(why);
    }

    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        cx.emit(FileEditorEvent::Changed);
        cx.notify();
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

#[cfg(test)]
mod tests;
