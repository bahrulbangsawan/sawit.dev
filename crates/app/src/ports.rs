//! Ports sidebar: what of this user listens on a TCP port, one row per
//! command that was run (`bun run dev`, `npm run dev`, `pnpm dev`, a
//! database, …) with its ports and project folder. Dev servers are shown by
//! default, every listening app on request. Stopping a row stops the whole
//! command (launcher and children, so nothing respawns) with SIGTERM; what
//! is still alive shortly after can then be force-stopped (SIGKILL).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::ports::PortJob;
use sawit_core::probe;

use crate::rt;
use crate::servers::ServerStore;
use crate::states::{Banner, Placeholder};

/// How often the list refreshes on its own.
const POLL: Duration = Duration::from_secs(3);
/// How long stopped processes get to exit before "Force Stop" is offered.
const GRACE: Duration = Duration::from_millis(1500);

pub struct PortsPanel {
    servers: Entity<ServerStore>,
    filter: Entity<InputState>,
    jobs: Vec<PortJob>,
    /// Selected jobs, by root pid.
    selected: HashSet<u32>,
    /// Also list apps that aren't dev servers (AirPlay, browsers, …).
    show_all: bool,
    /// Stopped with SIGTERM but still running.
    stubborn: Vec<u32>,
    loading: bool,
    /// The last render drew the Refresh button spinning.
    spinner_shown: bool,
    loaded: bool,
    error: Option<String>,
    notice: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl PortsPanel {
    pub fn new(servers: Entity<ServerStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx
            .new(|cx| InputState::new(window, cx).placeholder("Filter by port, command or folder"));
        let subscriptions = vec![cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })];
        cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(POLL).await;
            }
        })
        .detach();
        Self {
            servers,
            filter,
            jobs: Vec::new(),
            selected: HashSet::new(),
            show_all: false,
            stubborn: Vec::new(),
            loading: false,
            spinner_shown: false,
            loaded: false,
            error: None,
            notice: None,
            _subscriptions: subscriptions,
        }
    }

    /// Everything listening, as of the last check.
    pub fn jobs(&self) -> &[PortJob] {
        &self.jobs
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        let rx = rt::spawn(cx, probe::listening_jobs());
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                // Every 3 s poll used to redraw; most find nothing new.
                if this.loaded
                    && this.error.is_none()
                    && matches!(&result, Ok(Ok(jobs)) if *jobs == this.jobs)
                {
                    // Unless the Refresh button is showing its spinner.
                    if this.spinner_shown {
                        cx.notify();
                    }
                    return;
                }
                this.loaded = true;
                match result {
                    Ok(Ok(jobs)) => {
                        this.error = None;
                        // Forget selections of commands that are gone.
                        this.selected
                            .retain(|root| jobs.iter().any(|j| j.root == *root));
                        this.stubborn
                            .retain(|pid| jobs.iter().any(|j| j.pids.contains(pid)));
                        this.jobs = jobs;
                    }
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {
                        this.error =
                            Some("Unable to check ports. Click Refresh to try again.".into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Stops the jobs rooted at `roots`, each with all its processes.
    fn stop_jobs(&mut self, roots: &[u32], cx: &mut Context<Self>) {
        let pids: Vec<u32> = self
            .jobs
            .iter()
            .filter(|j| roots.contains(&j.root))
            .flat_map(|j| j.pids.iter().copied())
            .collect();
        for root in roots {
            self.selected.remove(root);
        }
        let n = roots.len();
        self.signal(pids, false, format!("{n} {}", noun(n)), cx);
    }

    /// Sends SIGTERM (or SIGKILL when `force`) to `pids`, then refreshes and
    /// remembers the ones that didn't exit. `what` names them in the notice.
    fn signal(&mut self, pids: Vec<u32>, force: bool, what: String, cx: &mut Context<Self>) {
        if pids.is_empty() {
            return;
        }
        let errors: Vec<String> = pids
            .iter()
            .filter_map(|pid| probe::terminate(*pid, force).err())
            .collect();
        self.notice = Some(match (errors.is_empty(), force) {
            (true, true) => format!("Force-stopped {what}."),
            (true, false) => format!("Stopping {what}…"),
            (false, _) => errors.join("\n"),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(GRACE).await;
            let _ = this.update(cx, |this, cx| {
                let alive: Vec<u32> = pids.into_iter().filter(|p| probe::is_alive(*p)).collect();
                if alive.is_empty() {
                    if this.notice.as_deref().is_some_and(|n| n.ends_with('…')) {
                        this.notice = Some(format!("Stopped {what}."));
                    }
                } else if !force {
                    let n = alive.len();
                    let noun = if n == 1 { "process" } else { "processes" };
                    this.notice = Some(format!("{n} {noun} still running. Try Force Stop."));
                }
                this.stubborn = alive;
                this.refresh(cx);
            });
        })
        .detach();
    }

    fn visible(&self, cx: &App) -> Vec<PortJob> {
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let query = query.trim_start_matches(':');
        self.jobs
            .iter()
            .filter(|j| self.show_all || j.dev)
            .filter(|j| {
                let has = |s: &str| s.to_lowercase().contains(query);
                query.is_empty()
                    || has(&j.label)
                    || has(&j.cmdline)
                    || has(&j.tool)
                    || j.cwd.as_deref().is_some_and(has)
                    || j.listeners.iter().any(|(_, c)| has(c))
                    || j.pids.iter().any(|p| p.to_string() == query)
                    || j.ports
                        .iter()
                        .any(|(port, _)| port.to_string().starts_with(query))
            })
            .cloned()
            .collect()
    }

    fn render_job(
        &self,
        j: &PortJob,
        sawit: &HashMap<u16, String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let root = j.root;
        let muted = cx.theme().muted_foreground;
        let checked = self.selected.contains(&root);
        let owner = j
            .ports
            .iter()
            .find_map(|(port, _)| sawit.get(port).cloned());
        let tag = |text: String, color: Hsla| {
            div()
                .flex_none()
                .px_1()
                .rounded(px(3.))
                .text_xs()
                .text_color(color)
                .bg(color.opacity(0.12))
                .child(text)
        };
        let ports = h_flex()
            .flex_wrap()
            .gap_1()
            .children(j.ports.iter().map(|&(port, addr)| {
                div()
                    .id(("port-open", port as usize))
                    .px_1()
                    .rounded(px(3.))
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .bg(cx.theme().secondary)
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().accent))
                    .child(format!(":{port}"))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(format!(
                            "Listening on {addr}:{port}. Click to open http://localhost:{port}"
                        ))
                        .build(window, cx)
                    })
                    .on_click(move |_, _, cx| cx.open_url(&format!("http://localhost:{port}")))
            }));
        // "my-app · node server.js · 612"
        let folder = j
            .cwd
            .as_deref()
            .map(|c| c.rsplit('/').next().unwrap_or(c).to_string());
        let listener = j
            .listeners
            .iter()
            .find(|(pid, _)| *pid != root)
            .map(|(pid, cmd)| format!("{cmd} · {pid}"))
            .unwrap_or_else(|| root.to_string());
        let detail = folder
            .iter()
            .cloned()
            .chain(std::iter::once(listener))
            .collect::<Vec<_>>()
            .join(" · ");
        let full: SharedString = format!(
            "{}{}\n{} process{} (pids {})",
            j.cmdline,
            j.cwd
                .as_deref()
                .map(|c| format!("\nin {c}"))
                .unwrap_or_default(),
            j.pids.len(),
            if j.pids.len() == 1 { "" } else { "es" },
            j.pids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
        .into();
        let docker = j.is_docker();
        let processes = j.pids.len();
        h_flex()
            .id(("port-job", root as usize))
            .w_full()
            .px_2()
            .py_1()
            .gap_1p5()
            .items_start()
            .text_sm()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.5))
            .hover(|el| el.bg(cx.theme().list_hover))
            .child(
                Button::new(("port-check", root as usize))
                    .ghost()
                    .xsmall()
                    .icon(if checked {
                        Lucide::SquareCheck
                    } else {
                        Lucide::Square
                    })
                    .tooltip("Select")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.selected.remove(&root) {
                            this.selected.insert(root);
                        }
                        cx.notify();
                    })),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                div()
                                    .id(("port-label", root as usize))
                                    .font_semibold()
                                    .truncate()
                                    .child(j.label.clone())
                                    .tooltip(move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(full.clone())
                                            .build(window, cx)
                                    }),
                            )
                            .child(tag(j.tool.clone(), cx.theme().primary))
                            .when_some(owner, |el, name| {
                                el.child(tag(format!("Sawit · {name}"), cx.theme().success))
                            })
                            .when(docker, |el| {
                                el.child(tag("Docker".into(), cx.theme().warning))
                            }),
                    )
                    .child(ports)
                    .child(div().text_xs().text_color(muted).truncate().child(detail)),
            )
            .child(
                Button::new(("port-stop", root as usize))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::X)
                    .tooltip(if docker {
                        "Stop Docker Desktop".to_string()
                    } else if processes > 1 {
                        format!("Stop (SIGTERM to all {processes} processes)")
                    } else {
                        "Stop (SIGTERM)".to_string()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.stop_jobs(&[root], cx))),
            )
            .into_any_element()
    }
}

