//! Run sidebar: one-click start, stop and restart for the open project's
//! scripts (`package.json` with its package manager, workspaces, deno,
//! Make, composer, cargo) and for the user's saved commands, each one or
//! more command lines run in order or all at once. Every script runs in its
//! own terminal tab, which shows its output; a running one links the ports
//! it listens on.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::scripts::{RunMode, SavedCommand, ScriptGroup, clean_commands, scan};

use super::store::RunStore;
use crate::ports::PortsPanel;
use crate::rt;
use crate::states::{Banner, Placeholder};
use crate::terminal::{JobTag, TerminalPanel};

/// How often running state is checked.
const POLL: Duration = Duration::from_millis(500);
/// Scripts are read again every this many polls (files may have changed).
const RESCAN_EVERY: u32 = 10;
/// How long a restart waits for the old command to exit.
const RESTART_WAIT: Duration = Duration::from_secs(5);

/// One terminal to start: what to type, where, under which tab.
#[derive(Clone)]
pub struct Launch {
    pub tag: JobTag,
    pub dir: PathBuf,
    pub command: String,
}

pub enum RunEvent {
    /// Start these terminals (a running one is only shown).
    Start(Vec<Launch>),
    /// Show the terminal of the script with this key.
    Show(String),
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

/// The add / edit form for a saved command.
struct Editor {
    /// The command being edited; `None` for a new one.
    id: Option<String>,
    name: Entity<InputState>,
    lines: Vec<Entity<InputState>>,
    mode: RunMode,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

pub struct RunPanel {
    store: Entity<RunStore>,
    terminal: Entity<TerminalPanel>,
    ports: Entity<PortsPanel>,
    root: Option<PathBuf>,
    groups: Vec<ScriptGroup>,
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
            move || scan(&root)
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
                    Ok(groups) => {
                        if this.scanned && this.scan_error.is_none() && groups == this.groups {
                            return;
                        }
                        this.groups = groups;
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

    fn saved_items(&self, root: &Path, cx: &App) -> Vec<Item> {
        self.store
            .read(cx)
            .saved
            .for_project(root)
            .iter()
            .map(|c| {
                let lines = c.lines();
                let many = lines.len() > 1;
                let launches = lines
                    .into_iter()
                    .enumerate()
                    .map(|(i, command)| Launch {
                        tag: JobTag {
                            key: format!("saved\u{0}{}\u{0}{}\u{0}{i}", root.display(), c.id),
                            name: if many {
                                format!("{} {}", c.name, i + 1)
                            } else {
                                c.name.clone()
                            },
                        },
                        dir: root.to_path_buf(),
                        command,
                    })
                    .collect();
                let joiner = match c.mode {
                    RunMode::Sequence => " → ",
                    RunMode::Parallel => " ‖ ",
                };
                Item {
                    name: c.name.clone(),
                    detail: c.commands.join(joiner),
                    launches,
                    saved: Some(c.id.clone()),
                }
            })
            .collect()
    }

    fn group_items(group: &ScriptGroup) -> Vec<Item> {
        group
            .scripts
            .iter()
            .map(|s| Item {
                name: s.name.clone(),
                detail: if s.body.is_empty() {
                    s.command.clone()
                } else {
                    s.body.clone()
                },
                launches: vec![Launch {
                    tag: JobTag {
                        key: format!(
                            "script\u{0}{}\u{0}{}\u{0}{}",
                            group.dir.display(),
                            group.source,
                            s.name
                        ),
                        name: if group.rel.is_empty() {
                            s.name.clone()
                        } else {
                            format!("{} · {}", group.label, s.name)
                        },
                    },
                    dir: group.dir.clone(),
                    command: s.command.clone(),
                }],
                saved: None,
            })
            .collect()
    }

    /// (open, running) per row, in order.
    fn status_snapshot(&self, cx: &App) -> Vec<(bool, bool)> {
        let Some(root) = &self.root else {
            return Vec::new();
        };
        let terminal = self.terminal.read(cx);
        self.saved_items(root, cx)
            .iter()
            .chain(
                self.groups
                    .iter()
                    .flat_map(Self::group_items)
                    .collect::<Vec<_>>()
                    .iter(),
            )
            .map(|item| {
                let status: Vec<_> = item
                    .keys()
                    .filter_map(|k| terminal.job_status(k, cx))
                    .collect();
                (
                    !status.is_empty(),
                    status.iter().any(|s| s.running.is_some()),
                )
            })
            .collect()
    }

    /// Reads the saved commands again after `sawit-mcp` changed them.
    pub fn reload_saved(&mut self, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.reload(cx));
    }

