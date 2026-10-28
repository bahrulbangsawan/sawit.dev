//! Center pane for the selected instance: controls, port-conflict warning,
//! live log, and the "new instance" form.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::control::Preflight;
use pgdesk_core::instances::Instance;
use pgdesk_core::pgctl::PgCtlStatus;
use pgdesk_core::ports::PortVerdict;
use pgdesk_core::probe::PortReport;

use super::list::status_color;
use super::store::{BinaryChoice, ServerStore, Status};

/// Majors offered for download when creating an instance.
const DOWNLOADABLE: [u32; 3] = [18, 17, 16];

pub struct ServerDetail {
    store: Entity<ServerStore>,
    name_input: Entity<InputState>,
    port_input: Entity<InputState>,
    /// Index into `choices()`.
    choice: usize,
    form_error: Option<String>,
    form_busy: bool,
    log_scroll: UniformListScrollHandle,
    log_len: usize,
    was_creating: bool,
}

impl ServerDetail {
    pub fn new(store: Entity<ServerStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_in(&store, window, |this, store, window, cx| {
            let creating = store.read(cx).creating;
            if creating && !this.was_creating {
                this.reset_form(window, cx);
            }
            this.was_creating = creating;
            let len = store.read(cx).log.len();
            if len != this.log_len {
                this.log_len = len;
                if len > 0 {
                    this.log_scroll
                        .scroll_to_item(len - 1, ScrollStrategy::Bottom);
                }
            }
            cx.notify();
        })
        .detach();
        let mut this = Self {
            store,
            name_input: cx.new(|cx| InputState::new(window, cx).placeholder("project-name")),
            port_input: cx.new(|cx| InputState::new(window, cx).placeholder("5432")),
            choice: 0,
            form_error: None,
            form_busy: false,
            log_scroll: UniformListScrollHandle::new(),
            log_len: 0,
            was_creating: false,
        };
        if this.store.read(cx).registry.instances.is_empty() {
            this.reset_form(window, cx);
        }
        this
    }

    fn choices(&self, cx: &App) -> Vec<(String, BinaryChoice)> {
        let store = self.store.read(cx);
        let mut choices: Vec<(String, BinaryChoice)> = store
            .binaries
            .iter()
            .map(|b| {
                (
                    format!("PostgreSQL {} · {}", b.version, b.source),
                    BinaryChoice::Installed(b.clone()),
                )
            })
            .collect();
        for major in DOWNLOADABLE {
            let installed = store.binaries.iter().any(|b| {
                b.version.major == major && b.source == pgdesk_core::binaries::BinarySource::Managed
            });
            if !installed {
                choices.push((
                    format!("Download PostgreSQL {major} (no Homebrew/Docker)"),
                    BinaryChoice::Download { major },
                ));
            }
        }
        choices
    }

