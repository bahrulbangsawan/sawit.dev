//! Dev Tools page: developer tools that install with a cURL one-liner
//! (Claude Code, Bun, uv, …). Install one, a selection, or everything that
//! is missing; prerequisites (nvm before Node.js, uv before Python) are
//! queued first. Rows change state as soon as they are queued and show
//! "Installed" the moment the installer succeeds; the next detection
//! confirms it.

use std::collections::{HashMap, HashSet, VecDeque};

use futures::StreamExt as _;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::dev_tools::{Category, Found, TOOLS, Tool, detect, find, install, plan};

use crate::rt;

/// Lines of installer output kept per tool.
const LOG_LINES: usize = 200;

#[derive(Debug, Clone, PartialEq)]
enum State {
    /// Waiting its turn; `for_` names the tool that needs it, when it
    /// wasn't picked itself.
    Queued {
        for_: Option<&'static str>,
    },
    Installing,
    /// The installer succeeded; shown as installed before detection agrees.
    Done,
    Failed(String),
    /// Not run because a prerequisite isn't installed.
    Skipped(&'static str),
    /// Handed to the terminal (it asks for a password).
    InTerminal,
}

pub enum ToolsEvent {
    RunInTerminal(String),
}

impl EventEmitter<ToolsEvent> for ToolsPage {}

pub struct ToolsPage {
    found: HashMap<&'static str, Found>,
    loading: bool,
    states: HashMap<&'static str, State>,
    logs: HashMap<&'static str, Vec<String>>,
    selected: HashSet<&'static str>,
    expanded: HashSet<&'static str>,
    queue: VecDeque<&'static Tool>,
    running: Option<&'static str>,
    /// Progress through the current run of the queue.
    batch: (usize, usize),
    /// Made on the first render, which has the window an input needs.
    search: Option<Entity<InputState>>,
    _search: Option<Subscription>,
}

/// The tool's logo, 20 px: colour logos as images, one-colour ones tinted
/// with the theme.
fn logo(id: &str, size: f32, cx: &App) -> AnyElement {
    let (path, colour) = match id {
        "claude-code" => ("agent_logos/claudecode-color.svg", true),
        "codex" => ("agent_logos/codex-color.svg", true),
        "amp" => ("agent_logos/amp-color.svg", true),
        "kiro" => ("agent_logos/kiro-color.svg", true),
        "gemini" => ("agent_logos/geminicli-color.svg", true),
        "opencode" => ("agent_logos/opencode.svg", false),
        "cursor" => ("agent_logos/cursor.svg", false),
        "zed" => ("agent_logos/zed.svg", false),
        "herdr" => (crate::assets::HERDR_LOGO, true),
        "bun" | "deno" | "rust" | "flyio" => {
            return svg()
                .path(SharedString::from(format!("tool_logos/{id}.svg")))
                .size(px(size))
                .flex_none()
                .text_color(cx.theme().foreground)
                .into_any_element();
        }
        _ => {
            let name = match id {
                "node" => "nodejs",
                "droid" => "factory",
                other => other,
            };
            return img(SharedString::from(format!("tool_logos/{name}-color.svg")))
                .size(px(size))
                .flex_none()
                .into_any_element();
        }
    };
    if colour {
        img(path)
            .size(px(size))
            .flex_none()
            .rounded_sm()
            .into_any_element()
    } else {
        svg()
            .path(path)
            .size(px(size))
            .flex_none()
            .text_color(cx.theme().foreground)
            .into_any_element()
    }
}

/// `~/…` instead of the full home path.
fn tilde(path: &std::path::Path) -> String {
    let s = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if s.starts_with(&home) => s.replacen(&home, "~", 1),
        _ => s,
    }
}