fn noun(n: usize) -> &'static str {
    if n == 1 { "command" } else { "commands" }
}

impl Render for PortsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.spinner_shown = self.loading;
        let visible = self.visible(cx);
        let sawit: HashMap<u16, String> = self
            .servers
            .read(cx)
            .registry
            .instances
            .iter()
            .map(|i| (i.port, i.name.clone()))
            .collect();
        // "Select all" only picks dev servers, never Docker Desktop.
        let selectable: Vec<u32> = visible
            .iter()
            .filter(|j| j.dev && !j.is_docker())
            .map(|j| j.root)
            .collect();
        let all_selected =
            !selectable.is_empty() && selectable.iter().all(|p| self.selected.contains(p));
        let selected = self.selected.len();
        let hidden = self.jobs.iter().filter(|j| !j.dev).count();
        let rows: Vec<AnyElement> = visible
            .iter()
            .map(|j| self.render_job(j, &sawit, cx))
            .collect();
        let muted = cx.theme().muted_foreground;
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
                            .text_color(muted)
                            .child("LISTENING PORTS"),
                    )
                    .child(
                        Button::new("ports-dev")
                            .ghost()
                            .xsmall()
                            .label("Dev")
                            .selected(!self.show_all)
                            .tooltip("Dev Servers, Databases and Docker")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_all = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ports-all")
                            .ghost()
                            .xsmall()
                            .label("All")
                            .selected(self.show_all)
                            .tooltip(format!(
                                "Also Other Apps That Listen ({hidden} Hidden)"
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_all = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ports-select-all")
                            .ghost()
                            .xsmall()
                            .icon(if all_selected {
                                Lucide::SquareCheck
                            } else {
                                Lucide::Square
                            })
                            .tooltip("Select All Dev Servers (Except Docker)")
                            .disabled(selectable.is_empty())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if all_selected {
                                    this.selected.clear();
                                } else {
                                    this.selected.extend(selectable.iter().copied());
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ports-refresh")
                            .ghost()
                            .xsmall()
                            .icon(IconName::RotateCw)
                            .tooltip("Refresh")
                            .loading(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .p_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(Input::new(&self.filter).small().cleanable(true))
                    .when(selected > 0, |el| {
                        el.child(
                            Button::new("ports-stop-selected")
                                .danger()
                                .xsmall()
                                .icon(Lucide::X)
                                .label(format!("Stop {selected} Selected"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let roots: Vec<u32> = this.selected.iter().copied().collect();
                                    this.stop_jobs(&roots, cx)
                                })),
                        )
                    })
                    .when(!self.stubborn.is_empty(), |el| {
                        el.child(
                            Button::new("ports-force")
                                .danger()
                                .outline()
                                .xsmall()
                                .icon(Lucide::Zap)
                                .label(format!("Force Stop {} (SIGKILL)", self.stubborn.len()))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let pids = std::mem::take(&mut this.stubborn);
                                    let n = pids.len();
                                    let what = format!(
                                        "{n} {}",
                                        if n == 1 { "process" } else { "processes" }
                                    );
                                    this.signal(pids, true, what, cx)
                                })),
                        )
                    }),
            )
            .when_some(self.notice.clone(), |el, n| {
                el.child(Banner::info("ports-notice", n).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    },
                )))
            })
            .when_some(self.error.clone(), |el, e| {
                el.child(Banner::error("ports-error", e))
            })
            .child(
                div()
                    .id("ports-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(!self.loaded && self.error.is_none(), |el| {
                        el.child(Placeholder::loading("Checking open ports…"))
                    })
                    .when(self.loaded && visible.is_empty(), |el| {
                        let filtering = !self.filter.read(cx).value().trim().is_empty();
                        el.child(if filtering {
                            Placeholder::new(IconName::Search, "No matching processes")
                                .detail("Nothing matches this port, command or folder.")
                                .action(
                                    Button::new("clear-ports-filter")
                                        .small()
                                        .label("Clear Filter")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.filter
                                                .update(cx, |s, cx| s.set_value("", window, cx));
                                        })),
                                )
                        } else if !self.show_all {
                            Placeholder::new(Lucide::Radio, "No dev servers running")
                                .detail("Run bun run dev, npm run dev, pnpm dev or any other server and it shows up here.")
                        } else {
                            Placeholder::new(Lucide::Radio, "No open ports")
                                .detail("None of your processes are listening on a TCP port. Start a server and it shows up here.")
                        })
                    }),
            )
    }
}
