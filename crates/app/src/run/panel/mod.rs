//! Run sidebar: one-click start, stop and restart for the open project's
//! scripts (`package.json` with its package manager, workspaces, deno,
//! Make, composer, cargo) and for the user's saved commands, each one or
//! more command lines run in order or all at once. Every script runs in its
//! own terminal tab, which shows its output; a running one links the ports
//! it listens on. A project with Cloudflare Workers also gets a Local Explorer
//! section ([`explorer`]).

mod editor;
mod explorer;
mod items;
mod render;
mod row;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::scripts::{ScriptGroup, scan};

use super::store::RunStore;
use crate::ports::PortsPanel;
use crate::rt;
use crate::terminal::{JobTag, Origin, TerminalPanel};
use editor::Editor;

/// How often running state is checked.
const POLL: Duration = Duration::from_millis(500);
/// Scripts are read again every this many polls (files may have changed).
const RESCAN_EVERY: u32 = 10;
/// How often a restart looks whether the old command has exited.
const RESTART_POLL: Duration = Duration::from_millis(100);

/// Calls `then` on `this` once `busy` says its old run has stopped, looking
/// every [`RESTART_POLL`] however long that takes: a dev server's shutdown
/// hooks, `docker compose` stopping its containers. Starting again before
/// would only show the old run's terminal. Nothing happens once `this` is
/// gone.
fn once_stopped<T: 'static>(
    busy: impl Fn(&T, &App) -> bool + 'static,
    then: impl FnOnce(&mut T, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> Task<()> {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(RESTART_POLL).await;
            let Ok(busy) = this.read_with(cx, |this, cx| busy(this, cx)) else {
                return;
            };
            if !busy {
                break;
            }
        }
        let _ = this.update(cx, then);
    })
}

/// Starts the row whose first key is `key` again with `start` once `busy` says its old run has
/// stopped ([`once_stopped`]), if it still waits to then: `pending` holds the rows waiting to
/// start again, by first key, and a Stop meanwhile takes the row out, which calls the restart
/// off.
fn restart_once_stopped<T: 'static>(
    key: String,
    pending: fn(&mut T) -> &mut HashSet<String>,
    busy: impl Fn(&T, &App) -> bool + 'static,
    start: impl FnOnce(&mut T, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> Task<()> {
    once_stopped(
        busy,
        move |this, cx| {
            if pending(this).remove(&key) {
                start(this, cx);
            }
        },
        cx,
    )
}

/// One terminal to start: what to type, where, under which tab.
#[derive(Clone)]
pub struct Launch {
    pub tag: JobTag,
    pub dir: PathBuf,
    pub command: String,
}

pub enum RunEvent {
    /// Start these terminals (a running one is only shown), for whoever
    /// asked.
    Start(Vec<Launch>, Origin),
    /// Show the terminal of the script with this key.
    Show(String),
    /// Open this page in the project's browser tab (a dev server's Local Explorer).
    Browse(String),
}

impl EventEmitter<RunEvent> for RunPanel {}

/// A row: a script, or a saved command with one terminal per launch.
struct Item {
    name: String,
    /// The script's body, or the saved command lines.
    detail: String,
    launches: Vec<Launch>,
    /// Set for saved commands.
    saved: Option<String>,
}

impl Item {
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.launches.iter().map(|l| l.tag.key.as_str())
    }
}

pub struct RunPanel {
    store: Entity<RunStore>,
    terminal: Entity<TerminalPanel>,
    ports: Entity<PortsPanel>,
    root: Option<PathBuf>,
    groups: Vec<ScriptGroup>,
    /// The project's Cloudflare Workers, read with its scripts.
    workers: Vec<explorer::Worker>,
    scanned: bool,
    scanning: bool,
    scan_error: Option<String>,
    /// Collapsed groups, by `dir` + source.
    collapsed: HashSet<String>,
    filter: Entity<InputState>,
    editor: Option<Editor>,
    /// Rows being restarted, by first key.
    restarting: HashSet<String>,
    /// Running state of every row at the last poll, to redraw on change.
    last_status: Vec<(bool, bool)>,
    _subscriptions: Vec<Subscription>,
}

