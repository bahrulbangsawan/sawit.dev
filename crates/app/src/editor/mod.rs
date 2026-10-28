//! A text file open in a center tab: code editor with highlighting, soft
//! wrap, dirty tracking, Cmd-S to save, Shift-Cmd-I to format, and a live
//! preview (side by side or alone) for Markdown, HTML and SVG.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Editor, EditorState, InputEvent, Position, TabSize};
use gpui_kit::component::text::TextView;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::files::{
    FileKind, TextFile, classify, language_for, language_label, read_text, write_text_atomic,
};
use pgdesk_core::format::{format_text, tool_for};

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

impl EventEmitter<FileEditorEvent> for FileEditor {}

/// How a previewable file (Markdown, HTML, SVG) is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Code,
    Preview,
    Split,
}

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
    /// Set from the tab menu ("Make Tab Read-Only"): no edits, no saves.
    pub read_only: bool,
    notice: Option<Notice>,
    loading: bool,
    formatting: bool,
    mode: ViewMode,
    /// Text the preview renders (lags typing by `PREVIEW_DEBOUNCE`).
    preview: SharedString,
    preview_scheduled: bool,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl FileEditor {
    pub fn open(
        path: PathBuf,
        root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let read_path = path.clone();
        let rx = rt::spawn(cx, async move { read_text(&read_path) });
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                match result {
                    Ok(Ok(file)) => this.loaded(file, window, cx),
                    Ok(Err(e)) => this.notice = Some(error(format!("Gagal membuka: {e}"))),
                    Err(_) => {}
                }
                this.loading = false;
                cx.notify();
            });
        })
        .detach();
        let kind = classify(&path);
        Self {
            language: language_for(&path),
            mode: match kind {
                // Markdown reads best side by side; HTML/SVG start as code.
                FileKind::Markdown => ViewMode::Split,
                _ => ViewMode::Code,
            },
            kind,
            path,
            root,
            state: None,
            saved_text: String::new(),
            dirty: false,
            read_only: false,
            notice: None,
            loading: true,
            formatting: false,
            preview: SharedString::default(),
            preview_scheduled: false,
            focus: cx.focus_handle(),
            _subscriptions: Vec::new(),
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
            FileKind::Markdown | FileKind::Html | FileKind::Svg
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
                .indent_guides(true)
                .tab_size(TabSize {
                    tab_size: 2,
                    hard_tabs: false,
                })
                // Long lines wrap: no horizontal scrolling.
                .soft_wrap(true)
                .default_value(text.clone());
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
                    this.schedule_preview(cx);
                    cx.notify();
                }
            }),
            // Cursor moves re-render the "Ln, Col" in the toolbar.
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];
        self.preview = text.clone().into();
        self.saved_text = text;
        state.update(cx, |s, cx| s.focus(window, cx));
        self.state = Some(state);
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
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        cx.emit(FileEditorEvent::Changed);
        cx.notify();
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        self.save_task(cx).detach();
    }

    /// Writes the buffer; resolves to whether the file is now saved.
    pub fn save_task(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        let Some(state) = &self.state else {
            return Task::ready(false);
        };
        if self.read_only {
            return Task::ready(!self.dirty);
        }
        let text = state.read(cx).value().to_string();
        let path = self.path.clone();
        let to_write = text.clone();
        let rx = rt::spawn(cx, async move { write_text_atomic(&path, &to_write) });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            this.update(cx, |this, cx| {
                let saved = matches!(result, Ok(Ok(())));
                match result {
                    Ok(Ok(())) => {
                        this.saved_text = text;
                        this.dirty = false;
                        this.notice = None;
                    }
                    Ok(Err(e)) => this.notice = Some(error(format!("Gagal menyimpan: {e}"))),
                    Err(_) => {
                        this.notice = Some(error("Gagal menyimpan: runtime berhenti.".into()))
                    }
                }
                cx.emit(FileEditorEvent::Changed);
                cx.notify();
                saved
            })
            .unwrap_or(false)
        })
    }

    fn format(&mut self, _: &Format, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.state.clone() else {
            return;
        };
        if self.read_only || self.formatting {
            return;
        }
        let Some(tool) = tool_for(&self.path) else {
            self.notice = Some(error("Belum ada formatter untuk jenis file ini.".into()));
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
                        this.notice = Some(Notice {
                            text: if unchanged {
                                format!("Sudah rapi ({}).", tool.name())
                            } else {
                                format!("Diformat dengan {}.", tool.name())
                            },
                            error: false,
                        });
                        // The success note is transient.
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(Duration::from_secs(3)).await;
                            let _ = this.update(cx, |this, cx| {
                                if this.notice.as_ref().is_some_and(|n| !n.error) {
                                    this.notice = None;
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                    }
                    Ok(Err(e)) => this.notice = Some(error(e.to_string())),
                    Err(_) => this.notice = Some(error("Formatter berhenti.".into())),
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
        let mode_button = |id: &'static str, label: &'static str, mode: ViewMode, current| {
            Button::new(id)
                .ghost()
                .xsmall()
                .label(label)
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
                            mode_button("mode-code", "Kode", ViewMode::Code, current).on_click(
                                cx.listener(|this, _, _, cx| this.set_mode(ViewMode::Code, cx)),
                            ),
                        )
                        .child(
                            mode_button("mode-split", "Berdampingan", ViewMode::Split, current)
                                .on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.set_mode(ViewMode::Split, cx)
                                    }),
                                ),
                        )
                        .child(
                            mode_button("mode-preview", "Pratinjau", ViewMode::Preview, current)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_mode(ViewMode::Preview, cx)
                                })),
                        ),
                )
            })
            .when_some(cursor, |el, pos| {
                el.child(format!("Ln {}, Col {}", pos.line + 1, pos.character + 1))
            })
            .child(language_label(&self.language))
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
                            .tooltip("Format (⇧⌘I)")
                            .loading(self.formatting)
                            .disabled(self.read_only)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.format(&Format, window, cx)),
                            ),
                    )
                },
            )
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let id = ElementId::Name(format!("preview\u{0}{}", self.path.display()).into());
        let dir = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let body = match self.kind {
            FileKind::Svg => {
                let image = Arc::new(Image::from_bytes(
                    ImageFormat::Svg,
                    self.preview.as_bytes().to_vec(),
                ));
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
            FileKind::Html => TextView::html(id, self.preview.clone()),
            _ => TextView::markdown(id, self.preview.clone()),
        };
        div()
            .size_full()
            .bg(cx.theme().background)
            .child(
                body.scrollable(true)
                    .selectable(true)
                    .image_source(move |uri| resolve_image(&dir, uri))
                    .on_link_click(|url, _, _, cx| {
                        if url.starts_with("http://") || url.starts_with("https://") {
                            cx.open_url(url);
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
                        .child("Membuka…"),
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