    /// Every row, saved commands first, as the panel lists them.
    fn items(&self, cx: &App) -> Vec<Item> {
        let Some(root) = &self.root else {
            return Vec::new();
        };
        let mut items = self.saved_items(root, cx);
        items.extend(self.groups.iter().flat_map(Self::group_items));
        items
    }

    /// The name `sawit-mcp` uses for a row: a script's tab name
    /// (`web · dev` in a workspace package), or a saved command's name.
    fn label(item: &Item) -> &str {
        match (&item.saved, item.launches.first()) {
            (None, Some(launch)) => &launch.tag.name,
            _ => &item.name,
        }
    }

    /// Every row with its command and whether it is running.
    pub fn describe(&self, cx: &App) -> Result<Vec<serde_json::Value>, String> {
        if self.root.is_none() {
            return Err("No project is open in this window.".into());
        }
        let terminal = self.terminal.read(cx);
        Ok(self
            .items(cx)
            .iter()
            .map(|item| {
                let running = item
                    .keys()
                    .filter_map(|k| terminal.job_status(k, cx))
                    .any(|s| s.running.is_some());
                serde_json::json!({
                    "name": Self::label(item),
                    "command": item.detail,
                    "saved": item.saved.is_some(),
                    "running": running,
                })
            })
            .collect())
    }

    /// The row called `name`: its label, else a unique row with that name.
    fn find(&self, name: &str, cx: &App) -> Result<Item, String> {
        if self.root.is_none() {
            return Err("No project is open in this window.".into());
        }
        if !self.scanned {
            return Err("Sawit is still reading the project's scripts. Try again in a moment.".into());
        }
        let mut items = self.items(cx);
        if let Some(ix) = items.iter().position(|i| Self::label(i) == name) {
            return Ok(items.swap_remove(ix));
        }
        let mut same: Vec<Item> = items.into_iter().filter(|i| i.name == name).collect();
        match same.len() {
            1 => Ok(same.remove(0)),
            0 => Err(format!("No script or saved command is called “{name}”. See app_run_list.")),
            _ => Err(format!(
                "Several scripts are called “{name}”: {}. Use the full name.",
                same.iter().map(Self::label).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    pub fn start_named(&mut self, name: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let item = self.find(name, cx)?;
        self.start(&item, cx);
        Ok(())
    }

    pub fn stop_named(&mut self, name: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let item = self.find(name, cx)?;
        self.stop(&item, cx);
        Ok(())
    }

    fn start(&mut self, item: &Item, cx: &mut Context<Self>) {
        cx.emit(RunEvent::Start(item.launches.clone()));
    }

    fn stop(&mut self, item: &Item, cx: &mut Context<Self>) {
        self.terminal.update(cx, |t, cx| {
            for key in item.keys() {
                t.stop_job(key, cx);
            }
        });
        cx.notify();
    }

    /// Stops the item, waits for its commands to exit, then starts it again.
    fn restart(&mut self, item: &Item, cx: &mut Context<Self>) {
        let Some(first) = item.keys().next().map(str::to_string) else {
            return;
        };
        if !self.restarting.insert(first.clone()) {
            return;
        }
        self.stop(item, cx);
        let keys: Vec<String> = item.keys().map(str::to_string).collect();
        let launches = item.launches.clone();
        cx.spawn(async move |this, cx| {
            let started = std::time::Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let Ok(busy) = this.update(cx, |this, cx| {
                    let terminal = this.terminal.read(cx);
                    keys.iter().any(|k| {
                        terminal
                            .job_status(k, cx)
                            .is_some_and(|s| s.running.is_some())
                    })
                }) else {
                    return;
                };
                if !busy || started.elapsed() > RESTART_WAIT {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.restarting.remove(&first);
                cx.emit(RunEvent::Start(launches));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn open_editor(&mut self, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let existing = id.as_ref().and_then(|id| {
            let root = self.root.as_ref()?;
            self.store.read(cx).saved.get(root, id).cloned()
        });
        let name = existing
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Name, e.g. Start everything")
                .default_value(name)
        });
        let mode = existing.as_ref().map(|c| c.mode).unwrap_or_default();
        let mut editor = Editor {
            id: existing.as_ref().map(|c| c.id.clone()),
            name: name.clone(),
            lines: Vec::new(),
            mode,
            error: None,
            _subscriptions: vec![self.save_on_enter(&name, window, cx)],
        };
        let commands = existing.map(|c| c.commands).unwrap_or_default();
        for command in if commands.is_empty() {
            vec![String::new()]
        } else {
            commands
        } {
            self.push_line(&mut editor, command, window, cx);
        }
        name.update(cx, |s, cx| s.focus(window, cx));
        self.editor = Some(editor);
        cx.notify();
    }

    fn save_on_enter(
        &self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.save_editor(window, cx);
            }
        })
    }

    fn push_line(
        &self,
        editor: &mut Editor,
        command: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let n = editor.lines.len() + 1;
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(if n == 1 {
                    "Command, e.g. pnpm dev".to_string()
                } else {
                    format!("Command {n}")
                })
                .default_value(command)
        });
        editor
            ._subscriptions
            .push(self.save_on_enter(&input, window, cx));
        editor.lines.push(input.clone());
        input
    }

    fn add_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut editor) = self.editor.take() else {
            return;
        };
        let input = self.push_line(&mut editor, String::new(), window, cx);
        input.update(cx, |s, cx| s.focus(window, cx));
        self.editor = Some(editor);
        cx.notify();
    }