impl ToolsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            found: HashMap::new(),
            loading: true,
            states: HashMap::new(),
            logs: HashMap::new(),
            selected: HashSet::new(),
            expanded: HashSet::new(),
            queue: VecDeque::new(),
            running: None,
            batch: (0, 0),
            search: None,
            _search: None,
        };
        this.reload(cx);
        this
    }

    /// Detects installed tools again (off the UI thread).
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let rx = rt::spawn(cx, detect());
        cx.spawn(async move |this, cx| {
            if let Ok(found) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.found = found;
                    this.loading = false;
                    // Detection now agrees with these; the rest keep their
                    // optimistic or failed state.
                    this.states.retain(|id, s| {
                        !(this.found.contains_key(id)
                            && matches!(s, State::Done | State::InTerminal | State::Skipped(_)))
                    });
                    this.selected.retain(|id| !this.found.contains_key(id));
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// Installed per detection, or the installer just succeeded.
    fn installed(&self, id: &str) -> bool {
        self.found.contains_key(id) || self.states.get(id) == Some(&State::Done)
    }

    fn busy(&self, id: &str) -> bool {
        matches!(
            self.states.get(id),
            Some(State::Queued { .. } | State::Installing)
        )
    }

    /// Missing tools that "Install All" covers (not the ones that need the
    /// terminal).
    fn missing(&self) -> Vec<&'static str> {
        TOOLS
            .iter()
            .filter(|t| !t.interactive && !self.installed(t.id) && !self.busy(t.id))
            .map(|t| t.id)
            .collect()
    }

    /// The tools `wanted` would run, prerequisites included.
    fn plan_for(&self, wanted: &[&str]) -> Vec<&'static Tool> {
        plan(wanted, &|id| self.installed(id) || self.busy(id))
    }

    /// Queues `wanted` (and what they need) and starts the queue.
    fn enqueue(&mut self, wanted: &[&'static str], cx: &mut Context<Self>) {
        let tools = self.plan_for(wanted);
        if self.running.is_none() && self.queue.is_empty() {
            self.batch = (0, 0);
        }
        for tool in tools {
            self.selected.remove(tool.id);
            if tool.interactive {
                self.states.insert(tool.id, State::InTerminal);
                cx.emit(ToolsEvent::RunInTerminal(tool.install.to_string()));
                continue;
            }
            let for_ = (!wanted.contains(&tool.id))
                .then(|| {
                    wanted
                        .iter()
                        .filter_map(|w| find(w))
                        .find(|w| self.plan_for(&[w.id]).iter().any(|t| t.id == tool.id))
                        .map(|w| w.name)
                })
                .flatten();
            self.states.insert(tool.id, State::Queued { for_ });
            self.queue.push_back(tool);
            self.batch.1 += 1;
        }
        self.pump(cx);
        cx.notify();
    }

    /// Starts the next queued installer unless one is running.
    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let tool = loop {
            let Some(tool) = self.queue.pop_front() else {
                // The queue is done: let detection confirm the results.
                if self.batch.1 > 0 {
                    self.reload(cx);
                }
                return;
            };
            match tool.needs.iter().find(|n| !self.installed(n)) {
                Some(need) => {
                    let name = find(need).map_or(*need, |t| t.name);
                    self.states.insert(tool.id, State::Skipped(name));
                    self.batch.0 += 1;
                }
                None => break tool,
            }
        };
        let id = tool.id;
        self.running = Some(id);
        self.states.insert(id, State::Installing);
        self.logs.insert(id, vec![format!("$ {}", tool.install)]);
        let (tx, mut lines) = futures::channel::mpsc::unbounded::<String>();
        let done = rt::spawn(cx, async move {
            install(tool, move |line| {
                let _ = tx.unbounded_send(line);
            })
            .await
            .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            // Ends when the installer exits and drops the sender.
            while let Some(line) = lines.next().await {
                let ok = this.update(cx, |this, cx| {
                    let log = this.logs.entry(id).or_default();
                    log.push(line);
                    if log.len() > LOG_LINES {
                        log.drain(..log.len() - LOG_LINES);
                    }
                    cx.notify();
                });
                if ok.is_err() {
                    return;
                }
            }
            let result = done
                .await
                .unwrap_or_else(|_| Err("The installer stopped.".into()));
            let _ = this.update(cx, |this, cx| this.finish(id, result, cx));
        })
        .detach();
    }

    fn finish(&mut self, id: &'static str, result: Result<(), String>, cx: &mut Context<Self>) {
        self.running = None;
        self.batch.0 += 1;
        match result {
            Ok(()) => {
                self.states.insert(id, State::Done);
                self.expanded.remove(id);
            }
            Err(e) => {
                self.states.insert(id, State::Failed(e));
                self.expanded.insert(id);
            }
        }
        self.pump(cx);
        cx.notify();
    }

    /// Drops everything still waiting; the running installer finishes.
    fn stop_queue(&mut self, cx: &mut Context<Self>) {
        for tool in self.queue.drain(..) {
            self.states.remove(tool.id);
        }
        self.batch.1 = self.batch.0 + usize::from(self.running.is_some());
        cx.notify();
    }

    fn dequeue(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.queue.iter().position(|t| t.id == id) {
            self.queue.remove(ix);
            self.states.remove(id);
            self.batch.1 = self.batch.1.saturating_sub(1);
        }
        cx.notify();
    }

    /// What's typed in the search field, lowercased.
    fn query(&self, cx: &App) -> String {
        self.search
            .as_ref()
            .map(|s| s.read(cx).value().trim().to_lowercase())
            .unwrap_or_default()
    }

    fn matches(tool: &Tool, query: &str) -> bool {
        let has = |s: &str| s.to_lowercase().contains(query);
        query.is_empty()
            || has(tool.name)
            || has(tool.id)
            || has(tool.description)
            || has(tool.category.title())
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let missing = self.missing();
        let selected: Vec<&'static str> = TOOLS
            .iter()
            .filter(|t| self.selected.contains(t.id))
            .map(|t| t.id)
            .collect();
        let extra = self
            .plan_for(&selected)
            .len()
            .saturating_sub(selected.len());
        let selected_label = match (selected.len(), extra) {
            (0, _) => "Install Selected".to_string(),
            (n, 0) => format!("Install Selected ({n})"),
            (n, e) => format!("Install Selected ({n} + {e} required)"),
        };
        let all_selected =
            !missing.is_empty() && missing.iter().all(|id| self.selected.contains(id));
        let can_select = !missing.is_empty();
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_lg().font_semibold().flex_1().child("Dev Tools"))
                    .when_some(self.search.clone(), |el, search| {
                        el.child(
                            div().w(px(220.)).child(
                                Input::new(&search)
                                    .small()
                                    .prefix(Icon::new(Lucide::Search).xsmall().text_color(muted))
                                    .cleanable(true),
                            ),
                        )
                    })
                    .child(
                        Button::new("tools-reload")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Detect Again")
                            .loading(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .child(
                        Button::new("tools-select-missing")
                            .ghost()
                            .small()
                            .label(if all_selected { "Clear Selection" } else { "Select Missing" })
                            .disabled(!can_select && self.selected.is_empty())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if all_selected || !can_select {
                                    this.selected.clear();
                                } else {
                                    let missing = this.missing();
                                    this.selected.extend(missing);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tools-install-selected")
                            .outline()
                            .small()
                            .label(selected_label)
                            .disabled(selected.is_empty())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.enqueue(&selected, cx)
                            })),
                    )
                    .child(
                        Button::new("tools-install-all")
                            .primary()
                            .small()
                            .label(format!("Install All Missing ({})", missing.len()))
                            .disabled(missing.is_empty() || self.loading)
                            .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&missing, cx))),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .whitespace_normal()
                    .child(
                        "Each tool installs with its official cURL script, one at a time, in your login shell. \
                         Prerequisites are queued first. Open a new terminal tab afterwards so it sees the new PATH.",
                    ),
            )
    }

    /// "Installing Node.js · 2 of 5" and a bar, while the queue runs.
    fn render_progress(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (done, total) = self.batch;
        let running = self.running.and_then(find);
        if running.is_none() && self.queue.is_empty() {
            return None;
        }
        let muted = cx.theme().muted_foreground;
        let fraction = if total == 0 {
            0.
        } else {
            done as f32 / total as f32
        };
        Some(
            v_flex()
                .gap_2()
                .p_3()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(Spinner::new().xsmall().color(muted))
                        .when_some(running, |el, t| el.child(logo(t.id, 16., cx)))
                        .child(div().flex_1().child(match running {
                            Some(t) => format!("Installing {} · {} of {total}", t.name, done + 1),
                            None => format!("{done} of {total} done"),
                        }))
                        .when(!self.queue.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{} queued", self.queue.len())),
                            )
                            .child(
                                Button::new("tools-stop")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::CircleStop)
                                    .label("Stop Queue")
                                    .tooltip("Cancel What's Waiting; the Current Install Finishes")
                                    .on_click(cx.listener(|this, _, _, cx| this.stop_queue(cx))),
                            )
                        }),
                )
                .child(
                    div()
                        .h(px(4.))
                        .w_full()
                        .rounded_full()
                        .bg(cx.theme().border)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(cx.theme().primary)
                                .w(relative(fraction.max(0.02))),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_row(&self, ix: usize, tool: &'static Tool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let id = tool.id;
        let state = self.states.get(id).cloned();
        let found = self.found.get(id);
        let installed = self.installed(id);
        let busy = self.busy(id);

        let (badge, color): (String, Hsla) = match (&state, found) {
            (Some(State::Queued { for_: Some(f) }), _) => (format!("Queued · for {f}"), muted),
            (Some(State::Queued { for_: None }), _) => ("Queued".into(), muted),
            (Some(State::Installing), _) => ("Installing…".into(), theme.warning),
            (Some(State::Failed(_)), _) => ("Failed".into(), theme.danger),
            (Some(State::Skipped(need)), _) => (format!("Skipped · {need} missing"), theme.danger),
            (Some(State::InTerminal), _) => ("In terminal".into(), theme.warning),
            (Some(State::Done), None) if !self.loading => {
                ("Installed · restart shell".into(), theme.success)
            }
            (_, Some(f)) => (
                f.version
                    .as_ref()
                    .map_or("Installed".into(), |v| format!("Installed · {v}")),
                theme.success,
            ),
            (Some(State::Done), None) => ("Installed".into(), theme.success),
            (None, None) if self.loading => ("Checking…".into(), muted),
            (None, None) => ("Not installed".into(), muted),
        };

        let action = match &state {
            Some(State::Installing) => Button::new(("tool-busy", ix))
                .ghost()
                .xsmall()
                .label("Installing")
                .loading(true)
                .disabled(true),
            Some(State::Queued { .. }) => Button::new(("tool-dequeue", ix))
                .ghost()
                .xsmall()
                .label("Cancel")
                .on_click(cx.listener(move |this, _, _, cx| this.dequeue(id, cx))),
            Some(State::Failed(_) | State::Skipped(_)) => Button::new(("tool-retry", ix))
                .primary()
                .xsmall()
                .icon(Lucide::RotateCcw)
                .label("Retry")
                .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
            _ if installed => Button::new(("tool-reinstall", ix))
                .ghost()
                .xsmall()
                .label("Reinstall")
                .tooltip("Run the Installer Again (Updates to the Latest Version)")
                .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
            _ if tool.interactive => Button::new(("tool-terminal", ix))
                .primary()
                .xsmall()
                .icon(Lucide::SquareTerminal)
                .label("Install in Terminal")
                .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
            _ => Button::new(("tool-install", ix))
                .primary()
                .xsmall()
                .icon(Lucide::Download)
                .label("Install")
                .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
        };

        let checkbox = if installed || busy || tool.interactive {
            div().size(px(16.)).flex_none().into_any_element()
        } else {
            let checked = self.selected.contains(id);
            Checkbox::new(("tool-pick", ix))
                .checked(checked)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    if *checked {
                        this.selected.insert(id);
                    } else {
                        this.selected.remove(id);
                    }
                    cx.notify();
                }))
                .into_any_element()
        };

        // One chip per prerequisite: green once it's installed.
        let needs = tool.needs.iter().filter_map(|n| find(n)).map(|need| {
            let ok = self.installed(need.id);
            h_flex()
                .gap_1()
                .px_1p5()
                .py_0p5()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(if ok {
                    cx.theme().success
                } else {
                    cx.theme().border
                })
                .text_xs()
                .text_color(if ok { cx.theme().success } else { muted })
                .child(logo(need.id, 12., cx))
                .child(need.name)
                .when(ok, |el| el.child(Icon::new(Lucide::Check).xsmall()))
        });

        let log = self.logs.get(id);
        let last_line = log
            .filter(|_| state == Some(State::Installing))
            .and_then(|l| l.last().cloned());
        let expanded = self.expanded.contains(id) && log.is_some();
        let command = tool.install;

        v_flex()
            .px_3()
            .py_2p5()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .when(state == Some(State::Installing), |el| {
                el.bg(cx.theme().list_hover)
            })
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(div().w(px(16.)).flex_none().pt(px(2.)).child(checkbox))
                    .child(div().pt(px(1.)).child(logo(id, 20., cx)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(div().text_sm().font_semibold().child(tool.name))
                                    .when(!tool.needs.is_empty(), |el| {
                                        el.child(div().text_xs().text_color(muted).child("needs"))
                                            .children(needs)
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .whitespace_normal()
                                    .child(tool.description),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_xs()
                                            .font_family(mono.clone())
                                            .text_color(muted)
                                            .child(match found {
                                                Some(f) => tilde(&f.path),
                                                None => command.to_string(),
                                            }),
                                    )
                                    .child(
                                        Button::new(("tool-copy", ix))
                                            .ghost()
                                            .xsmall()
                                            .icon(Lucide::Copy)
                                            .tooltip("Copy Install Command")
                                            .on_click(move |_, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    command.to_string(),
                                                ))
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .w(px(170.))
                            .flex_none()
                            .justify_end()
                            .pt(px(2.))
                            .text_xs()
                            .text_color(color)
                            .child(match state {
                                Some(State::Installing) => {
                                    Spinner::new().xsmall().color(color).into_any_element()
                                }
                                _ => div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(color)
                                    .into_any_element(),
                            })
                            .child(div().truncate().child(badge)),
                    )
                    .child(
                        h_flex()
                            .gap_0p5()
                            .w(px(150.))
                            .flex_none()
                            .justify_end()
                            .when(log.is_some(), |el| {
                                el.child(
                                    Button::new(("tool-log", ix))
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::ScrollText)
                                        .selected(expanded)
                                        .tooltip(if expanded {
                                            "Hide Output"
                                        } else {
                                            "Show Output"
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !this.expanded.remove(id) {
                                                this.expanded.insert(id);
                                            }
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(action),
                    ),
            )
            .when_some(last_line.filter(|_| !expanded), |el, line| {
                el.child(
                    div()
                        .pl(px(60.))
                        .text_xs()
                        .font_family(mono.clone())
                        .text_color(muted)
                        .truncate()
                        .child(line),
                )
            })
            .when_some(
                match &state {
                    Some(State::Failed(e)) if !expanded => Some(e.clone()),
                    _ => None,
                },
                |el, e| {
                    el.child(
                        div()
                            .pl(px(60.))
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .whitespace_normal()
                            .child(e),
                    )
                },
            )
            .when(expanded, |el| {
                let lines = log
                    .map(|l| l[l.len().saturating_sub(14)..].join("\n"))
                    .unwrap_or_default();
                el.child(
                    div()
                        .ml(px(60.))
                        .p_2()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().secondary)
                        .text_xs()
                        .font_family(mono)
                        .text_color(muted)
                        .whitespace_normal()
                        .child(lines),
                )
            })
            .into_any_element()
    }
}

impl Render for ToolsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.search.is_none() {
            let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search tools"));
            self._search = Some(cx.subscribe(&search, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }));
            self.search = Some(search);
        }
        let muted = cx.theme().muted_foreground;
        let query = self.query(cx);
        let mut sections: Vec<AnyElement> = Vec::new();
        for category in Category::ALL {
            let rows: Vec<AnyElement> = TOOLS
                .iter()
                .enumerate()
                .filter(|(_, t)| t.category == category && Self::matches(t, &query))
                .map(|(ix, t)| self.render_row(ix, t, cx))
                .collect();
            if rows.is_empty() {
                continue;
            }
            sections.push(
                div()
                    .pt_2()
                    .text_xs()
                    .font_semibold()
                    .text_color(muted)
                    .child(category.title())
                    .into_any_element(),
            );
            sections.push(
                v_flex()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .overflow_hidden()
                    .children(rows)
                    .into_any_element(),
            );
        }
        if sections.is_empty() {
            sections.push(
                div()
                    .pt_2()
                    .text_sm()
                    .text_color(muted)
                    .child(format!("No tools match “{query}”."))
                    .into_any_element(),
            );
        }
        let progress = self.render_progress(cx);
        div()
            .id("tools-page")
            .size_full()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .p_6()
                    .gap_3()
                    .max_w(px(1080.))
                    .child(self.render_header(cx))
                    .children(progress)
                    .children(sections),
            )
    }
}
