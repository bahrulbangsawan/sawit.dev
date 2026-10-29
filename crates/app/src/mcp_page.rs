//! MCP page: a catalog of MCP servers (the bundled `sawit-mcp`, remote
//! servers such as AWS, Cloudflare and Neon, and local ones run with npx),
//! each installed into any AI client and profile found on this Mac. Remote
//! servers that need an account are signed in here through Claude Code's
//! and Codex's own `mcp login`, profile by profile. A server can be removed
//! from every client in one go, and an install or removal that hangs can be
//! stopped.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use futures::StreamExt as _;
use futures::future::{AbortHandle, abortable};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::mcp_auth::{
    self, Login, LoginEvent, SignIn, is_callback_url, login_call, sign_in_states,
};
use sawit_core::mcp_catalog::{Auth, DEFAULT_AWS_REGION, SERVERS, Server};
use sawit_core::mcp_clients::{
    Category, Client, Spec, Status, Target, discover, install, snippet, status, uninstall,
};
use sawit_core::probe::find_in_path;

use crate::rt;

/// A server in the catalog and a target, by index.
type Pair = (usize, usize);

/// Programs local servers and the remote bridge run.
const PROGRAMS: [&str; 2] = ["npx", "cap"];

pub struct McpPage {
    /// `sawit-mcp` next to the running app (inside the .app bundle, or the
    /// cargo target folder during development).
    server: PathBuf,
    server_exists: bool,
    targets: Vec<Target>,
    status: HashMap<Pair, Status>,
    /// What Claude Code and Codex profiles report, by server name.
    sign_ins: HashMap<usize, HashMap<String, SignIn>>,
    checking: HashSet<usize>,
    clis: HashMap<&'static str, PathBuf>,
    programs: HashMap<&'static str, PathBuf>,
    region: String,
    busy: HashSet<Pair>,
    /// Stops the install or removal running for a pair.
    stops: HashMap<Pair, AbortHandle>,
    errors: HashMap<Pair, String>,
    loading: bool,
    sheet: Option<Sheet>,
    focus: FocusHandle,
    /// Made on the first render, which has the window an input needs.
    search: Option<Entity<InputState>>,
    _search: Option<Subscription>,
}

enum Sheet {
    Install(InstallSheet),
    SignIn(SignInFlow),
}

struct InstallSheet {
    server: usize,
    selected: HashSet<usize>,
    region: Option<Entity<InputState>>,
    _subscription: Option<Subscription>,
}

#[derive(Clone, PartialEq)]
enum Phase {
    Starting,
    Waiting,
    /// The address was typed into the client's prompt.
    Finishing,
    Failed(String),
}

struct SignInFlow {
    queue: Vec<Pair>,
    pos: usize,
    done: Vec<Pair>,
    link: Option<String>,
    address: Entity<InputState>,
    phase: Phase,
    /// The client's answer to a pasted address, or why it wasn't sent.
    problem: Option<String>,
    copied: bool,
    login: Option<Login>,
    _task: Option<Task<()>>,
    _subscription: Subscription,
}

/// The logo of a client: colour logos as images, one-colour ones tinted
/// with the theme, and a neutral glyph for clients without a free logo.
fn client_logo(client: Client, cx: &App) -> AnyElement {
    let logo = match client {
        Client::ClaudeCode => Some(("claudecode-color", true)),
        Client::ClaudeDesktop => Some(("claude-color", true)),
        Client::Codex => Some(("codex-color", true)),
        Client::Gemini => Some(("geminicli-color", true)),
        Client::Qwen => Some(("qwen-color", true)),
        Client::Amp => Some(("amp-color", true)),
        Client::Kiro => Some(("kiro-color", true)),
        Client::Junie => Some(("junie-color", true)),
        Client::Antigravity => Some(("antigravity-color", true)),
        Client::Trae => Some(("trae-color", true)),
        Client::OpenCode => Some(("opencode", false)),
        Client::CopilotCli => Some(("githubcopilot", false)),
        Client::Cline => Some(("cline", false)),
        Client::Goose => Some(("goose", false)),
        Client::Cursor => Some(("cursor", false)),
        Client::Windsurf => Some(("windsurf", false)),
        Client::Zed => Some(("zed", false)),
        Client::Warp => Some(("warp", false)),
        Client::RooCode => Some(("roocode", false)),
        Client::KiloCode => Some(("kilocode", false)),
        Client::LmStudio => Some(("lmstudio", false)),
        _ => None,
    };
    logo_image(logo, Lucide::Bot, 18., cx)
}

fn server_logo(server: &Server, cx: &App) -> AnyElement {
    if server.id == sawit_core::mcp_clients::SERVER_NAME {
        return img(crate::assets::LOGO_MARK)
            .size(px(20.))
            .flex_none()
            .into_any_element();
    }
    let glyph = if server.is_remote() {
        Lucide::Globe
    } else {
        Lucide::SquareTerminal
    };
    logo_image(server.logo, glyph, 20., cx)
}