    fn save_editor(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let (Some(root), Some(editor)) = (self.root.clone(), self.editor.as_mut()) else {
            return;
        };
        let name = editor.name.read(cx).value().trim().to_string();
        let commands = clean_commands(
            editor
                .lines
                .iter()
                .map(|l| l.read(cx).value().to_string())
                .collect(),
        );
        if commands.is_empty() {
            editor.error = Some("Add at least one command.".into());
            cx.notify();
            return;
        }
        let name = if name.is_empty() {
            commands[0].clone()
        } else {
            name
        };
        let mut command = SavedCommand::new(&name, commands, editor.mode);
        if let Some(id) = &editor.id {
            command.id = id.clone();
        }
        self.store.update(cx, |s, cx| s.upsert(&root, command, cx));
        self.editor = None;
        cx.notify();
    }

    fn delete_saved(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.store.update(cx, |s, cx| s.remove(&root, id, cx));
        self.editor = None;
        cx.notify();
    }

    fn matches(&self, item: &Item, group: Option<&ScriptGroup>, query: &str) -> bool {
        let has = |s: &str| s.to_lowercase().contains(query);
        query.is_empty()
            || has(&item.name)
            || has(&item.detail)
            || item.launches.iter().any(|l| has(&l.command))
            || group.is_some_and(|g| has(&g.label) || has(&g.tool))
    }
}

impl RunPanel {
    fn render_section(
        &self,
        id: impl Into<ElementId>,
        title: String,
        tag: Option<String>,
        tip: String,
        collapsed: Option<bool>,
        cx: &App,
    ) -> Stateful<Div> {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .id(id)
            .w_full()
            .h(px(24.))
            .px_2()
            .gap_1()
            .text_xs()
            .font_semibold()
            .text_color(muted)
            .bg(cx.theme().sidebar)
            .when_some(collapsed, |el, collapsed| {
                el.cursor_pointer()
                    .hover(|el| el.bg(cx.theme().list_hover))
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .xsmall(),
                    )
            })
            .child(div().flex_1().min_w_0().truncate().child(title))
            .when_some(tag, |el, tag| {
                el.child(
                    div()
                        .flex_none()
                        .px_1()
                        .rounded(px(3.))
                        .font_normal()
                        .text_color(cx.theme().primary)
                        .bg(cx.theme().primary.opacity(0.12))
                        .child(tag),
                )
            })
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
    }

