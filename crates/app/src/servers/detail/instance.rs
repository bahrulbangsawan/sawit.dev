//! The selected instance: its status and controls, the port-conflict warning, its DATABASE_URL
//! (which can be written to a project's .env) and its live log.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::control::Preflight;
use sawit_core::instances::Instance;
use sawit_core::pgctl::PgCtlStatus;
use sawit_core::ports::PortVerdict;
use sawit_core::probe::PortReport;

use super::{ServerDetail, error_box};
use crate::servers::{Status, status_color};

/// The DATABASE_URL a .env file sets, if it sets one.
fn current_url(env: &str) -> Option<String> {
    env.lines().find_map(|line| {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        let value = line.strip_prefix("DATABASE_URL=")?;
        Some(value.trim().trim_matches(['"', '\'']).to_string())
    })
}

impl ServerDetail {
    /// Writes this instance's DATABASE_URL into a chosen project's .env.
    /// Asks first when the .env already has a DATABASE_URL other than this one: writing it
    /// replaces that line.
    fn write_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(inst) = self.store.read(cx).selected_instance().cloned() else {
            return;
        };
        let url = format!("postgres://postgres@localhost:{}/postgres", inst.port);
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose Project Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let old = sawit_core::db::read_dotenv(&dir)
                .ok()
                .and_then(|text| current_url(&text))
                .filter(|old| *old != url);
            if let Some(old) = old {
                let Ok(answer) = this.update_in(cx, |_, window, cx| {
                    crate::confirm::Confirm::new(
                        "Replace DATABASE_URL in .env?",
                        "Replace DATABASE_URL",
                    )
                    .detail(format!(
                        "{} has DATABASE_URL={old}. It becomes {url}; the rest of the file stays.",
                        dir.join(".env").display()
                    ))
                    .ask(window, cx)
                }) else {
                    return;
                };
                if !answer.await {
                    return;
                }
            }
            let Ok(rx) = this.update(cx, |_, cx| {
                crate::rt::spawn(
                    cx,
                    async move { sawit_core::db::write_database_url(&dir, &url) },
                )
            }) else {
                return;
            };
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                let notice = match result {
                    Ok(path) => format!("Wrote DATABASE_URL to {}.", path.display()),
                    Err(e) => format!("Unable to write .env. {e}"),
                };
                this.store.update(cx, |s, cx| {
                    s.notice = Some(notice);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub(super) fn render_instance(
        &mut self,
        inst: &Instance,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let store = self.store.read(cx);
        let status = store.status(&inst.name).clone();
        let busy = store.busy.contains(&inst.name);
        let running = matches!(status, Status::Known(PgCtlStatus::Running { .. }));
        let version = store.version_of(inst).unwrap_or_else(|| "?".into());
        let blocked = store.blocked.get(&inst.name).cloned();
        let error = store.errors.get(&inst.name).cloned();
        let notice = store.notice.clone();
        let log_len = store.log.len();
        let name = inst.name.clone();
        let status_text = match &status {
            Status::Unknown => "checking…".to_string(),
            Status::Known(PgCtlStatus::Running { pid }) => format!("running · PID {pid}"),
            Status::Known(PgCtlStatus::Stopped) => "stopped".into(),
            Status::Known(PgCtlStatus::NotACluster) => "not a cluster".into(),
            Status::Known(PgCtlStatus::MissingDir) => "data directory missing".into(),
            Status::Error(e) => format!("error: {e}"),
        };
        let url = format!("postgres://postgres@localhost:{}/postgres", inst.port);

        let n1 = name.clone();
        let n2 = name.clone();
        let n3 = name.clone();
        let n4 = name.clone();
        let header = h_flex()
            .px_4()
            .py_3()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .size(px(9.))
                    .rounded_full()
                    .bg(status_color(&status, cx)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().text_base().font_semibold().child(inst.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "PostgreSQL {version} · port {} · {status_text}",
                                        inst.port
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(inst.data_dir.display().to_string()),
                    ),
            )
            .child(
                Button::new("start")
                    .small()
                    .primary()
                    .icon(IconName::Play)
                    .label("Start")
                    .loading(busy && !running)
                    .disabled(busy || running)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.store
                            .update(cx, |s, cx| s.start(&n1, Preflight::Check, cx))
                    })),
            )
            .child(
                Button::new("stop")
                    .small()
                    .icon(IconName::Square)
                    .label("Stop")
                    .disabled(busy || !running)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let answer = crate::confirm::Confirm::new(
                            format!("Stop the instance {n2}?"),
                            "Stop Instance",
                        )
                        .thing(n2.clone())
                        .detail(
                            "Apps and terminals connected to it lose their connection. Its data \
                             stays, and Start brings it back.",
                        )
                        .ask(window, cx);
                        let (store, name) = (this.store.clone(), n2.clone());
                        cx.spawn(async move |_, cx| {
                            if answer.await {
                                store.update(cx, |s, cx| s.stop(&name, cx));
                            }
                        })
                        .detach();
                    })),
            )
            .child(
                Button::new("restart")
                    .small()
                    .icon(IconName::RotateCw)
                    .label("Restart")
                    .disabled(busy || !running)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.store.update(cx, |s, cx| s.restart(&n3, cx))
                    })),
            )
            .child(
                Button::new("write-env")
                    .small()
                    .ghost()
                    .label(".env")
                    .tooltip("Write DATABASE_URL to the Project's .env File")
                    .on_click(cx.listener(move |this, _, window, cx| this.write_env(window, cx))),
            )
            .child(
                Button::new("forget")
                    .small()
                    .ghost()
                    .icon(Lucide::X)
                    .tooltip("Remove from List (keeps the data directory)")
                    .disabled(busy || running)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let answer = crate::confirm::Confirm::new(
                            format!("Remove {n4} from the list?"),
                            "Remove Instance",
                        )
                        .thing(n4.clone())
                        .detail(
                            "Sawit stops showing it. Its data directory stays on disk, so its \
                             databases aren't deleted.",
                        )
                        .ask(window, cx);
                        let (store, name) = (this.store.clone(), n4.clone());
                        cx.spawn(async move |_, cx| {
                            if answer.await {
                                store.update(cx, |s, cx| s.forget(&name, cx));
                            }
                        })
                        .detach();
                    })),
            );

        let log_list = {
            let store = self.store.clone();
            uniform_list("server-log", log_len, move |range, _, cx| {
                let log = &store.read(cx).log;
                range
                    .map(|ix| {
                        div()
                            .px_4()
                            .whitespace_nowrap()
                            .child(log.get(ix).cloned().unwrap_or_default())
                    })
                    .collect()
            })
            .track_scroll(&self.log_scroll)
            .size_full()
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
        };

        v_flex()
            .size_full()
            .child(header)
            .when_some(blocked, |el, report| {
                el.child(self.render_blocked(&name, report, cx))
            })
            .when_some(error, |el, e| {
                el.child(div().px_4().py_3().child(error_box(e, cx)))
            })
            .when_some(notice, |el, n| {
                el.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(n),
                )
            })
            .child(
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child("DATABASE_URL")
                    .child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(cx.theme().foreground)
                            .child(url),
                    ),
            )
            .child(
                h_flex()
                    .px_4()
                    .py_1()
                    .justify_between()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child("LOG")
                    .child(self.copy_buttons(log_len == 0, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .bg(cx.theme().background)
                    .child(log_list),
            )
    }

    fn render_blocked(
        &self,
        name: &str,
        report: PortReport,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let conflict = matches!(report.verdict, PortVerdict::Conflict { .. });
        let owners: Vec<_> = report
            .owners
            .iter()
            .map(|o| {
                div()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(format!(
                        "{}:{}  {}  — {}",
                        o.listener.addr,
                        o.listener.port,
                        o.describe(),
                        o.command_line
                    ))
            })
            .collect();
        let n1 = name.to_string();
        let n2 = name.to_string();
        v_flex()
            .mx_4()
            .my_3()
            .p_3()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(if conflict {
                cx.theme().danger
            } else {
                cx.theme().warning
            })
            .bg(cx.theme().muted)
            .child(
                h_flex()
                    .gap_2()
                    .child(Icon::new(IconName::TriangleAlert).text_color(if conflict {
                        cx.theme().danger
                    } else {
                        cx.theme().warning
                    }))
                    .child(div().font_semibold().child(if conflict {
                        format!(
                            "Port {} is already in use, so the instance didn't start",
                            report.port
                        )
                    } else {
                        format!(
                            "Another process also listens on port {}. Start anyway?",
                            report.port
                        )
                    })),
            )
            .child(div().text_sm().whitespace_normal().child(report.explain()))
            .children(owners)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("dismiss-blocked")
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.store.update(cx, |s, cx| s.dismiss_blocked(&n1, cx))
                            })),
                    )
                    .when(!conflict, |el| {
                        el.child(
                            Button::new("force-start")
                                .small()
                                .warning()
                                .label("Start Anyway")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.store
                                        .update(cx, |s, cx| s.start(&n2, Preflight::Force, cx))
                                })),
                        )
                    }),
            )
    }
}
