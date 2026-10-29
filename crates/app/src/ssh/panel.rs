//! Sidebar list of servers: saved ones by group, then the hosts in
//! `~/.ssh/config`. Each shows whether it answers and how many sessions are
//! open; a click connects (or shows the open session).

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::ssh::{Probe, user_config_path};

use super::store::{ProbeState, SshStore, alias_key, saved_key};
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

    fn render_host(
        &self,
        ix: usize,
        key: String,
        name: String,
        address: String,
        saved_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (muted, success, danger) = (theme.muted_foreground, theme.success, theme.danger);
        let probe = self.store.read(cx).probes.get(&key).cloned();
        let (dot, status): (Hsla, String) = match &probe {
            Some(ProbeState::Done(Probe::Up { latency_ms, banner })) => {
                (success, format!("Reachable in {latency_ms} ms · {banner}"))
            }
            Some(ProbeState::Done(Probe::Down(e))) => (danger, e.clone()),
            Some(ProbeState::Done(Probe::Indirect)) => (
                muted,
                "Reached through a jump host; not checked directly.".into(),
            ),
            Some(ProbeState::Checking) => (muted.opacity(0.5), "Checking…".into()),
            None => (muted.opacity(0.5), "Not checked yet.".into()),
        };
        let latency = match &probe {
            Some(ProbeState::Done(Probe::Up { latency_ms, .. })) => {
                Some(format!("{latency_ms} ms"))
            }
            _ => None,
        };
        let sessions = self.terminal.read(cx).ssh_sessions(&key, cx);
        let copied = self.copied.as_deref() == Some(key.as_str());
        let (k_click, k_new, k_menu) = (key.clone(), key.clone(), key.clone());
        let edit_id = saved_id.clone();
        let this = cx.entity().downgrade();
        h_flex()
            .id(("ssh-host", ix))
            .w_full()
            .h(px(42.))
            .px_2()
            .gap_2()
            .cursor_pointer()
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(SshEvent::Connect {
                    key: k_click.clone(),
                    new: false,
                })
            }))
            .child(
                div()
                    .id(("ssh-status", ix))
                    .flex_none()
                    .size(px(8.))
                    .rounded_full()
                    .bg(dot)
                    .tooltip(move |window, cx| Tooltip::new(status.clone()).build(window, cx)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(name))
                    .child(
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(muted)
                            .child(div().min_w_0().truncate().child(address))
                            .when_some(latency, |el, l| el.child("·").child(l)),
                    ),
            )
            .when(sessions > 0, |el| {
                el.child(
                    div()
                        .flex_none()
                        .px_1p5()
                        .rounded_full()
                        .text_xs()
                        .bg(cx.theme().success.opacity(0.15))
                        .text_color(cx.theme().success)
                        .child(sessions.to_string()),
                )
            })
            .when_some(edit_id, |el, id| {
                el.child(
                    Button::new(("ssh-edit", ix))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Pencil)
                        .tooltip("Edit Server")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            cx.emit(SshEvent::Edit(id.clone()));
                        })),
                )
            })
            .child(
                Button::new(("ssh-connect", ix))
                    .ghost()
                    .xsmall()
                    .icon(if sessions > 0 {
                        Lucide::Plus
                    } else {
                        Lucide::LogIn
                    })
                    .tooltip(if sessions > 0 {
                        "New Session"
                    } else {
                        "Connect"
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(SshEvent::Connect {
                            key: k_new.clone(),
                            new: true,
                        });
                    })),
            )
            .context_menu(move |menu, _, _| {
                host_menu(this.clone(), k_menu.clone(), saved_id.clone(), copied, menu)
            })
            .into_any_element()
    }
}

type MenuAction = dyn Fn(&mut SshPanel, &mut Context<SshPanel>);

fn host_menu(
    this: WeakEntity<SshPanel>,
    key: String,
    saved_id: Option<String>,
    copied: bool,
    menu: PopupMenu,
) -> PopupMenu {
    let act = |f: Box<MenuAction>| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |panel, cx| f(panel, cx));
        }
    };
    let (k1, k2, k3, k4) = (key.clone(), key.clone(), key.clone(), key.clone());
    let menu = menu
        .item(
            PopupMenuItem::new("Connect").on_click(act(Box::new(move |_, cx| {
                cx.emit(SshEvent::Connect {
                    key: k1.clone(),
                    new: false,
                })
            }))),
        )
        .item(
            PopupMenuItem::new("New Session").on_click(act(Box::new(move |_, cx| {
                cx.emit(SshEvent::Connect {
                    key: k2.clone(),
                    new: true,
                })
            }))),
        )
        .item(
            PopupMenuItem::new(if copied { "Copied" } else { "Copy SSH Command" })
                .on_click(act(Box::new(move |panel, cx| panel.copy_command(&k3, cx)))),
        )
        .item(
            PopupMenuItem::new("Check Connection").on_click(act(Box::new(move |panel, cx| {
                let key = k4.clone();
                panel.store.update(cx, |s, cx| s.probe(&key, cx));
            }))),
        )
        .separator();
    match saved_id {
        Some(id) => {
            let (e, d, r) = (id.clone(), id.clone(), id);
            menu.item(
                PopupMenuItem::new("Edit").on_click(act(Box::new(move |_, cx| {
                    cx.emit(SshEvent::Edit(e.clone()))
                }))),
            )
            .item(
                PopupMenuItem::new("Duplicate").on_click(act(Box::new(move |panel, cx| {
                    let id = d.clone();
                    if let Some(new) = panel.store.update(cx, |s, cx| s.duplicate(&id, cx)) {
                        cx.emit(SshEvent::Edit(new));
                    }
                }))),
            )
            .separator()
            .item(
                PopupMenuItem::new("Delete").on_click(act(Box::new(move |panel, cx| {
                    let id = r.clone();
                    panel.store.update(cx, |s, cx| s.remove(&id, cx));
                }))),
            )
        }
        None => {
            let alias = key
                .strip_prefix("ssh-config\u{0}")
                .unwrap_or_default()
                .to_string();
            menu.item(PopupMenuItem::new("Save as Server").on_click(act(Box::new(
                move |panel, cx| {
                    let alias = alias.clone();
                    let id = panel.store.update(cx, |s, cx| s.save_alias(&alias, cx));
                    cx.emit(SshEvent::Edit(id));
                },
            ))))
        }
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
