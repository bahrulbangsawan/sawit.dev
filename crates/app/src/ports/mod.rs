//! Ports sidebar: what of this user listens on a TCP port, one row per
//! command that was run (`bun run dev`, `npm run dev`, `pnpm dev`, a
//! database, …) with its ports and project folder. Dev servers are shown by
//! default, every listening app on request. Stopping a row stops the whole
//! command (launcher and children, so nothing respawns) with SIGTERM; what
//! is still alive shortly after can then be force-stopped (SIGKILL). A Workers dev server that
//! serves Cloudflare's Local Explorer gets a button that opens it in Sawit's browser.

mod render;
mod row;

use std::collections::HashSet;
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_core::cloudflare::explorer::{self, Explorer};
use sawit_core::ports::PortJob;
use sawit_core::probe;

use crate::rt;
use crate::servers::ServerStore;

/// How often the list refreshes on its own.
const POLL: Duration = Duration::from_secs(3);
/// How long stopped processes get to exit before "Force Stop" is offered.
const GRACE: Duration = Duration::from_millis(1500);

/// What the panel asks of the workspace.
pub enum PortsEvent {
    OpenInBrowser(String),
}

impl EventEmitter<PortsEvent> for PortsPanel {}

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
    /// The Local Explorers the dev servers serve, as last asked.
    explorers: Vec<Explorer>,
    /// Asking the dev servers for Local Explorer.
    probing: bool,
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
            explorers: Vec::new(),
            probing: false,
            _subscriptions: subscriptions,
        }
    }

    /// The Local Explorers running, as of the last check: one per dev server that serves one.
    pub fn explorers(&self) -> &[Explorer] {
        &self.explorers
    }

    /// Asks the dev servers among `jobs` whether they serve Local Explorer, then keeps what was
    /// found here and for the browsers ([`crate::local_explorer::publish`]). The answers are kept
    /// per port and process, so most checks ask no one.
    fn find_explorers(&mut self, jobs: Vec<PortJob>, cx: &mut Context<Self>) {
        if self.probing {
            return;
        }
        self.probing = true;
        let rx = rt::spawn(cx, async move { explorer::find(&jobs).await });
        cx.spawn(async move |this, cx| {
            let found = rx.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.probing = false;
                crate::local_explorer::publish(&found, cx);
                if found != this.explorers {
                    this.explorers = found;
                    cx.notify();
                }
            });
        })
        .detach();
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
                if let Ok(Ok(jobs)) = &result {
                    this.find_explorers(jobs.clone(), cx);
                }
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
}

fn noun(n: usize) -> &'static str {
    if n == 1 { "command" } else { "commands" }
}