    fn render_item(&self, ix: usize, item: Item, cx: &mut Context<Self>) -> AnyElement {
        let (open, running, pgrps) = {
            let terminal = self.terminal.read(cx);
            let status: Vec<_> = item
                .keys()
                .filter_map(|k| terminal.job_status(k, cx))
                .collect();
            let pgrps: Vec<u32> = status.iter().filter_map(|s| s.running).collect();
            (!status.is_empty(), !pgrps.is_empty(), pgrps)
        };
        let restarting = item
            .keys()
            .next()
            .is_some_and(|k| self.restarting.contains(k));
        let mut ports: Vec<u16> = self
            .ports
            .read(cx)
            .jobs()
            .iter()
            .filter(|j| pgrps.iter().any(|p| j.root == *p || j.pids.contains(p)))
            .flat_map(|j| j.ports.iter().map(|(port, _)| *port))
            .collect();
        ports.sort_unstable();
        ports.dedup();
        let muted = cx.theme().muted_foreground;
        let dot = if running {
            cx.theme().success
        } else if open {
            muted
        } else {
            muted.opacity(0.35)
        };
        let many = item.launches.len();
        let tip: SharedString = item
            .launches
            .iter()
            .map(|l| l.command.clone())
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        let first_key = item.keys().next().unwrap_or_default().to_string();
        let saved = item.saved.clone();
        let item = std::rc::Rc::new(item);
        h_flex()
            .id(("run-item", ix))
            .w_full()
            .px_2()
            .py_1()
            .gap_1p5()
            .items_start()
            .text_sm()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.5))
            .hover(|el| el.bg(cx.theme().list_hover))
            .when(open, |el| el.cursor_pointer())
            .on_click(cx.listener({
                let key = first_key.clone();
                move |_, _, _, cx| {
                    if open {
                        cx.emit(RunEvent::Show(key.clone()));
                    }
                }
            }))
            .child(
                div()
                    .flex_none()
                    .mt(px(7.))
                    .size(px(7.))
                    .rounded_full()
                    .bg(dot),
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
                                    .id(("run-name", ix))
                                    .font_semibold()
                                    .truncate()
                                    .child(item.name.clone())
                                    .tooltip(move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                            .build(window, cx)
                                    }),
                            )
                            .when(many > 1, |el| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .px_1()
                                        .rounded(px(3.))
                                        .text_xs()
                                        .text_color(muted)
                                        .bg(cx.theme().secondary)
                                        .child(format!("{many} terminals")),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(item.detail.clone()),
                    )
                    .when(!ports.is_empty(), |el| {
                        el.child(h_flex().flex_wrap().gap_1().children(ports.into_iter().map(
                            |port| {
                                div()
                                    .id(("run-port", ix * 100_000 + port as usize))
                                    .px_1()
                                    .rounded(px(3.))
                                    .text_xs()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_color(cx.theme().success)
                                    .bg(cx.theme().success.opacity(0.12))
                                    .cursor_pointer()
                                    .child(format!(":{port}"))
                                    .tooltip(move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(format!(
                                            "Open http://localhost:{port}"
                                        ))
                                        .build(window, cx)
                                    })
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        cx.open_url(&format!("http://localhost:{port}"))
                                    })
                            },
                        )))
                    }),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_0p5()
                    .when_some(saved, |el, id| {
                        el.child(
                            Button::new(("run-edit", ix))
                                .ghost()
                                .xsmall()
                                .icon(Lucide::Pencil)
                                .tooltip("Edit Command")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.open_editor(Some(id.clone()), window, cx)
                                })),
                        )
                    })
                    .when(running, |el| {
                        el.child(
                            Button::new(("run-restart", ix))
                                .ghost()
                                .xsmall()
                                .icon(Lucide::RotateCcw)
                                .loading(restarting)
                                .tooltip("Restart")
                                .on_click(cx.listener({
                                    let item = item.clone();
                                    move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.restart(&item, cx)
                                    }
                                })),
                        )
                    })
                    .child(if running {
                        Button::new(("run-stop", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::CircleStop)
                            .tooltip("Stop (Ctrl-C)")
                            .on_click(cx.listener({
                                let item = item.clone();
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.stop(&item, cx)
                                }
                            }))
                    } else {
                        Button::new(("run-start", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Play)
                            .tooltip(if many > 1 {
                                format!("Run in {many} Terminals")
                            } else {
                                "Run in Terminal".to_string()
                            })
                            .on_click(cx.listener({
                                let item = item.clone();
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.start(&item, cx)
                                }
                            }))
                    }),
            )
            .into_any_element()
    }

    fn render_editor(&self, editor: &Editor, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let lines = editor.lines.len();
        let mode = editor.mode;
        v_flex()
            .m_2()
            .p_2()
            .gap_1p5()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(div().text_xs().font_semibold().text_color(muted).child(
                if editor.id.is_some() {
                    "EDIT COMMAND"
                } else {
                    "NEW COMMAND"
                },
            ))
            .child(Input::new(&editor.name).small())
            .children(editor.lines.iter().enumerate().map(|(i, line)| {
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .flex_none()
                            .w(px(14.))
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{}", i + 1)),
                    )
                    .child(div().flex_1().min_w_0().child(Input::new(line).small()))
                    .child(
                        Button::new(("run-line-remove", i))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .tooltip("Remove This Command")
                            .disabled(lines == 1)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(editor) = this.editor.as_mut()
                                    && editor.lines.len() > 1
                                {
                                    editor.lines.remove(i);
                                    cx.notify();
                                }
                            })),
                    )
            }))
            .child(
                Button::new("run-line-add")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .label("Add Another Command")
                    .on_click(cx.listener(|this, _, window, cx| this.add_line(window, cx))),
            )
            .when(lines > 1, |el| {
                el.child(
                    h_flex()
                        .gap_1()
                        .child(div().text_xs().text_color(muted).child("Run"))
                        .child(
                            Button::new("run-mode-sequence")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::ListOrdered)
                                .label("In Order")
                                .selected(mode == RunMode::Sequence)
                                .tooltip("One after another in one terminal (stops if one fails)")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(e) = this.editor.as_mut() {
                                        e.mode = RunMode::Sequence;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("run-mode-parallel")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::Layers)
                                .label("All at Once")
                                .selected(mode == RunMode::Parallel)
                                .tooltip("At the same time, each in its own terminal")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(e) = this.editor.as_mut() {
                                        e.mode = RunMode::Parallel;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when_some(editor.error.clone(), |el, e| {
                el.child(div().text_xs().text_color(cx.theme().danger).child(e))
            })
            .child(
                h_flex()
                    .gap_1()
                    .when_some(editor.id.clone(), |el, id| {
                        el.child(
                            Button::new("run-editor-delete")
                                .ghost()
                                .danger()
                                .xsmall()
                                .icon(Lucide::Trash)
                                .label("Delete")
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.delete_saved(&id, cx)),
                                ),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("run-editor-cancel")
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.editor = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("run-editor-save")
                            .primary()
                            .xsmall()
                            .label("Save")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_editor(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for RunPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let header = h_flex()
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
                    .child("RUN"),
            )
            .child(
                Button::new("run-add")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .tooltip("Save a Command")
                    .disabled(self.root.is_none() || self.store.read(cx).load_error.is_some())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_editor(None, window, cx)),
                    ),
            )
            .child(
                Button::new("run-refresh")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RotateCw)
                    .tooltip("Scan Scripts Again")
                    .disabled(self.root.is_none())
                    .loading(self.scanning)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rescan(cx);
                        cx.notify();
                    })),
            );
        let Some(root) = self.root.clone() else {
            return v_flex()
                .size_full()
                .bg(cx.theme().sidebar)
                .child(header)
                .child(Placeholder::new(Lucide::Play, "No project open").detail(
                    "Open a project folder to run its scripts (dev, build, …) with one click.",
                ));
        };
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut ix = 0;

        // Saved commands first: they are what the user picked.
        let saved: Vec<Item> = self
            .saved_items(&root, cx)
            .into_iter()
            .filter(|i| self.matches(i, None, &query))
            .collect();
        if !saved.is_empty() || query.is_empty() {
            rows.push(
                self.render_section(
                    "run-section-saved",
                    "SAVED COMMANDS".into(),
                    None,
                    "Your own commands for this project".into(),
                    None,
                    cx,
                )
                .into_any_element(),
            );
        }
        if saved.is_empty() && query.is_empty() && self.editor.is_none() {
            rows.push(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_1().child(
                        "Save commands you run often, like docker compose up and pnpm dev together.",
                    ))
                    .child(
                        Button::new("run-add-empty")
                            .xsmall()
                            .icon(IconName::Plus)
                            .label("Add")
                            .disabled(self.store.read(cx).load_error.is_some())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_editor(None, window, cx)),
                            ),
                    )
                    .into_any_element(),
            );
        }
        for item in saved {
            rows.push(self.render_item(ix, item, cx));
            ix += 1;
        }

        let filtering = !query.is_empty();
        let mut shown_scripts = 0;
        for (g_ix, group) in self.groups.clone().iter().enumerate() {
            let items: Vec<Item> = Self::group_items(group)
                .into_iter()
                .filter(|i| self.matches(i, Some(group), &query))
                .collect();
            if items.is_empty() {
                continue;
            }
            shown_scripts += items.len();
            let group_key = format!("{}\u{0}{}", group.dir.display(), group.source);
            let collapsed = !filtering && self.collapsed.contains(&group_key);
            let where_ = if group.rel.is_empty() {
                group.source.clone()
            } else {
                format!("{}/{}", group.rel, group.source)
            };
            rows.push(
                self.render_section(
                    ("run-section", g_ix),
                    group.label.to_uppercase(),
                    Some(group.tool.clone()),
                    format!("{} scripts from {where_}", items.len()),
                    Some(collapsed),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.collapsed.remove(&group_key) {
                        this.collapsed.insert(group_key.clone());
                    }
                    cx.notify();
                }))
                .into_any_element(),
            );
            for item in items {
                if !collapsed {
                    rows.push(self.render_item(ix, item, cx));
                }
                ix += 1;
            }
        }

        let store = self.store.read(cx);
        let load_error = store.load_error.clone();
        let notice = store.notice.clone();
        let editor = self.editor.as_ref().map(|e| self.render_editor(e, cx));
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(header)
            .child(
                div()
                    .p_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(Input::new(&self.filter).small().cleanable(true)),
            )
            .when_some(load_error, |el, e| {
                el.child(Banner::error(
                    "run-load-error",
                    format!("Saved commands can't be read, so changes won't be saved. {e}"),
                ))
            })
            .when_some(notice, |el, n| {
                el.child(Banner::info("run-notice", n).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.store.update(cx, |s, cx| {
                            s.notice = None;
                            cx.notify();
                        });
                    },
                )))
            })
            .when_some(self.scan_error.clone(), |el, e| {
                el.child(Banner::error("run-scan-error", e))
            })
            .children(editor)
            .child(
                div()
                    .id("run-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(!self.scanned, |el| {
                        el.child(Placeholder::loading("Looking for scripts…").compact())
                    })
                    .when(self.scanned && shown_scripts == 0, |el| {
                        el.child(if filtering {
                            Placeholder::new(IconName::Search, "No matching scripts")
                                .compact()
                                .action(
                                    Button::new("run-clear-filter")
                                        .small()
                                        .label("Clear Filter")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.filter
                                                .update(cx, |s, cx| s.set_value("", window, cx));
                                        })),
                                )
                        } else {
                            Placeholder::new(Lucide::Package, "No scripts found")
                                .compact()
                                .detail("Nothing in package.json, deno.json, composer.json, a Makefile or Cargo.toml here. Save your own command with +.")
                        })
                    }),
            )
    }
}
