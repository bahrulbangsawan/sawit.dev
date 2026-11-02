//! One server of the SSH sidebar: whether it answers, its address and open sessions, Edit and
//! Connect, and its right-click menu.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::ssh::Probe;

use super::{SshEvent, SshPanel};
use crate::ssh::store::ProbeState;

impl SshPanel {
    pub(super) fn render_host(
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
