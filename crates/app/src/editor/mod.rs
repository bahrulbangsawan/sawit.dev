//! A text file open in a center tab: code editor with highlighting, dirty
//! tracking and Cmd-S to save.

use std::path::PathBuf;

use gpui_kit::component::input::{Editor, EditorState, InputEvent, TabSize};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::files::{TextFile, language_for, read_text, write_text_atomic};

use crate::rt;

actions!(file_editor, [Save]);

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-s", Save, Some("FileEditor"))]);
}

pub enum FileEditorEvent {
    Changed,
}

impl EventEmitter<FileEditorEvent> for FileEditor {}

pub struct FileEditor {
    pub path: PathBuf,
    state: Option<Entity<EditorState>>,
    saved_text: String,
    pub dirty: bool,
    notice: Option<String>,
    loading: bool,
    focus: FocusHandle,
    _subscription: Option<Subscription>,
}

impl FileEditor {
    pub fn open(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let read_path = path.clone();
        let rx = rt::spawn(cx, async move { read_text(&read_path) });
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                match result {
                    Ok(Ok(file)) => this.loaded(file, window, cx),
                    Ok(Err(e)) => this.notice = Some(format!("Gagal membuka: {e}")),
                    Err(_) => {}
                }
                this.loading = false;
                cx.notify();
            });
        })
        .detach();
        Self {
            path,
            state: None,
            saved_text: String::new(),
            dirty: false,
            notice: None,
            loading: true,
            focus: cx.focus_handle(),
            _subscription: None,
        }
    }

    pub fn title(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    fn loaded(&mut self, file: TextFile, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(reason) = file.read_only_reason {
            self.notice = Some(reason);
            return;
        }
        let language = language_for(&self.path);
        let text = file.text;
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .line_number(true)
                .tab_size(TabSize {
                    tab_size: 2,
                    hard_tabs: false,
                })
                .soft_wrap(false)
                .default_value(text.clone())
        });
        self._subscription = Some(cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let dirty = state.read(cx).value().as_ref() != this.saved_text;
                if dirty != this.dirty {
                    this.dirty = dirty;
                    cx.emit(FileEditorEvent::Changed);
                    cx.notify();
                }
            }
        }));
        self.saved_text = text;
        state.update(cx, |s, cx| s.focus(window, cx));
        self.state = Some(state);
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = &self.state else { return };
        let text = state.read(cx).value().to_string();
        let path = self.path.clone();
        let to_write = text.clone();
        let rx = rt::spawn(cx, async move { write_text_atomic(&path, &to_write) });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(())) => {
                        this.saved_text = text;
                        this.dirty = false;
                        this.notice = None;
                    }
                    Ok(Err(e)) => this.notice = Some(format!("Gagal menyimpan: {e}")),
                    Err(_) => {}
                }
                cx.emit(FileEditorEvent::Changed);
                cx.notify();
            });
        })
        .detach();
    }
}

impl Focusable for FileEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for FileEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("FileEditor")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::save))
            .size_full()
            .bg(cx.theme().background)
            .when_some(self.notice.clone(), |el, n| {
                el.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(n),
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
            .when_some(self.state.clone(), |el, state| {
                el.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(Editor::new(&state).size_full().bordered(false)),
                )
            })
    }
}