    fn reset_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form_error = None;
        self.form_busy = false;
        self.choice = 0;
        self.name_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        let rx = self.store.update(cx, |s, cx| s.suggest_port(cx));
        cx.spawn_in(window, async move |this, window| {
            if let Ok(port) = rx.await {
                let _ = this.update_in(window, |this, window, cx| {
                    this.port_input
                        .update(cx, |s, cx| s.set_value(port.to_string(), window, cx));
                });
            }
        })
        .detach();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).value().trim().to_string();
        let Ok(port) = self.port_input.read(cx).value().trim().parse::<u16>() else {
            self.form_error = Some("Port must be a number from 1 to 65535.".into());
            cx.notify();
            return;
        };
        let Some((_, choice)) = self.choices(cx).into_iter().nth(self.choice) else {
            self.form_error = Some("Choose a PostgreSQL binary.".into());
            cx.notify();
            return;
        };
        self.form_busy = true;
        self.form_error = None;
        cx.notify();
        let task = self
            .store
            .update(cx, |s, cx| s.create(name, port, choice, cx));
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.form_busy = false;
                this.form_error = result.err();
                cx.notify();
            });
        })
        .detach();
    }

    fn adopt(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add Data Directory".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let Ok(task) = this.update(cx, |this, cx| {
                this.store.update(cx, |s, cx| s.adopt(dir, cx))
            }) else {
                return;
            };
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.form_error = result.err();
                cx.notify();
            });
        })
        .detach();
    }

    /// Writes this instance's DATABASE_URL into a chosen project's .env.
    fn write_env(&mut self, cx: &mut Context<Self>) {
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
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let Ok(rx) = this.update(cx, |_, cx| {
                crate::rt::spawn(cx, async move {
                    pgdesk_core::db::write_database_url(&dir, &url)
                })
            }) else {
                return;
            };
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                let notice = match result {
                    Ok(path) => format!("Wrote DATABASE_URL to {}", path.display()),
                    Err(e) => format!("Failed to write .env: {e}"),
                };
                this.store.update(cx, |s, cx| {
                    s.notice = Some(notice);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn render_form(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let choices = self.choices(cx);
        let options: Vec<_> = choices
            .iter()
            .enumerate()
            .map(|(ix, (label, choice))| {
                let selected = ix == self.choice;
                let icon = match choice {
                    BinaryChoice::Installed(_) => Icon::new(IconName::HardDrive),
                    BinaryChoice::Download { .. } => Icon::new(Lucide::Download),
                };
                h_flex()
                    .id(("bin-choice", ix))
                    .px_2()
                    .py_1()
                    .gap_2()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(if selected {
                        cx.theme().ring
                    } else {
                        cx.theme().border
                    })
                    .when(selected, |el| el.bg(cx.theme().list_active))
                    .cursor_pointer()
                    .child(icon.small())
                    .child(div().text_sm().child(label.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choice = ix;
                        cx.notify();
                    }))
            })
            .collect();
        let downloading = self.form_busy
            && matches!(
                choices.get(self.choice),
                Some((_, BinaryChoice::Download { .. }))
            );

        let data_root = {
            let root = self.store.read(cx).paths.root.display().to_string();
            match std::env::var("HOME") {
                Ok(home) if root.starts_with(&home) => root.replacen(&home, "~", 1),
                _ => root,
            }
        };
        v_flex()
            .p_4()
            .gap_3()
            .max_w(px(560.))
            .child(div().text_lg().font_semibold().child("New PostgreSQL Instance"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "Created in {}, listens on localhost only, user `postgres` with no password.",
                        data_root
                    )),
            )
            .child(field("Name", Input::new(&self.name_input)))
            .child(field("Port", Input::new(&self.port_input)))
            .child(field("Binary", v_flex().gap_1().children(options)))
            .when_some(self.form_error.clone(), |el, e| el.child(error_box(e, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("create")
                            .primary()
                            .small()
                            .loading(self.form_busy)
                            .disabled(self.form_busy)
                            .label(if downloading { "Downloading & creating…" } else { "Create Instance" })
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    )
                    .child(
                        Button::new("adopt")
                            .ghost()
                            .small()
                            .icon(IconName::FolderOpen)
                            .label("Add existing data directory…")
                            .on_click(cx.listener(|this, _, _, cx| this.adopt(cx))),
                    ),
            )
    }

    fn render_instance(
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.store.update(cx, |s, cx| s.stop(&n2, cx))
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
                    .tooltip("Write DATABASE_URL to the Project's .env (e.g. TanStack Start)")
                    .on_click(cx.listener(move |this, _, _, cx| this.write_env(cx))),
            )
            .child(
                Button::new("forget")
                    .small()
                    .ghost()
                    .icon(Lucide::X)
                    .tooltip("Remove from List (keeps the data directory)")
                    .disabled(busy || running)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.store.update(cx, |s, cx| s.forget(&n4, cx))
                    })),
            );

        let log_list = {
            let store = self.store.clone();
            uniform_list("server-log", log_len, move |range, _, cx| {
                let log = &store.read(cx).log;
                range
                    .map(|ix| {
                        div()
                            .px_3()
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
            .when_some(error, |el, e| el.child(div().p_3().child(error_box(e, cx))))
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
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child("LOG"),
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
            .m_3()
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
                        format!("Port {} is already in use — start cancelled", report.port)
                    } else {
                        format!(
                            "Port {} is shadowed by another process — start held",
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

fn field(label: &'static str, child: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(div().text_xs().font_semibold().child(label))
        .child(child)
}

fn error_box(message: String, cx: &App) -> impl IntoElement {
    div()
        .p_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().danger)
        .text_sm()
        .text_color(cx.theme().danger)
        .whitespace_normal()
        .child(message)
}

impl Render for ServerDetail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let content = if store.creating || store.registry.instances.is_empty() {
            self.render_form(cx).into_any_element()
        } else if let Some(inst) = store.selected_instance().cloned() {
            self.render_instance(&inst, cx).into_any_element()
        } else {
            div()
                .p_4()
                .text_color(cx.theme().muted_foreground)
                .child("Select an instance in the left panel.")
                .into_any_element()
        };
        div().size_full().bg(cx.theme().background).child(content)
    }
}