impl RunPanel {
    pub fn new(
        store: Entity<RunStore>,
        terminal: Entity<TerminalPanel>,
        ports: Entity<PortsPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter scripts"));
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.observe(&ports, |_, _, cx| cx.notify()),
            cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        cx.spawn(async move |this, cx| {
            let mut tick = 0u32;
            loop {
                cx.background_executor().timer(POLL).await;
                tick += 1;
                let alive = this.update(cx, |this, cx| {
                    if tick.is_multiple_of(RESCAN_EVERY) {
                        this.rescan(cx);
                    }
                    let status = this.status_snapshot(cx);
                    if status != this.last_status {
                        this.last_status = status;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            store,
            terminal,
            ports,
            root: None,
            groups: Vec::new(),
            workers: Vec::new(),
            scanned: false,
            scanning: false,
            scan_error: None,
            collapsed: HashSet::new(),
            filter,
            editor: None,
            restarting: HashSet::new(),
            last_status: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    /// The project whose scripts are listed.
    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.root == root {
            return;
        }
        self.root = root;
        self.groups.clear();
        self.workers.clear();
        self.scanned = false;
        self.editor = None;
        self.rescan(cx);
        cx.notify();
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.scanning {
            return;
        }
        self.scanning = true;
        let rx = rt::spawn_blocking(cx, {
            let root = root.clone();
            move || (scan(&root), explorer::read(&root))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.scanning = false;
                // The project changed while scanning: that scan is next.
                if this.root.as_ref() != Some(&root) {
                    this.rescan(cx);
                    return;
                }
                match result {
                    Ok((groups, workers)) => {
                        if this.scanned
                            && this.scan_error.is_none()
                            && groups == this.groups
                            && workers == this.workers
                        {
                            return;
                        }
                        this.groups = groups;
                        this.workers = workers;
                        this.scan_error = None;
                    }
                    Err(_) => {
                        this.scan_error = Some("Unable to read the project's scripts.".into())
                    }
                }
                this.scanned = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the saved commands again after `sawit-mcp` changed them.
    pub fn reload_saved(&mut self, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.reload(cx));
    }

    fn start(&mut self, item: &Item, origin: Origin, cx: &mut Context<Self>) {
        cx.emit(RunEvent::Start(item.launches.clone(), origin));
    }

    /// Stops the item (Ctrl-C), and calls off a restart of it that waits for its old run to stop:
    /// Stop means stop.
    fn stop(&mut self, item: &Item, cx: &mut Context<Self>) {
        if let Some(first) = item.keys().next() {
            self.restarting.remove(first);
        }
        self.interrupt(item, cx);
    }

    /// Interrupts (Ctrl-C) what the item's terminals run.
    fn interrupt(&mut self, item: &Item, cx: &mut Context<Self>) {
        self.terminal.update(cx, |t, cx| {
            for key in item.keys() {
                t.stop_job(key, cx);
            }
        });
        cx.notify();
    }

    /// Stops the item, waits for its commands to exit, then starts it again.
    /// Its Restart button spins meanwhile; Stop interrupts the old run again
    /// and calls the restart off.
    fn restart(&mut self, item: &Item, cx: &mut Context<Self>) {
        let Some(first) = item.keys().next().map(str::to_string) else {
            return;
        };
        if !self.restarting.insert(first.clone()) {
            return;
        }
        self.interrupt(item, cx);
        let keys: Vec<String> = item.keys().map(str::to_string).collect();
        let launches = item.launches.clone();
        restart_once_stopped(
            first,
            |this: &mut Self| &mut this.restarting,
            move |this: &Self, cx| {
                let terminal = this.terminal.read(cx);
                keys.iter().any(|k| {
                    terminal
                        .job_status(k, cx)
                        .is_some_and(|s| s.running.is_some())
                })
            },
            move |_, cx| {
                cx.emit(RunEvent::Start(launches, Origin::User));
                cx.notify();
            },
            cx,
        )
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