fn logo_image(logo: Option<(&str, bool)>, glyph: Lucide, size: f32, cx: &App) -> AnyElement {
    let Some((name, colour)) = logo else {
        // Same box as the logos, so names line up.
        return div()
            .size(px(size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::new(glyph)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            )
            .into_any_element();
    };
    let path: SharedString = format!("agent_logos/{name}.svg").into();
    if colour {
        img(path).size(px(size)).flex_none().into_any_element()
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

/// "Claude Code · work", or the client alone for its only profile.
fn target_label(t: &Target) -> String {
    if t.profile == "default" {
        t.client.name().to_string()
    } else {
        format!("{} · {}", t.client.name(), t.profile)
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl McpPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let server = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("sawit-mcp")))
            .unwrap_or_else(|| PathBuf::from("sawit-mcp"));
        let mut this = Self {
            server_exists: false,
            server,
            targets: Vec::new(),
            status: HashMap::new(),
            sign_ins: HashMap::new(),
            checking: HashSet::new(),
            clis: HashMap::new(),
            programs: HashMap::new(),
            region: DEFAULT_AWS_REGION.into(),
            busy: HashSet::new(),
            stops: HashMap::new(),
            errors: HashMap::new(),
            loading: true,
            sheet: None,
            focus: cx.focus_handle(),
            search: None,
            _search: None,
        };
        this.reload(cx);
        this
    }

    fn spec(&self, ix: usize) -> Spec {
        let programs = self.programs.clone();
        SERVERS[ix].spec(&self.server, &self.region, move |c| {
            programs.get(c).cloned()
        })
    }

    fn cli_for(&self, client: Client) -> Option<PathBuf> {
        client.cli().and_then(|n| self.clis.get(n).cloned())
    }

    /// Re-detects clients and re-reads every config (off the UI thread),
    /// then asks Claude Code and Codex which servers need a sign-in.
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let server = self.server.clone();
        let region = self.region.clone();
        let rx = rt::spawn(cx, async move {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
            let targets = discover(&home, claude_dir);
            let programs: HashMap<&'static str, PathBuf> = PROGRAMS
                .into_iter()
                .filter_map(|n| find_in_path(n).map(|p| (n, p)))
                .collect();
            let mut statuses = HashMap::new();
            for (s, server_entry) in SERVERS.iter().enumerate() {
                let spec = server_entry.spec(&server, &region, |c| programs.get(c).cloned());
                for (t, target) in targets.iter().enumerate() {
                    statuses.insert((s, t), status(target, &spec));
                }
            }
            let clis: HashMap<&'static str, PathBuf> = ["claude", "codex"]
                .into_iter()
                .filter_map(|n| find_in_path(n).map(|p| (n, p)))
                .collect();
            (targets, statuses, clis, programs, server.is_file())
        });
        cx.spawn(async move |this, cx| {
            if let Ok((targets, statuses, clis, programs, exists)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.targets = targets;
                    this.status = statuses;
                    this.clis = clis;
                    this.programs = programs;
                    this.server_exists = exists;
                    this.sign_ins.clear();
                    this.errors.clear();
                    this.loading = false;
                    let all: Vec<usize> = (0..this.targets.len()).collect();
                    this.check_sign_ins(all, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Asks each Claude Code / Codex profile in `targets` that has a remote
    /// catalog server which servers are signed in.
    fn check_sign_ins(&mut self, targets: Vec<usize>, cx: &mut Context<Self>) {
        for t in targets {
            let target = self.targets[t].clone();
            let Some(cli) = self.cli_for(target.client) else {
                continue;
            };
            if target.client.profile_var().is_none()
                || self.checking.contains(&t)
                || !SERVERS
                    .iter()
                    .enumerate()
                    .any(|(s, server)| server.auth == Auth::Browser && self.is_installed((s, t)))
            {
                continue;
            }
            self.checking.insert(t);
            let rx = rt::spawn(cx, async move { sign_in_states(&target, &cli).await });
            cx.spawn(async move |this, cx| {
                let result = rx.await;
                let _ = this.update(cx, |this, cx| {
                    this.checking.remove(&t);
                    if let Ok(Ok(states)) = result {
                        this.sign_ins.insert(t, states);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn is_installed(&self, pair: Pair) -> bool {
        matches!(
            self.status.get(&pair),
            Some(Status::Installed | Status::Outdated(_))
        )
    }

    /// Whether this profile reported that the server needs a sign-in.
    fn needs_sign_in(&self, (s, t): Pair) -> bool {
        SERVERS[s].auth == Auth::Browser
            && self.is_installed((s, t))
            && self.sign_ins.get(&t).and_then(|m| m.get(SERVERS[s].id)) == Some(&SignIn::Needed)
    }

    fn pending_sign_ins(&self, server: Option<usize>) -> Vec<Pair> {
        let mut out = Vec::new();
        for s in 0..SERVERS.len() {
            if server.is_some_and(|x| x != s) {
                continue;
            }
            for t in 0..self.targets.len() {
                if self.needs_sign_in((s, t)) && self.cli_for(self.targets[t].client).is_some() {
                    out.push((s, t));
                }
            }
        }
        out
    }

    fn act(&mut self, pair: Pair, remove: bool, cx: &mut Context<Self>) {
        let (s, t) = pair;
        let Some(target) = self.targets.get(t).cloned() else {
            return;
        };
        if !self.busy.insert(pair) {
            return;
        }
        self.errors.remove(&pair);
        let spec = self.spec(s);
        let cli = self.cli_for(target.client);
        let (run, stop) = abortable({
            let (target, spec) = (target.clone(), spec.clone());
            async move {
                if remove {
                    uninstall(&target, &spec.name, cli.as_deref()).await
                } else {
                    install(&target, &spec, cli.as_deref()).await
                }
            }
        });
        self.stops.insert(pair, stop);
        let rx = rt::spawn(cx, async move {
            // `None` when it was stopped; the config is read again either way.
            let result = run.await.ok();
            (result, status(&target, &spec))
        });
        cx.spawn(async move |this, cx| {
            if let Ok((result, new_status)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.busy.remove(&pair);
                    this.stops.remove(&pair);
                    match result {
                        Some(Err(e)) => {
                            this.errors.insert(pair, e.to_string());
                        }
                        None => {
                            this.errors.insert(pair, "Stopped.".into());
                        }
                        Some(Ok(())) => {}
                    }
                    this.status.insert(pair, new_status);
                    if !remove && SERVERS[s].auth == Auth::Browser {
                        if let Some(states) = this.sign_ins.get_mut(&t) {
                            states.remove(SERVERS[s].id);
                        }
                        this.check_sign_ins(vec![t], cx);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    // Install sheet

    fn open_install(&mut self, s: usize, window: &mut Window, cx: &mut Context<Self>) {
        let selected = (0..self.targets.len())
            .filter(|t| self.is_installed((s, *t)))
            .collect();
        let (region, subscription) = if SERVERS[s].has_region() {
            let region = self.region.clone();
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(DEFAULT_AWS_REGION);
                state.set_value(region, window, cx);
                state
            });
            let sub = cx.subscribe_in(&input, window, |this, input, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::Change) {
                    let value = input.read(cx).value().trim().to_string();
                    this.set_region(
                        if value.is_empty() {
                            DEFAULT_AWS_REGION.into()
                        } else {
                            value
                        },
                        cx,
                    );
                }
            });
            (Some(input), Some(sub))
        } else {
            (None, None)
        };
        self.sheet = Some(Sheet::Install(InstallSheet {
            server: s,
            selected,
            region,
            _subscription: subscription,
        }));
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// A new region re-reads the statuses: installs elsewhere become "Other
    /// region".
    fn set_region(&mut self, region: String, cx: &mut Context<Self>) {
        if region == self.region {
            return;
        }
        self.region = region;
        for (s, server) in SERVERS.iter().enumerate() {
            if server.has_region() {
                let spec = self.spec(s);
                for (t, target) in self.targets.iter().enumerate() {
                    self.status.insert((s, t), status(target, &spec));
                }
            }
        }
        cx.notify();
    }

    /// Stops every install or removal running for server `s`.
    fn stop(&mut self, s: usize, cx: &mut Context<Self>) {
        for (_, stop) in self.stops.iter().filter(|((x, _), _)| *x == s) {
            stop.abort();
        }
        cx.notify();
    }

    /// The targets server `s` can be removed from here (not the ones edited
    /// by hand).
    fn removable(&self, s: usize) -> Vec<usize> {
        (0..self.targets.len())
            .filter(|&t| {
                self.targets[t].client.editable()
                    && matches!(
                        self.status.get(&(s, t)),
                        Some(Status::Installed | Status::Outdated(_))
                    )
            })
            .collect()
    }

    /// Asks, then removes server `s` from every client it's installed in.
    fn confirm_uninstall(&mut self, s: usize, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.removable(s);
        if targets.is_empty() {
            return;
        }
        let names: Vec<String> = targets
            .iter()
            .map(|&t| target_label(&self.targets[t]))
            .collect();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Uninstall {} from {}?",
                SERVERS[s].name,
                plural(targets.len(), "client", "clients")
            ),
            Some(&format!(
                "Its entry is removed from: {}. Restart those clients to drop it.",
                names.join(", ")
            )),
            &["Uninstall", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    for t in targets {
                        this.act((s, t), true, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn install_selected(&mut self, remove: bool, cx: &mut Context<Self>) {
        let Some(Sheet::Install(sheet)) = &self.sheet else {
            return;
        };
        let s = sheet.server;
        let mut chosen: Vec<usize> = sheet.selected.iter().copied().collect();
        chosen.sort();
        for t in chosen {
            let editable = self.targets[t].client.editable();
            let todo = match self.status.get(&(s, t)) {
                Some(Status::NotInstalled | Status::Outdated(_)) => !remove && editable,
                Some(Status::Installed) => remove && editable,
                _ => false,
            };
            if todo || (remove && matches!(self.status.get(&(s, t)), Some(Status::Outdated(_)))) {
                self.act((s, t), remove, cx);
            }
        }
    }

    // Sign-in flow

    fn start_sign_in(&mut self, queue: Vec<Pair>, window: &mut Window, cx: &mut Context<Self>) {
        if queue.is_empty() {
            return;
        }
        let address = cx.new(|cx| {
            InputState::new(window, cx).placeholder("http://localhost:…/callback?code=…")
        });
        let subscription =
            cx.subscribe_in(&address, window, |this, _, e: &InputEvent, _, cx| match e {
                InputEvent::PressEnter { .. } => this.submit_address(cx),
                InputEvent::Change => {
                    if let Some(Sheet::SignIn(flow)) = &mut this.sheet {
                        flow.problem = None;
                        cx.notify();
                    }
                }
                _ => {}
            });
        self.sheet = Some(Sheet::SignIn(SignInFlow {
            queue,
            pos: 0,
            done: Vec::new(),
            link: None,
            address,
            phase: Phase::Starting,
            problem: None,
            copied: false,
            login: None,
            _task: None,
            _subscription: subscription,
        }));
        self.focus.focus(window, cx);
        self.begin_current(window, cx);
    }

    /// Starts the client's sign-in for the flow's current server and profile.
    fn begin_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::SignIn(flow)) = &self.sheet else {
            return;
        };
        let Some(&(s, t)) = flow.queue.get(flow.pos) else {
            return;
        };
        let target = self.targets[t].clone();
        let cli = self.cli_for(target.client);
        let started = cli
            .and_then(|cli| login_call(&target, &cli, SERVERS[s].id))
            .ok_or_else(|| {
                format!(
                    "{} isn't installed, so it can't sign in here.",
                    target.client.name()
                )
            })
            .and_then(|call| {
                mcp_auth::start(&call).map_err(|e| format!("Couldn't start the sign-in: {e}"))
            });
        let Some(Sheet::SignIn(flow)) = &mut self.sheet else {
            return;
        };
        flow.link = None;
        flow.problem = None;
        flow.copied = false;
        flow.address.update(cx, |s, cx| s.set_value("", window, cx));
        match started {
            Err(e) => {
                flow.phase = Phase::Failed(e);
                flow.login = None;
            }
            Ok((login, mut events)) => {
                flow.phase = Phase::Starting;
                flow.login = Some(login);
                flow._task = Some(cx.spawn_in(window, async move |this, cx| {
                    while let Some(event) = events.next().await {
                        let ok = this.update_in(cx, |this, window, cx| {
                            this.on_login_event((s, t), event, window, cx)
                        });
                        if ok.is_err() {
                            break;
                        }
                    }
                }));
            }
        }
        cx.notify();
    }

    fn on_login_event(
        &mut self,
        pair: Pair,
        event: LoginEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Sheet::SignIn(flow)) = &mut self.sheet else {
            return;
        };
        if flow.queue.get(flow.pos) != Some(&pair) {
            return;
        }
        match event {
            LoginEvent::Link(url) => {
                flow.link = Some(url);
                flow.phase = Phase::Waiting;
            }
            LoginEvent::Message(line) => {
                if flow.phase == Phase::Finishing {
                    // The client rejected the address and asks again.
                    flow.problem = Some(line);
                    flow.phase = Phase::Waiting;
                }
            }
            LoginEvent::Exited { success, message } => {
                flow.login = None;
                if success {
                    let (s, t) = pair;
                    self.sign_ins
                        .entry(t)
                        .or_default()
                        .insert(SERVERS[s].id.to_string(), SignIn::SignedIn);
                    self.advance(window, cx);
                    return;
                }
                flow.phase = Phase::Failed(if message.is_empty() {
                    "The sign-in stopped before it finished.".into()
                } else {
                    message
                });
            }
        }
        cx.notify();
    }

    /// Moves to the next server and profile, or ends the flow.
    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::SignIn(flow)) = &mut self.sheet else {
            return;
        };
        if let Some(&pair) = flow.queue.get(flow.pos)
            && flow.phase != Phase::Starting
            && !matches!(flow.phase, Phase::Failed(_))
        {
            flow.done.push(pair);
        }
        flow.login = None;
        flow.pos += 1;
        if flow.pos < flow.queue.len() {
            self.begin_current(window, cx);
        } else {
            flow.link = None;
            cx.notify();
        }
    }

    fn skip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Sheet::SignIn(flow)) = &mut self.sheet {
            flow.phase = Phase::Starting;
        }
        self.advance(window, cx);
    }

    fn submit_address(&mut self, cx: &mut Context<Self>) {
        let Some(Sheet::SignIn(flow)) = &mut self.sheet else {
            return;
        };
        let address = flow.address.read(cx).value().trim().to_string();
        if address.is_empty() || flow.phase != Phase::Waiting {
            return;
        }
        if !is_callback_url(&address) {
            flow.problem = Some(
                "That isn't the address the browser ended on. It starts with http://localhost or http://127.0.0.1 and has code= in it."
                    .into(),
            );
        } else if let Some(login) = &flow.login {
            match login.submit(&address) {
                Ok(()) => {
                    flow.phase = Phase::Finishing;
                    flow.problem = None;
                }
                Err(e) => flow.problem = Some(format!("Couldn't pass the address on: {e}")),
            }
        }
        cx.notify();
    }

    fn close_sheet(&mut self, cx: &mut Context<Self>) {
        let rechecks: Vec<usize> = match self.sheet.take() {
            Some(Sheet::SignIn(flow)) => {
                let mut t: Vec<usize> = flow.queue.iter().map(|(_, t)| *t).collect();
                t.sort();
                t.dedup();
                t
            }
            _ => Vec::new(),
        };
        // Signed-in states come from the clients, not from our guess.
        if !rechecks.is_empty() {
            self.check_sign_ins(rechecks, cx);
        }
        cx.notify();
    }

    // Rendering

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let pending = self.pending_sign_ins(None).len();
        let checking = !self.checking.is_empty();
        let agents = {
            let mut counts: Vec<(Client, usize)> = Vec::new();
            for t in &self.targets {
                match counts.iter_mut().find(|(c, _)| *c == t.client) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((t.client, 1)),
                }
            }
            counts.sort_by_key(|(c, _)| {
                (
                    c.category() == Category::Editor,
                    Client::ALL.iter().position(|x| x == c),
                )
            });
            counts
                .into_iter()
                .map(|(c, n)| {
                    if n > 1 {
                        format!("{} ×{n}", c.name())
                    } else {
                        c.name().to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" · ")
        };
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_lg().font_semibold().flex_1().child("MCP Servers"))
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
                    .when(checking, |el| {
                        el.child(
                            h_flex()
                                .gap_1p5()
                                .text_xs()
                                .text_color(muted)
                                .child(Spinner::new().xsmall().color(muted))
                                .child("Checking sign-ins…"),
                        )
                    })
                    .child(
                        Button::new("mcp-reload")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Detect Again")
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .when(pending > 0, |el| {
                        el.child(
                            Button::new("mcp-sign-in-all")
                                .primary()
                                .small()
                                .icon(Lucide::KeyRound)
                                .label(format!("Sign In to All ({pending})"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let queue = this.pending_sign_ins(None);
                                    this.start_sign_in(queue, window, cx)
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .whitespace_normal()
                    .child(
                        "Install a server into every agent, editor and profile at once. Servers that need an account \
                         sign in through the agent's own sign-in; restart the client (or the session) to pick up changes.",
                    ),
            )
            .when(!self.loading, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .whitespace_normal()
                        .child(if self.targets.is_empty() {
                            "No AI clients found.".to_string()
                        } else {
                            format!("{} on this Mac: {agents}", plural(self.targets.len(), "target", "targets"))
                        }),
                )
            })
    }

    fn render_server(&self, s: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let server = &SERVERS[s];
        let total = self.targets.len();
        let installed = (0..total)
            .filter(|t| self.status.get(&(s, *t)) == Some(&Status::Installed))
            .count();
        let other = (0..total)
            .filter(|t| matches!(self.status.get(&(s, *t)), Some(Status::Outdated(_))))
            .count();
        let to_sign_in = self.pending_sign_ins(Some(s)).len();
        let checking = server.auth == Auth::Browser
            && (0..total).any(|t| self.checking.contains(&t) && self.is_installed((s, t)));
        let missing = server.missing_program(|c| self.programs.get(c).cloned());
        let bundled_missing = s == 0 && !self.server_exists;
        let busy = (0..total).any(|t| self.busy.contains(&(s, t)));

        let (summary, summary_color) = if installed + other == 0 {
            ("Not installed".to_string(), muted)
        } else if other > 0 {
            (
                format!("{installed} of {total} · {other} to update"),
                theme.warning,
            )
        } else {
            (format!("{installed} of {total}"), theme.success)
        };

        let mut notes: Vec<(Lucide, String, Hsla)> = Vec::new();
        if bundled_missing {
            notes.push((
                Lucide::TriangleAlert,
                format!(
                    "{} not found. Build it with: cargo build --release -p sawit-mcp",
                    tilde(&self.server)
                ),
                theme.danger,
            ));
        }
        if let Some(program) = missing {
            notes.push((
                Lucide::TriangleAlert,
                format!("`{program}` isn't installed, so clients can't start this server."),
                theme.warning,
            ));
        } else if let Some(note) = server.note {
            notes.push((Lucide::Info, note.to_string(), muted));
        }
        if let Auth::EnvKey(var) = server.auth {
            notes.push((
                Lucide::KeyRound,
                format!("Uses an API key: set {var} in the environment the client starts in."),
                muted,
            ));
        }

        h_flex()
            .px_3()
            .py_2p5()
            .gap_3()
            .items_start()
            .border_b_1()
            .border_color(theme.border)
            .child(div().pt_0p5().child(server_logo(server, cx)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().text_sm().font_semibold().child(server.name))
                            .when(server.auth == Auth::Browser, |el| {
                                el.child(
                                    div()
                                        .px_1p5()
                                        .rounded(theme.radius)
                                        .bg(theme.secondary)
                                        .text_xs()
                                        .text_color(muted)
                                        .child("Sign-in"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(server.about),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .font_family(theme.mono_font_family.clone())
                            .whitespace_normal()
                            .child(server.address(&self.region)),
                    )
                    .children(notes.into_iter().map(|(icon, text, color)| {
                        h_flex()
                            .gap_1p5()
                            .items_start()
                            .text_xs()
                            .text_color(color)
                            .child(div().pt(px(1.)).child(Icon::new(icon).xsmall()))
                            .child(div().whitespace_normal().child(text))
                    })),
            )
            .child(
                v_flex()
                    .flex_none()
                    .items_end()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .text_xs()
                                    .text_color(summary_color)
                                    .when(busy, |el| el.child(Spinner::new().xsmall().color(muted)))
                                    .when(!busy, |el| {
                                        el.child(
                                            div().size(px(6.)).rounded_full().bg(summary_color),
                                        )
                                    })
                                    .child(summary),
                            )
                            .when(busy, |el| {
                                el.child(
                                    Button::new(("mcp-stop", s))
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::CircleStop)
                                        .label("Stop")
                                        .tooltip("Stop Installing or Removing")
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| this.stop(s, cx)),
                                        ),
                                )
                            })
                            .when(!busy && !self.removable(s).is_empty(), |el| {
                                el.child(
                                    Button::new(("mcp-uninstall", s))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(Lucide::Trash).text_color(theme.danger))
                                        .tooltip(format!(
                                            "Uninstall {} from Every Client",
                                            server.name
                                        ))
                                        .disabled(self.loading)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.confirm_uninstall(s, window, cx)
                                        })),
                                )
                            })
                            .child(
                                Button::new(("mcp-open", s))
                                    .when(installed + other == 0, |b| b.primary())
                                    .when(installed + other > 0, |b| b.outline())
                                    .xsmall()
                                    .label(if installed + other == 0 {
                                        "Install…"
                                    } else {
                                        "Manage…"
                                    })
                                    .disabled(self.loading || self.targets.is_empty())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_install(s, window, cx)
                                    })),
                            ),
                    )
                    .when(to_sign_in > 0, |el| {
                        el.child(
                            Button::new(("mcp-sign-in", s))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(Lucide::KeyRound).text_color(theme.warning))
                                .label(format!(
                                    "Sign In ({})",
                                    plural(to_sign_in, "profile", "profiles")
                                ))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let queue = this.pending_sign_ins(Some(s));
                                    this.start_sign_in(queue, window, cx)
                                })),
                        )
                    })
                    .when(checking && to_sign_in == 0, |el| {
                        el.child(div().text_xs().text_color(muted).child("Checking sign-in…"))
                    }),
            )
    }

    fn render_install_sheet(&self, sheet: &InstallSheet, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let s = sheet.server;
        let server = &SERVERS[s];
        let spec = self.spec(s);
        let selected = &sheet.selected;
        let count = |pred: &dyn Fn(&Status) -> bool| {
            selected
                .iter()
                .filter(|t| self.targets[**t].client.editable())
                .filter(|t| self.status.get(&(s, **t)).is_some_and(pred))
                .count()
        };
        let to_install = count(&|st| matches!(st, Status::NotInstalled | Status::Outdated(_)));
        let to_remove = count(&|st| matches!(st, Status::Installed | Status::Outdated(_)));
        let to_sign_in: Vec<Pair> = self
            .pending_sign_ins(Some(s))
            .into_iter()
            .filter(|(_, t)| selected.contains(t))
            .collect();
        let busy = (0..self.targets.len()).any(|t| self.busy.contains(&(s, t)));
        let blocked = (s == 0 && !self.server_exists)
            || server
                .missing_program(|c| self.programs.get(c).cloned())
                .is_some();

        // Targets grouped under their client: agents first, then editors.
        let mut groups: Vec<(Client, Vec<usize>)> = Vec::new();
        for (t, target) in self.targets.iter().enumerate() {
            match groups.iter_mut().find(|(c, _)| *c == target.client) {
                Some((_, ts)) => ts.push(t),
                None => groups.push((target.client, vec![t])),
            }
        }
        groups.sort_by_key(|(c, _)| {
            (
                c.category() == Category::Editor,
                Client::ALL.iter().position(|x| x == c),
            )
        });

        let select = |label: &'static str, id: &'static str, pick: fn(&Status) -> bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let n = this.targets.len();
                    let chosen: HashSet<usize> = (0..n)
                        .filter(|t| this.status.get(&(s, *t)).is_some_and(pick))
                        .collect();
                    if let Some(Sheet::Install(sheet)) = &mut this.sheet {
                        sheet.selected = chosen;
                    }
                    cx.notify();
                }))
        };

        let select_buttons = [
            select("All", "mcp-sel-all", |_| true),
            select("Installed", "mcp-sel-installed", |st| {
                matches!(st, Status::Installed | Status::Outdated(_))
            }),
            select("None", "mcp-sel-none", |_| false),
        ];
        let list = v_flex().children(groups.into_iter().map(|(client, ts)| {
            v_flex()
                .child(
                    h_flex()
                        .px_3()
                        .py_1()
                        .gap_2()
                        .bg(theme.secondary)
                        .border_b_1()
                        .border_color(theme.border)
                        .child(client_logo(client, cx))
                        .child(div().text_xs().font_semibold().child(client.name())),
                )
                .children(
                    ts.into_iter()
                        .map(|t| self.render_target_row(s, t, &spec, selected.contains(&t), cx)),
                )
        }));

        let title = format!("Install {}", server.name);
        v_flex()
            .w(px(720.))
            .max_w_full()
            .max_h_full()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .shadow_lg()
            .child(
                v_flex()
                    .p_4()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(server_logo(server, cx))
                            .child(div().text_base().font_semibold().flex_1().child(title))
                            .child(
                                Button::new("mcp-sheet-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::X)
                                    .tooltip("Close")
                                    .on_click(cx.listener(|this, _, _, cx| this.close_sheet(cx))),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(theme.mono_font_family.clone())
                            .text_color(muted)
                            .whitespace_normal()
                            .child(server.address(&self.region)),
                    )
                    .when_some(sheet.region.as_ref(), |el, region| {
                        el.child(
                            h_flex()
                                .pt_2()
                                .gap_2()
                                .child(div().text_sm().child("Region"))
                                .child(div().w(px(180.)).child(Input::new(region).small()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child("The AWS Region the server works in."),
                                ),
                        )
                    }),
            )
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_xs().text_color(muted).pr_1().child("Select"))
                    .children(select_buttons)
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{} selected", selected.len())),
                    ),
            )
            .child(
                div()
                    .id("mcp-sheet-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .when(!to_sign_in.is_empty(), |el| {
                        el.child(
                            Button::new("mcp-sheet-sign-in")
                                .outline()
                                .small()
                                .icon(Lucide::KeyRound)
                                .label(format!("Sign In ({})", to_sign_in.len()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.start_sign_in(to_sign_in.clone(), window, cx)
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("mcp-sheet-remove")
                            .danger()
                            .outline()
                            .small()
                            .label(format!("Remove from {to_remove}"))
                            .disabled(to_remove == 0 || busy)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.install_selected(true, cx)),
                            ),
                    )
                    .child(
                        Button::new("mcp-sheet-install")
                            .primary()
                            .small()
                            .label(format!("Install to {to_install}"))
                            .loading(busy)
                            .disabled(to_install == 0 || busy || blocked)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.install_selected(false, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_target_row(
        &self,
        s: usize,
        t: usize,
        spec: &Spec,
        checked: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let target = &self.targets[t];
        let pair = (s, t);
        let state = self
            .status
            .get(&pair)
            .cloned()
            .unwrap_or(Status::NotInstalled);
        let signed = self
            .sign_ins
            .get(&t)
            .and_then(|m| m.get(SERVERS[s].id))
            .copied();
        let (badge, color): (&str, Hsla) = match &state {
            Status::Installed if self.needs_sign_in(pair) => ("Needs sign-in", theme.warning),
            Status::Installed
                if signed == Some(SignIn::SignedIn) && SERVERS[s].auth == Auth::Browser =>
            {
                ("Signed in", theme.success)
            }
            Status::Installed => ("Installed", theme.success),
            Status::Outdated(_) if SERVERS[s].has_region() => ("Other region", theme.warning),
            Status::Outdated(_) => ("Other version", theme.warning),
            Status::NotInstalled => ("Not installed", muted),
            Status::Unreadable(_) => ("Edit manually", theme.danger),
            Status::Manual => ("Paste by hand", muted),
        };
        let detail = match &state {
            Status::Outdated(other) => Some(format!("Points to {other}")),
            Status::Unreadable(why) => Some(why.clone()),
            _ => None,
        };
        let via = match self.cli_for(target.client) {
            Some(_) => format!("via {}", target.client.cli().unwrap_or_default()),
            None if !target.client.editable() => "by hand".into(),
            None => "edits file".into(),
        };
        let manual = matches!(state, Status::Manual | Status::Unreadable(_))
            || (!target.client.editable() && state == Status::Installed);
        let busy = self.busy.contains(&pair);
        let text = snippet(target, spec);
        let profile_label = if target.profile == "default" && target.client.profile_var().is_none()
        {
            "This Mac".to_string()
        } else {
            target.profile.clone()
        };
        v_flex()
            .px_3()
            .py_1p5()
            .gap_0p5()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().w(px(150.)).flex_none().child(
                            Checkbox::new(("mcp-target", t))
                                .label(profile_label)
                                .checked(checked)
                                .disabled(manual)
                                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                    if let Some(Sheet::Install(sheet)) = &mut this.sheet {
                                        if *checked {
                                            sheet.selected.insert(t);
                                        } else {
                                            sheet.selected.remove(&t);
                                        }
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(
                        div()
                            .id(("mcp-config", t))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .font_family(theme.mono_font_family.clone())
                            .tooltip({
                                let full = target.config.display().to_string();
                                move |window, cx| Tooltip::new(full.clone()).build(window, cx)
                            })
                            .child(tilde(&target.config)),
                    )
                    .child(div().text_xs().text_color(muted).child(via))
                    .child(
                        h_flex()
                            .gap_1()
                            .w(px(104.))
                            .flex_none()
                            .text_xs()
                            .text_color(color)
                            .when(busy, |el| el.child(Spinner::new().xsmall().color(muted)))
                            .when(!busy, |el| {
                                el.child(div().size(px(6.)).rounded_full().bg(color))
                            })
                            .child(badge),
                    )
                    .child(
                        div()
                            .w(px(92.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .when(manual, |el| {
                                el.child(
                                    Button::new(("mcp-copy", t))
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::Copy)
                                        .label("Snippet")
                                        .tooltip("Copy the entry to paste into the config")
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                text.clone(),
                                            ))
                                        }),
                                )
                            })
                            .when(!manual && self.needs_sign_in(pair), |el| {
                                el.child(
                                    Button::new(("mcp-row-sign-in", t))
                                        .ghost()
                                        .xsmall()
                                        .label("Sign In")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.start_sign_in(vec![pair], window, cx)
                                        })),
                                )
                            }),
                    ),
            )
            .when_some(detail, |el, d| {
                el.child(
                    div()
                        .pl(px(158.))
                        .text_xs()
                        .text_color(muted)
                        .whitespace_normal()
                        .child(d),
                )
            })
            .when_some(self.errors.get(&pair).cloned(), |el, e| {
                el.child(
                    div()
                        .pl(px(158.))
                        .text_xs()
                        .text_color(theme.danger)
                        .whitespace_normal()
                        .child(e),
                )
            })
    }

    fn render_sign_in_sheet(&self, flow: &SignInFlow, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let finished = flow.pos >= flow.queue.len();
        let current = flow.queue.get(flow.pos).copied();
        let (s, t) = current
            .or_else(|| flow.queue.last().copied())
            .unwrap_or((0, 0));
        let server = &SERVERS[s];
        let target = &self.targets[t];
        let agent = target.client.name();
        let step = |n: &'static str, title: &'static str| {
            h_flex()
                .gap_2()
                .child(
                    div()
                        .size(px(20.))
                        .flex_none()
                        .rounded_full()
                        .bg(theme.primary)
                        .text_color(theme.primary_foreground)
                        .text_xs()
                        .font_semibold()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(n),
                )
                .child(div().text_sm().font_semibold().child(title))
        };
        let address_ok = is_callback_url(&flow.address.read(cx).value());

        let body = if finished {
            let names: Vec<String> = flow
                .done
                .iter()
                .map(|(s, t)| {
                    format!(
                        "{} on {}",
                        SERVERS[*s].name,
                        target_label(&self.targets[*t])
                    )
                })
                .collect();
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(Icon::new(Lucide::CircleCheck).text_color(theme.success))
                        .child(div().text_sm().font_semibold().child(if names.is_empty() {
                            "Nothing was signed in.".to_string()
                        } else {
                            format!("Signed in: {}", plural(names.len(), "sign-in", "sign-ins"))
                        })),
                )
                .children(names.into_iter().map(|n| {
                    div().pl_6().text_xs().text_color(muted).child(n)
                }))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .whitespace_normal()
                        .child("The clients saved each sign-in themselves. New sessions can use the tools; running sessions pick them up after /mcp or a restart."),
                )
                .into_any_element()
        } else {
            let link_box = match &flow.link {
                Some(link) => div()
                    .id("mcp-link")
                    .p_2()
                    .rounded(theme.radius)
                    .bg(theme.secondary)
                    .text_xs()
                    .font_family(theme.mono_font_family.clone())
                    .whitespace_normal()
                    .line_clamp(3)
                    .child(link.clone())
                    .into_any_element(),
                None => h_flex()
                    .p_2()
                    .gap_2()
                    .rounded(theme.radius)
                    .bg(theme.secondary)
                    .text_xs()
                    .text_color(muted)
                    .child(Spinner::new().xsmall().color(muted))
                    .child(format!("Starting {agent}'s sign-in…"))
                    .into_any_element(),
            };
            let link = flow.link.clone();
            let link_copy = flow.link.clone();
            let (status_icon, status_text, status_color): (AnyElement, String, Hsla) =
                match &flow.phase {
                    Phase::Starting => (
                        Spinner::new().xsmall().color(muted).into_any_element(),
                        "Starting the sign-in…".into(),
                        muted,
                    ),
                    Phase::Waiting => (
                        Spinner::new().xsmall().color(muted).into_any_element(),
                        "Waiting for approval…".into(),
                        muted,
                    ),
                    Phase::Finishing => (
                        Spinner::new().xsmall().color(muted).into_any_element(),
                        "Finishing the sign-in…".into(),
                        muted,
                    ),
                    Phase::Failed(why) => (
                        Icon::new(Lucide::CircleX)
                            .xsmall()
                            .text_color(theme.danger)
                            .into_any_element(),
                        format!("Sign-in didn't finish: {why}"),
                        theme.danger,
                    ),
                };
            let can_paste = flow.phase == Phase::Waiting;
            v_flex()
                .gap_4()
                .child(
                    h_flex()
                        .gap_2()
                        .child(client_logo(target.client, cx))
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child(format!("{} on {}", server.name, target_label(target))),
                        ),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(step("1", "Open the link and approve access"))
                        .child(link_box)
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("mcp-open-link")
                                        .primary()
                                        .small()
                                        .icon(Lucide::ExternalLink)
                                        .label("Open in Browser")
                                        .disabled(link.is_none())
                                        .on_click(move |_, _, cx| {
                                            if let Some(link) = &link {
                                                cx.open_url(link);
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("mcp-copy-link")
                                        .outline()
                                        .small()
                                        .icon(if flow.copied { Lucide::Check } else { Lucide::Copy })
                                        .label(if flow.copied { "Copied" } else { "Copy Link" })
                                        .disabled(link_copy.is_none())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(link) = &link_copy {
                                                cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                                            }
                                            if let Some(Sheet::SignIn(flow)) = &mut this.sheet {
                                                flow.copied = true;
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .whitespace_normal()
                                .child(format!(
                                    "Opened on this Mac, the sign-in finishes here on its own after you approve. Sign in to {} with the account you want this profile to use.",
                                    server.name
                                )),
                        ),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(step("2", "Or paste the address the browser ends on"))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().flex_1().child(Input::new(&flow.address).disabled(!can_paste)))
                                .child(
                                    Button::new("mcp-complete")
                                        .outline()
                                        .label("Complete Sign-In")
                                        .loading(flow.phase == Phase::Finishing)
                                        .disabled(!can_paste || !address_ok)
                                        .on_click(cx.listener(|this, _, _, cx| this.submit_address(cx))),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .whitespace_normal()
                                .child("Used the link on another device, or the page says it can't connect? Copy the whole address from that browser's address bar and paste it here."),
                        )
                        .when_some(flow.problem.clone(), |el, p| {
                            el.child(
                                h_flex()
                                    .gap_1p5()
                                    .items_start()
                                    .text_xs()
                                    .text_color(theme.danger)
                                    .child(div().pt(px(1.)).child(Icon::new(Lucide::TriangleAlert).xsmall()))
                                    .child(div().whitespace_normal().child(p)),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_start()
                        .text_xs()
                        .text_color(status_color)
                        .child(div().pt(px(1.)).child(status_icon))
                        .child(div().flex_1().whitespace_normal().child(status_text))
                        .when(matches!(flow.phase, Phase::Failed(_)), |el| {
                            el.child(
                                Button::new("mcp-retry")
                                    .outline()
                                    .xsmall()
                                    .icon(Lucide::RefreshCw)
                                    .label("Try Again")
                                    .on_click(cx.listener(|this, _, window, cx| this.begin_current(window, cx))),
                            )
                        }),
                )
                .into_any_element()
        };

        let more = flow.queue.len() > 1;
        v_flex()
            .w(px(640.))
            .max_w_full()
            .max_h_full()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .shadow_lg()
            .child(
                v_flex()
                    .p_4()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_base()
                                    .font_semibold()
                                    .flex_1()
                                    .child(if more && !finished {
                                        format!("Sign In ({} of {})", flow.pos + 1, flow.queue.len())
                                    } else {
                                        format!("Sign In to {}", server.name)
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(format!(
                                "Runs {agent}'s own sign-in for each profile. Sawit never sees your password or tokens."
                            )),
                    ),
            )
            .child(
                div()
                    .id("mcp-sign-in-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(body),
            )
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(div().flex_1())
                    .when(!finished && flow.pos + 1 < flow.queue.len(), |el| {
                        el.child(
                            Button::new("mcp-skip")
                                .ghost()
                                .small()
                                .label("Skip This Profile")
                                .on_click(cx.listener(|this, _, window, cx| this.skip(window, cx))),
                        )
                    })
                    .child(
                        Button::new("mcp-sign-in-close")
                            .when(finished, |b| b.primary())
                            .when(!finished, |b| b.outline())
                            .small()
                            .label(if finished { "Done" } else { "Cancel" })
                            .on_click(cx.listener(|this, _, _, cx| this.close_sheet(cx))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for McpPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.search.is_none() {
            let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search servers"));
            self._search = Some(cx.subscribe(&search, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }));
            self.search = Some(search);
        }
        let muted = cx.theme().muted_foreground;
        let query = self
            .search
            .as_ref()
            .map(|s| s.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let shown = |s: &usize| {
            let server = &SERVERS[*s];
            let has = |t: &str| t.to_lowercase().contains(&query);
            query.is_empty() || has(server.name) || has(server.id) || has(server.about)
        };
        let body = if self.loading {
            h_flex()
                .gap_2()
                .text_sm()
                .text_color(muted)
                .child(Spinner::new().small().color(muted))
                .child("Detecting clients…")
                .into_any_element()
        } else {
            let (bundled, local): (Vec<usize>, Vec<usize>) = (0..SERVERS.len())
                .filter(|s| !SERVERS[*s].is_remote() && shown(s))
                .partition(|s| *s == 0);
            let remote: Vec<usize> = (0..SERVERS.len())
                .filter(|s| SERVERS[*s].is_remote() && shown(s))
                .collect();
            let none = bundled.is_empty() && remote.is_empty() && local.is_empty();
            let section = |title: &'static str, ixs: Vec<usize>, cx: &mut Context<Self>| {
                (!ixs.is_empty()).then(|| {
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(muted)
                                .child(title),
                        )
                        .child(
                            v_flex()
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(cx.theme().border)
                                .overflow_hidden()
                                .children(ixs.into_iter().map(|s| self.render_server(s, cx))),
                        )
                })
            };
            v_flex()
                .gap_4()
                .children(section("BUILT IN", bundled, cx))
                .children(section("REMOTE", remote, cx))
                .children(section("RUNS ON THIS MAC", local, cx))
                .when(none, |el| {
                    el.child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("No servers match “{query}”.")),
                    )
                })
                .into_any_element()
        };
        let overlay = match &self.sheet {
            Some(Sheet::Install(sheet)) => Some(self.render_install_sheet(sheet, cx)),
            Some(Sheet::SignIn(flow)) => Some(self.render_sign_in_sheet(flow, cx)),
            None => None,
        };
        div()
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .child(
                div().id("mcp-page").size_full().overflow_y_scroll().child(
                    v_flex()
                        .p_6()
                        .gap_5()
                        .max_w(px(980.))
                        .child(self.render_header(cx))
                        .child(body),
                ),
            )
            .when_some(overlay, |el, sheet| {
                el.child(
                    div()
                        .id("mcp-sheet")
                        .track_focus(&self.focus)
                        .absolute()
                        .inset_0()
                        .p_6()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(gpui_kit::black().opacity(0.45))
                        .occlude()
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                            if e.keystroke.key == "escape" {
                                this.close_sheet(cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(sheet),
                )
            })
    }
}
