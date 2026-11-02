//! Sidebar list of servers: saved ones by group, then the hosts in
//! `~/.ssh/config`. Each shows whether it answers and how many sessions are
//! open; a click connects (or shows the open session).

mod host;

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::ssh::user_config_path;

use super::store::{SshStore, alias_key, saved_key};
use crate::states::{Banner, Placeholder};
use crate::terminal::TerminalPanel;

pub enum SshEvent {
    /// Open a session to the server with this key; `new` even if one is open.
    Connect {
        key: String,
        new: bool,
    },
    /// Edit the saved server with this id in a tab.
    Edit(String),
    OpenFile(PathBuf),
}

impl EventEmitter<SshEvent> for SshPanel {}

pub struct SshPanel {
    store: Entity<SshStore>,
    terminal: Entity<TerminalPanel>,
    search: Entity<InputState>,
    copied: Option<String>,
    _subscriptions: Vec<Subscription>,
}

/// One line of the list.
enum Row {
    Section(String),
    Saved(String),
    Alias(String),
}

impl SshPanel {
    pub fn new(
        store: Entity<SshStore>,
        terminal: Entity<TerminalPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search servers"));
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.observe(&terminal, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        Self {
            store,
            terminal,
            search,
            copied: None,
            _subscriptions: subscriptions,
        }
    }

    fn rows(&self, cx: &App) -> Vec<Row> {
        let query = self.search.read(cx).value().to_string();
        let store = self.store.read(cx);
        let mut rows = Vec::new();
        let mut group: Option<String> = None;
        let saved = store.list.search(&query);
        let any_group = saved.iter().any(|h| !h.group.trim().is_empty());
        for h in saved {
            let g = h.group.trim().to_string();
            if any_group && group.as_ref() != Some(&g) {
                rows.push(Row::Section(if g.is_empty() {
                    "Servers".into()
                } else {
                    g.clone()
                }));
                group = Some(g);
            }
            rows.push(Row::Saved(h.id.clone()));
        }
        let aliases: Vec<_> = store.config.iter().filter(|h| h.matches(&query)).collect();
        if !aliases.is_empty() {
            rows.push(Row::Section("~/.ssh/config".into()));
            rows.extend(aliases.into_iter().map(|h| Row::Alias(h.alias.clone())));
        }
        rows
    }

    fn new_server(&mut self, cx: &mut Context<Self>) {
        let id = self.store.update(cx, |s, cx| s.create(cx));
        cx.emit(SshEvent::Edit(id));
    }

    fn copy_command(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(text) = self.store.read(cx).command_line(key) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = Some(key.to_string());
        let key = key.to_string();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.copied.as_deref() == Some(key.as_str()) {
                    this.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn render_section(&self, ix: usize, title: &str, cx: &mut Context<Self>) -> AnyElement {
        let config = title == "~/.ssh/config";
        h_flex()
            .id(("ssh-section", ix))
            .w_full()
            .h(px(26.))
            .mt_1()
            .px_2()
            .gap_1()
            .text_xs()
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(div().flex_1().truncate().child(if config {
                title.to_string()
            } else {
                title.to_uppercase()
            }))
            .when(config, |el| {
                el.child(
                    Button::new("ssh-edit-config")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::FileCog)
                        .tooltip("Edit ~/.ssh/config")
                        .on_click(cx.listener(|_, _, _, cx| {
                            if let Some(path) = user_config_path() {
                                cx.emit(SshEvent::OpenFile(path));
                            }
                        })),
                )
            })
            .into_any_element()
    }
}

impl Render for SshPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows(cx);
        let store = self.store.read(cx);
        let total = store.list.hosts.len() + store.config.len();
        let query = self.search.read(cx).value().to_string();
        let message = store
            .load_error
            .clone()
            .map(|e| {
                (
                    format!("Unable to read ssh_hosts.json. Sawit won't save over it until the file is fixed or moved. {e}"),
                    true,
                )
            })
            .or_else(|| store.notice.clone().map(|n| (n, false)));
        let items: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .filter_map(|(ix, row)| match row {
                Row::Section(title) => Some(self.render_section(ix, title, cx)),
                Row::Saved(id) => {
                    let h = self.store.read(cx).list.get(id)?.clone();
                    Some(self.render_host(
                        ix,
                        saved_key(id),
                        h.label(),
                        if h.host.trim().is_empty() {
                            "No address yet".into()
                        } else {
                            h.address()
                        },
                        Some(id.clone()),
                        cx,
                    ))
                }
                Row::Alias(alias) => {
                    let address = self
                        .store
                        .read(cx)
                        .config
                        .iter()
                        .find(|h| &h.alias == alias)?
                        .address();
                    Some(self.render_host(ix, alias_key(alias), alias.clone(), address, None, cx))
                }
            })
            .collect();
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
                            .child(format!("SSH · {total}")),
                    )
                    .child(
                        Button::new("ssh-refresh")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Check All Servers")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.store.update(cx, |s, cx| s.refresh(cx))
                            })),
                    )
                    .child(
                        Button::new("ssh-new")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("New Server")
                            .on_click(cx.listener(|this, _, _, cx| this.new_server(cx))),
                    ),
            )
            .child(div().p_2().child(Input::new(&self.search).small().cleanable(true)))
            .when_some(message, |el, (text, error)| {
                el.child(if error {
                    Banner::error("ssh-message", text)
                } else {
                    Banner::info("ssh-message", text)
                })
            })
            .child(
                div()
                    .id("ssh-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(!empty, |el| el.children(items))
                    .when(empty, |el| {
                        el.child(if total == 0 {
                            Placeholder::new(Lucide::Server, "No servers yet")
                                .compact()
                                .detail("Add your VPS once, then connect in one click. Hosts in ~/.ssh/config show up here too.")
                                .action(
                                    Button::new("empty-new-server")
                                        .small()
                                        .primary()
                                        .icon(IconName::Plus)
                                        .label("New Server")
                                        .on_click(cx.listener(|this, _, _, cx| this.new_server(cx))),
                                )
                        } else {
                            Placeholder::new(IconName::Search, "No matching servers")
                                .compact()
                                .detail(format!("Nothing matches “{}”.", query.trim()))
                                .action(
                                    Button::new("clear-ssh-search")
                                        .small()
                                        .label("Clear Search")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.search.update(cx, |s, cx| s.set_value("", window, cx));
                                        })),
                                )
                        })
                    }),
            )
    }
}
