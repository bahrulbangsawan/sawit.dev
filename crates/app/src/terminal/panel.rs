//! Tabs of terminals in the bottom dock, with quick launch for herdr.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::probe::find_in_path;
use pgdesk_core::terminal::{Command, GridSize, Session};

use super::view::{TerminalEvent, TerminalView};

pub struct TerminalPanel {
    tabs: Vec<Entity<TerminalView>>,
    active: usize,
    cwd: Option<PathBuf>,
    error: Option<String>,
}

impl TerminalPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut panel = Self {
            tabs: vec![],
            active: 0,
            cwd: None,
            error: None,
        };
        panel.open(Command::default(), "zsh", None, cx);
        panel
    }

    /// Directory new terminals start in (the open project, if any).
    pub fn set_cwd(&mut self, cwd: Option<PathBuf>) {
        self.cwd = cwd;
    }

    pub fn herdr_available() -> bool {
        find_in_path("herdr").is_some()
    }

    pub fn open_herdr(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let cmd = Command {
            program: "herdr".into(),
            ..Command::default()
        };
        self.open(cmd, "herdr", window, cx);
    }

    pub fn open_shell(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let shell = std::env::var("SHELL").ok();
        let name = shell
            .as_deref()
            .and_then(|s| s.rsplit('/').next())
            .unwrap_or("shell")
            .to_string();
        self.open(Command::default(), &name, window, cx);
    }

    fn open(
        &mut self,
        mut cmd: Command,
        title: &str,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        cmd.cwd = self
            .cwd
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        let size = GridSize {
            cols: 80,
            rows: 24,
            cell_width: 8,
            cell_height: 17,
        };
        match Session::spawn(cmd, size) {
            Ok((session, events)) => {
                let title = title.to_string();
                let view = cx.new(|cx| TerminalView::new(session, events, title, cx));
                cx.subscribe(&view, |_, _, _: &TerminalEvent, cx| cx.notify())
                    .detach();
                self.tabs.push(view.clone());
                self.active = self.tabs.len() - 1;
                self.error = None;
                if let Some(window) = window {
                    let handle = view.focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            Err(e) => self.error = Some(format!("Failed to open terminal: {e}")),
        }
        cx.notify();
    }

    fn close(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.tabs.remove(ix);
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
            cx.notify();
        }
    }

    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        if let Some(view) = self.tabs.get(self.active) {
            let handle = view.focus_handle(cx);
            window.focus(&handle, cx);
        }
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, view)| {
                let v = view.read(cx);
                let active = ix == self.active;
                let label = if v.exited {
                    format!("{} (exited)", v.title)
                } else {
                    v.title.to_string()
                };
                h_flex()
                    .id(("term-tab", ix))
                    .h_full()
                    .px_2()
                    .gap_1()
                    .text_xs()
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .when(active, |el| {
                        el.bg(cx.theme().tab_active)
                            .text_color(cx.theme().tab_active_foreground)
                    })
                    .when(!active, |el| el.text_color(cx.theme().tab_foreground))
                    .child(Icon::new(Lucide::Terminal).xsmall())
                    .child(label)
                    .child(
                        Button::new(("close-term", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .on_click(cx.listener(move |this, _, _, cx| this.close(ix, cx))),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.active = ix;
                        this.focus_active(window, cx);
                        cx.notify();
                    }))
            })
            .collect();
        let herdr = Self::herdr_available();

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .h(px(28.))
                    .bg(cx.theme().tab_bar)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .h_full()
                            .flex_1()
                            .overflow_x_hidden()
                            .children(tabs),
                    )
                    .child(
                        h_flex()
                            .px_1()
                            .gap_1()
                            .when(herdr, |el| {
                                el.child(
                                    Button::new("open-herdr")
                                        .ghost()
                                        .xsmall()
                                        .label("herdr")
                                        .tooltip("Open herdr in a New Tab")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_herdr(Some(window), cx)
                                        })),
                                )
                            })
                            .child(
                                Button::new("new-terminal")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Plus)
                                    .tooltip("New Terminal")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_shell(Some(window), cx)
                                    })),
                            ),
                    ),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(div().p_2().text_sm().text_color(cx.theme().danger).child(e))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when_some(self.tabs.get(self.active).cloned(), |el, view| {
                        el.child(view)
                    }),
            )
    }
}
