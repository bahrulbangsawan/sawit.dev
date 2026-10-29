//! Shell Config sidebar: the shell startup files found on this machine
//! (`~/.zshrc`, `/etc/profile`, …); a click opens one in the editor.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::recent::tildify;
use sawit_core::shell_config::{self, Scope, ShellConfig};

use crate::states::Placeholder;

pub enum ShellConfigEvent {
    OpenFile(PathBuf),
}

impl EventEmitter<ShellConfigEvent> for ShellConfigPanel {}

pub struct ShellConfigPanel {
    configs: Vec<ShellConfig>,
    login_shell: Option<String>,
}

impl ShellConfigPanel {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            configs: shell_config::detect(),
            login_shell: shell_config::login_shell(),
        }
    }

    /// Looks again: files may have been created or removed since.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.configs = shell_config::detect();
        cx.notify();
    }

    fn render_section(&self, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .h(px(26.))
            .mt_1()
            .px_2()
            .flex()
            .items_center()
            .text_xs()
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(title)
            .into_any_element()
    }

    fn render_config(&self, ix: usize, config: &ShellConfig, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let name = tildify(&config.path, home.as_deref());
        let mine = self.login_shell.as_deref() == Some(config.shell);
        let path = config.path.clone();
        h_flex()
            .id(("shell-config", ix))
            .w_full()
            .h(px(42.))
            .px_2()
            .gap_2()
            .cursor_pointer()
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(ShellConfigEvent::OpenFile(path.clone()))
            }))
            .child(
                Icon::new(Lucide::FileTerminal)
                    .small()
                    .text_color(muted),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .child(config.note),
                    ),
            )
            .when(mine, |el| {
                el.child(
                    div()
                        .flex_none()
                        .px_1p5()
                        .rounded_full()
                        .text_xs()
                        .bg(cx.theme().success.opacity(0.15))
                        .text_color(cx.theme().success)
                        .child(config.shell),
                )
            })
            .when(!config.writable, |el| {
                el.child(
                    div()
                        .id(("shell-config-lock", ix))
                        .flex_none()
                        .child(Icon::new(Lucide::Lock).xsmall().text_color(muted))
                        .tooltip(|window, cx| {
                            Tooltip::new("Saving asks for your admin password").build(window, cx)
                        }),
                )
            })
            .into_any_element()
    }
}

impl Render for ShellConfigPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut items = Vec::new();
        for (scope, title) in [(Scope::User, "YOUR PROFILE"), (Scope::System, "SYSTEM-WIDE")] {
            let configs: Vec<_> = self
                .configs
                .iter()
                .enumerate()
                .filter(|(_, c)| c.scope == scope)
                .collect();
            if configs.is_empty() {
                continue;
            }
            items.push(self.render_section(title, cx));
            for (ix, config) in configs {
                items.push(self.render_config(ix, config, cx));
            }
        }
        let empty = items.is_empty();
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(32.))
                    .px_2()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Shell Config · {}", self.configs.len())),
                    )
                    .child(
                        Button::new("shell-config-refresh")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Look Again")
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(
                div()
                    .id("shell-config-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(!empty, |el| el.children(items))
                    .when(empty, |el| {
                        el.child(
                            Placeholder::new(Lucide::FileTerminal, "No shell config files")
                                .compact()
                                .detail("Files like ~/.zshrc, ~/.bashrc and /etc/profile show up here once they exist."),
                        )
                    }),
            )
    }
}
