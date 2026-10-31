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
use sawit_core::dev_tools::find_program;
use sawit_core::mcp_auth::{
    self, Login, LoginEvent, SignIn, is_callback_url, login_call, sign_in_states,
};
use sawit_core::mcp_catalog::{Auth, DEFAULT_AWS_REGION, SERVERS, Server, valid_region};
use sawit_core::mcp_clients::{
    Category, Client, Entries, Spec, Status, Target, discover, install, needs_bridge, read_entries,
    snippet, status_in, uninstall,
};

use crate::catalog::{self, Mark, tilde};
use crate::rt;

/// A server in the catalog and a target, by index.
type Pair = (usize, usize);

/// What a detection finds, off the UI thread.
struct Detected {
    targets: Vec<Target>,
    /// Each target's config, read once.
    entries: Vec<Entries>,
    clis: HashMap<&'static str, PathBuf>,
    programs: HashMap<&'static str, PathBuf>,
    server_exists: bool,
}

/// Programs local servers and the remote bridge run.
const PROGRAMS: [&str; 2] = ["npx", "cap"];

pub struct McpPage {
    /// `sawit-mcp` next to the running app (inside the .app bundle, or the
    /// cargo target folder during development).
    server: PathBuf,
    server_exists: bool,
    targets: Vec<Target>,
    /// Each target's config as the last detection read it, by target.
    entries: Vec<Entries>,
    status: HashMap<Pair, Status>,
    /// What Claude Code and Codex profiles report, by server name.
    sign_ins: HashMap<usize, HashMap<String, SignIn>>,
    checking: HashSet<usize>,
    clis: HashMap<&'static str, PathBuf>,
    programs: HashMap<&'static str, PathBuf>,
    region: String,
    /// The Region was typed here: a detection no longer takes it from the
    /// installed entries.
    region_chosen: bool,
    busy: HashSet<Pair>,
    /// Stops the install or removal running for a pair.
    stops: HashMap<Pair, AbortHandle>,
    errors: HashMap<Pair, String>,
    loading: bool,
    sheet: Option<Sheet>,
    focus: FocusHandle,
    filter: Filter,
    kind: Option<Kind>,
    sort: Sort,
    /// The page of the list shown, from 0.
    page: usize,
    /// Made on the first render, which has the window an input needs.
    search: Option<Entity<InputState>>,
    _search: Option<Subscription>,
}

/// Which servers the filter tabs show.
#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Installed,
    Missing,
    SignIn,
}

impl Filter {
    const ALL: [Filter; 4] = [
        Filter::All,
        Filter::Installed,
        Filter::Missing,
        Filter::SignIn,
    ];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Installed => "Installed",
            Filter::Missing => "Not Installed",
            Filter::SignIn => "Needs Sign-In",
        }
    }
}

/// Where a server runs; also the catalog's order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    BuiltIn,
    Remote,
    Local,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::BuiltIn, Kind::Remote, Kind::Local];

    fn of(s: usize) -> Kind {
        if SERVERS[s].is_remote() {
            Kind::Remote
        } else if s == 0 {
            Kind::BuiltIn
        } else {
            Kind::Local
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::BuiltIn => "Built In",
            Kind::Remote => "Remote",
            Kind::Local => "Runs on This Mac",
        }
    }

    fn icon(self) -> Lucide {
        match self {
            Kind::BuiltIn => Lucide::Package,
            Kind::Remote => Lucide::Globe,
            Kind::Local => Lucide::SquareTerminal,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Catalog,
    Name,
    NameDesc,
    Installed,
}

impl Sort {
    const ALL: [Sort; 4] = [Sort::Catalog, Sort::Name, Sort::NameDesc, Sort::Installed];

    fn label(self) -> &'static str {
        match self {
            Sort::Catalog => "Type",
            Sort::Name => "Name (A–Z)",
            Sort::NameDesc => "Name (Z–A)",
            Sort::Installed => "Most Installed",
        }
    }
}

enum Sheet {
    Install(InstallSheet),
    SignIn(SignInFlow),
}

struct InstallSheet {
    server: usize,
    selected: HashSet<usize>,
    region: Option<Entity<InputState>>,
    /// What's typed as the Region isn't one.
    region_invalid: bool,
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
pub(crate) fn client_logo(client: Client, size: f32, cx: &App) -> AnyElement {
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
    logo_image(logo, Lucide::Bot, size, cx)
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
        let mut this = Self::blank(cx);
        this.reload(cx);
        this
    }

    /// The page before anything is detected.
    fn blank(cx: &mut Context<Self>) -> Self {
        let server = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("sawit-mcp")))
            .unwrap_or_else(|| PathBuf::from("sawit-mcp"));
        Self {
            server_exists: false,
            server,
            targets: Vec::new(),
            entries: Vec::new(),
            status: HashMap::new(),
            sign_ins: HashMap::new(),
            checking: HashSet::new(),
            clis: HashMap::new(),
            programs: HashMap::new(),
            region: DEFAULT_AWS_REGION.into(),
            region_chosen: false,
            busy: HashSet::new(),
            stops: HashMap::new(),
            errors: HashMap::new(),
            loading: true,
            sheet: None,
            focus: cx.focus_handle(),
            filter: Filter::All,
            kind: None,
            sort: Sort::Catalog,
            page: 0,
            search: None,
            _search: None,
        }
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

    /// Re-detects clients and reads every config once (off the UI thread and
    /// off the async workers), then asks Claude Code and Codex which servers
    /// need a sign-in.
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let server = self.server.clone();
        let rx = rt::spawn_blocking(cx, move || {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
            let targets = discover(&home, claude_dir);
            Detected {
                entries: targets.iter().map(read_entries).collect(),
                targets,
                // Where the Dev Tools page finds them, not only on the app's
                // PATH: that misses what `~/.zshrc` adds (nvm, ~/.local/bin).
                programs: PROGRAMS
                    .into_iter()
                    .filter_map(|n| find_program(n).map(|p| (n, p)))
                    .collect(),
                clis: ["claude", "codex"]
                    .into_iter()
                    .filter_map(|n| find_program(n).map(|p| (n, p)))
                    .collect(),
                server_exists: server.is_file(),
            }
        });
        cx.spawn(async move |this, cx| {
            if let Ok(detected) = rx.await {
                let _ = this.update(cx, |this, cx| this.apply(detected, cx));
            }
        })
        .detach();
    }

    /// Takes in what a detection found.
    fn apply(&mut self, detected: Detected, cx: &mut Context<Self>) {
        self.targets = detected.targets;
        self.entries = detected.entries;
        self.clis = detected.clis;
        self.programs = detected.programs;
        self.server_exists = detected.server_exists;
        if !self.region_chosen
            && let Some(region) = self.installed_region()
        {
            self.region = region;
        }
        self.recompute();
        self.sign_ins.clear();
        self.errors.clear();
        self.loading = false;
        let all: Vec<usize> = (0..self.targets.len()).collect();
        self.check_sign_ins(all, cx);
        cx.notify();
    }

    /// The AWS Region most clients have the server in, if any has it: the
    /// page shows that one, so an install there reads as installed, not as
    /// out of date. A tie keeps the page's.
    fn installed_region(&self) -> Option<String> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for server in SERVERS.iter().filter(|s| s.has_region()) {
            for read in &self.entries {
                if let Entries::Servers(map) = read
                    && let Some(region) = map.get(server.id).and_then(|sig| server.region_in(sig))
                {
                    match counts.iter_mut().find(|(r, _)| *r == region) {
                        Some((_, n)) => *n += 1,
                        None => counts.push((region, 1)),
                    }
                }
            }
        }
        let most = counts.iter().map(|(_, n)| *n).max()?;
        counts
            .iter()
            .find(|(r, n)| *n == most && *r == self.region)
            .or_else(|| counts.iter().find(|(_, n)| *n == most))
            .map(|(r, _)| r.clone())
    }

    /// The Region a client has a regional server in, when it's another one
    /// than the page's: installed there, not out of date.
    fn other_region(&self, (s, t): Pair) -> Option<String> {
        let server = &SERVERS[s];
        match self.status.get(&(s, t)) {
            Some(Status::Outdated(sig)) if server.has_region() => server.region_in(sig),
            _ => None,
        }
    }

    /// A remote server can't go into a client that only runs programs: it
    /// would get the `mcp-remote` bridge, and `npx` to run it isn't found.
    fn bridge_blocked(&self, (s, t): Pair) -> bool {
        SERVERS[s].is_remote()
            && !self.programs.contains_key("npx")
            && self.targets.get(t).is_some_and(|x| needs_bridge(x.client))
    }

    /// Every status, from the configs as read.
    fn recompute(&mut self) {
        self.status.clear();
        for s in 0..SERVERS.len() {
            let spec = self.spec(s);
            for (t, read) in self.entries.iter().enumerate() {
                self.status.insert((s, t), status_in(read, &spec));
            }
        }
    }

    /// Asks each Claude Code / Codex profile in `targets` that has a remote
    /// catalog server which servers are signed in.
    fn check_sign_ins(&mut self, targets: Vec<usize>, cx: &mut Context<Self>) {
        for t in targets {
            let Some(target) = self.targets.get(t).cloned() else {
                continue;
            };
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
        // The client, not its place in the list: a detection can move it.
        let id = (target.client, target.config.clone());
        let rx = rt::spawn(cx, async move {
            // `None` when it was stopped; the config is read again either way.
            let result = run.await.ok();
            (result, read_entries(&target))
        });
        cx.spawn(async move |this, cx| {
            if let Ok((result, read)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.busy.remove(&pair);
                    this.stops.remove(&pair);
                    let Some(t) = this
                        .targets
                        .iter()
                        .position(|x| x.client == id.0 && x.config == id.1)
                    else {
                        cx.notify();
                        return;
                    };
                    let pair = (s, t);
                    match result {
                        Some(Err(e)) => {
                            this.errors.insert(pair, e.to_string());
                        }
                        None => {
                            this.errors.insert(pair, "Stopped.".into());
                        }
                        Some(Ok(())) => {}
                    }
                    // Every server's status in that config: an install can also drop
                    // the old pre-rename entry.
                    for s in 0..SERVERS.len() {
                        let spec = this.spec(s);
                        this.status.insert((s, t), status_in(&read, &spec));
                    }
                    if let Some(entries) = this.entries.get_mut(t) {
                        *entries = read;
                    }
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
        // What's installed, to update or remove; not an install in another
        // Region, which Install would move.
        let selected = (0..self.targets.len())
            .filter(|t| self.is_installed((s, *t)) && self.other_region((s, *t)).is_none())
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
                    let region = if value.is_empty() {
                        DEFAULT_AWS_REGION.to_string()
                    } else {
                        value
                    };
                    // It goes into the server's host name.
                    let valid = valid_region(&region);
                    if let Some(Sheet::Install(sheet)) = &mut this.sheet {
                        sheet.region_invalid = !valid;
                    }
                    if valid {
                        this.region_chosen = true;
                        this.set_region(region, cx);
                    }
                    cx.notify();
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
            region_invalid: false,
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
                // From the configs as read: no file is read per keystroke.
                for (t, read) in self.entries.iter().enumerate() {
                    self.status.insert((s, t), status_in(read, &spec));
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

    /// Asks, then removes the sheet's server from the ticked clients, as the
    /// row's trash does: the sheet opens with every installed client ticked.
    fn confirm_remove_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::Install(sheet)) = &self.sheet else {
            return;
        };
        let s = sheet.server;
        let targets = self.selected_for(true);
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
                "Remove {} from {}?",
                SERVERS[s].name,
                plural(targets.len(), "client", "clients")
            ),
            Some(&format!(
                "Its entry is removed from: {}. Restart those clients to drop it.",
                names.join(", ")
            )),
            &["Remove", "Cancel"],
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

    /// The ticked clients the sheet's Install (or Remove) acts on.
    fn selected_for(&self, remove: bool) -> Vec<usize> {
        let Some(Sheet::Install(sheet)) = &self.sheet else {
            return Vec::new();
        };
        let s = sheet.server;
        let mut chosen: Vec<usize> = sheet
            .selected
            .iter()
            .copied()
            .filter(|&t| {
                let editable = self.targets.get(t).is_some_and(|x| x.client.editable());
                // Without npx, there's nothing to install there.
                let installable = editable && !self.bridge_blocked((s, t));
                match self.status.get(&(s, t)) {
                    Some(Status::NotInstalled) => !remove && installable,
                    Some(Status::Outdated(_)) => {
                        if remove {
                            editable
                        } else {
                            installable
                        }
                    }
                    Some(Status::Installed) => remove && editable,
                    _ => false,
                }
            })
            .collect();
        chosen.sort();
        chosen
    }

    fn install_selected(&mut self, cx: &mut Context<Self>) {
        let Some(Sheet::Install(sheet)) = &self.sheet else {
            return;
        };
        let s = sheet.server;
        for t in self.selected_for(false) {
            self.act((s, t), false, cx);
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
        let Some(target) = self.targets.get(t).cloned() else {
            return;
        };
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
        let mut clients: Vec<(Client, usize)> = Vec::new();
        for t in &self.targets {
            match clients.iter_mut().find(|(c, _)| *c == t.client) {
                Some((_, n)) => *n += 1,
                None => clients.push((t.client, 1)),
            }
        }
        clients.sort_by_key(|(c, _)| {
            (
                c.category() == Category::Editor,
                Client::ALL.iter().position(|x| x == c),
            )
        });
        let chips = clients.into_iter().map(|(client, n)| {
            h_flex()
                .h(px(24.))
                .px_2()
                .gap_1p5()
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .text_xs()
                .child(client_logo(client, 14., cx))
                .child(client.name())
                .when(n > 1, |el| {
                    el.child(div().text_color(muted).child(format!("×{n}")))
                })
        });
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_xl().font_semibold().child("MCP Servers"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .whitespace_normal()
                                    .child(
                                        "Install a server into every agent, editor and profile at once. Servers with an \
                                         account sign in through the agent's own sign-in; restart the client to pick up changes.",
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_2()
                            .when(checking, |el| {
                                el.child(catalog::pill(Mark::Spinner, "Checking sign-ins…", muted))
                            })
                            .child(
                                Button::new("mcp-reload")
                                    .ghost()
                                    .small()
                                    .icon(Lucide::RefreshCw)
                                    .tooltip(if self.busy.is_empty() {
                                        "Detect Again"
                                    } else {
                                        "Detect Again When the Installs Finish"
                                    })
                                    .loading(self.loading)
                                    // A detection can reorder the clients under a running
                                    // install.
                                    .disabled(!self.busy.is_empty())
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
                    ),
            )
            .when(!self.loading, |el| {
                el.child(
                    h_flex()
                        .gap_1p5()
                        .flex_wrap()
                        .child(
                            div()
                                .pr_1()
                                .text_xs()
                                .font_medium()
                                .text_color(muted)
                                .child(if self.targets.is_empty() {
                                    "No AI clients found on this Mac.".to_string()
                                } else {
                                    format!(
                                        "Installs into {} on this Mac:",
                                        plural(self.targets.len(), "target", "targets")
                                    )
                                }),
                        )
                        .children(chips),
                )
            })
    }

    /// Clients that have server `s`, counting other versions too.
    fn installs(&self, s: usize) -> usize {
        (0..self.targets.len())
            .filter(|t| self.is_installed((s, *t)))
            .count()
    }

    fn matches(s: usize, query: &str) -> bool {
        let server = &SERVERS[s];
        let has = |t: &str| t.to_lowercase().contains(query);
        query.is_empty()
            || has(server.name)
            || has(server.id)
            || has(server.about)
            || has(Kind::of(s).label())
            || Self::tags(server).iter().any(|t| has(t))
    }

    /// The tags a row shows beside the server's kind.
    fn tags(server: &Server) -> Vec<&'static str> {
        match server.auth {
            Auth::Browser => vec!["Sign-in"],
            Auth::EnvKey(_) => vec!["API key"],
            Auth::None | Auth::Program => Vec::new(),
        }
    }

    /// Matches the search and the kind: what the filter tabs count.
    fn in_scope(&self, s: usize, query: &str) -> bool {
        Self::matches(s, query) && self.kind.is_none_or(|k| Kind::of(s) == k)
    }

    fn passes(&self, s: usize, filter: Filter) -> bool {
        match filter {
            Filter::All => true,
            Filter::Installed => self.installs(s) > 0,
            Filter::Missing => self.installs(s) == 0,
            Filter::SignIn => !self.pending_sign_ins(Some(s)).is_empty(),
        }
    }

    /// The servers the search, kind, tab and sort leave.
    fn shown(&self, query: &str) -> Vec<usize> {
        let mut rows: Vec<usize> = (0..SERVERS.len())
            .filter(|s| self.in_scope(*s, query) && self.passes(*s, self.filter))
            .collect();
        match self.sort {
            Sort::Catalog => rows.sort_by_key(|s| (Kind::of(*s), *s)),
            Sort::Name => rows.sort_by_key(|s| SERVERS[*s].name.to_lowercase()),
            Sort::NameDesc => {
                rows.sort_by_key(|s| std::cmp::Reverse(SERVERS[*s].name.to_lowercase()))
            }
            Sort::Installed => rows.sort_by_key(|s| (std::cmp::Reverse(self.installs(*s)), *s)),
        }
        rows
    }

    /// Search, the filter tabs with their counts, kind and sort.
    fn render_toolbar(&self, query: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scope: Vec<usize> = (0..SERVERS.len())
            .filter(|s| self.in_scope(*s, query))
            .collect();
        let choices = Filter::ALL
            .iter()
            .map(|f| {
                (
                    f.label(),
                    scope.iter().filter(|s| self.passes(**s, *f)).count(),
                )
            })
            .collect();
        let current = Filter::ALL
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        let mut kinds = vec!["All Types"];
        kinds.extend(Kind::ALL.iter().map(|k| k.label()));
        let kind = self
            .kind
            .and_then(|k| Kind::ALL.iter().position(|x| *x == k))
            .map_or(0, |ix| ix + 1);
        let sort = Sort::ALL.iter().position(|s| *s == self.sort).unwrap_or(0);
        h_flex()
            .gap_2()
            .flex_wrap()
            .when_some(self.search.as_ref(), |el, search| {
                el.child(catalog::search(search, cx))
            })
            .child(catalog::tabs(
                "mcp-filter",
                choices,
                current,
                cx.listener(|this, ix: &usize, _, cx| {
                    this.filter = Filter::ALL[*ix];
                    this.page = 0;
                    cx.notify();
                }),
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .ml_auto()
                    .child(catalog::menu(
                        "mcp-kind",
                        Lucide::Funnel,
                        kinds[kind].to_string(),
                        kinds,
                        kind,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.kind = ix.checked_sub(1).map(|ix| Kind::ALL[ix]);
                            this.page = 0;
                            cx.notify();
                        }),
                    ))
                    .child(catalog::menu(
                        "mcp-sort",
                        Lucide::ArrowUpDown,
                        format!("Sort: {}", self.sort.label()),
                        Sort::ALL.iter().map(|s| s.label()).collect(),
                        sort,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.sort = Sort::ALL[*ix];
                            this.page = 0;
                            cx.notify();
                        }),
                    )),
            )
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
            .filter(|t| {
                matches!(self.status.get(&(s, *t)), Some(Status::Outdated(_)))
                    && self.other_region((s, *t)).is_none()
            })
            .count();
        // Installed in another Region: installed, not out of date.
        let mut elsewhere: Vec<String> = (0..total)
            .filter_map(|t| self.other_region((s, t)))
            .collect();
        let in_other_regions = elsewhere.len();
        elsewhere.sort();
        elsewhere.dedup();
        let to_sign_in = self.pending_sign_ins(Some(s)).len();
        let checking = server.auth == Auth::Browser
            && (0..total).any(|t| self.checking.contains(&t) && self.is_installed((s, t)));
        let missing = server.missing_program(|c| self.programs.get(c).cloned());
        let bundled_missing = s == 0 && !self.server_exists;
        let busy = (0..total).any(|t| self.busy.contains(&(s, t)));
        let none = installed + other + in_other_regions == 0;

        let (mark, summary, summary_color) = if busy {
            (Mark::Spinner, "Working…".to_string(), theme.warning)
        } else if none {
            (
                Mark::Icon(Lucide::CircleDashed),
                "Not installed".to_string(),
                muted,
            )
        } else if other > 0 {
            (
                Mark::Icon(Lucide::CircleAlert),
                format!(
                    "{} of {total} · {other} to update",
                    installed + in_other_regions
                ),
                theme.warning,
            )
        } else {
            (
                Mark::Icon(Lucide::CircleCheck),
                format!("Installed · {} of {total}", installed + in_other_regions),
                theme.success,
            )
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
        let bridge_only: Vec<&str> = (0..total)
            .filter(|&t| self.bridge_blocked((s, t)))
            .map(|t| self.targets[t].client.name())
            .fold(Vec::new(), |mut names, name| {
                if !names.contains(&name) {
                    names.push(name);
                }
                names
            });
        if !bridge_only.is_empty() {
            notes.push((
                Lucide::TriangleAlert,
                format!(
                    "{} reach remote servers through npx, and `npx` isn't installed: install Node.js.",
                    bridge_only.join(", ")
                ),
                theme.warning,
            ));
        }
        if !elsewhere.is_empty() {
            notes.push((
                Lucide::Globe,
                format!(
                    "{} in another Region: {}.",
                    plural(in_other_regions, "client has it", "clients have it"),
                    elsewhere.join(", ")
                ),
                muted,
            ));
        }
        if let Auth::EnvKey(var) = server.auth {
            notes.push((
                Lucide::KeyRound,
                format!("Uses an API key: set {var} in the environment the client starts in."),
                muted,
            ));
        }
        // What failed or was stopped, where it was started: the row's Uninstall
        // and Install don't open the sheet that lists errors per client.
        let failures: Vec<String> = (0..total)
            .filter_map(|t| {
                let error = self.errors.get(&(s, t))?;
                Some(format!(
                    "Couldn't change {}: {}",
                    target_label(&self.targets[t]),
                    error.lines().next().unwrap_or_default()
                ))
            })
            .collect();
        let kind = Kind::of(s);

        catalog::columns(
            None,
            catalog::tile(server_logo(server, cx), cx),
            v_flex()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(div().text_sm().font_semibold().child(server.name))
                        .child(catalog::tag(Some(kind.icon()), kind.label(), cx))
                        .children(
                            Self::tags(server)
                                .into_iter()
                                .map(|tag| catalog::tag(Some(Lucide::KeyRound), tag, cx)),
                        ),
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
                }))
                .when(!failures.is_empty(), |el| {
                    let more = failures.len().saturating_sub(2);
                    el.child(
                        v_flex()
                            .debug_selector(|| "mcp-row-error".into())
                            .gap_0p5()
                            .text_xs()
                            .text_color(theme.danger)
                            .children(failures.into_iter().take(2).map(|line| {
                                h_flex()
                                    .gap_1p5()
                                    .items_start()
                                    .child(
                                        div().pt(px(1.)).child(Icon::new(Lucide::CircleX).xsmall()),
                                    )
                                    .child(div().whitespace_normal().child(line))
                            }))
                            .when(more > 0, |el| {
                                el.child(format!(
                                    "And {} more: see Manage.",
                                    plural(more, "client", "clients")
                                ))
                            }),
                    )
                }),
            v_flex()
                .gap_1()
                .items_start()
                .child(catalog::pill(mark, summary, summary_color))
                .when(to_sign_in > 0, |el| {
                    el.child(catalog::pill(
                        Mark::Icon(Lucide::KeyRound),
                        format!(
                            "Sign-in needed · {}",
                            plural(to_sign_in, "profile", "profiles")
                        ),
                        theme.warning,
                    ))
                })
                .when(checking && to_sign_in == 0, |el| {
                    el.child(catalog::pill(Mark::Spinner, "Checking sign-in…", muted))
                }),
            h_flex()
                .gap_1()
                .when(busy, |el| {
                    el.child(
                        Button::new(("mcp-stop", s))
                            .outline()
                            .small()
                            .icon(Lucide::CircleStop)
                            .label("Stop")
                            .tooltip("Stop Installing or Removing")
                            .on_click(cx.listener(move |this, _, _, cx| this.stop(s, cx))),
                    )
                })
                .when(!busy && !self.removable(s).is_empty(), |el| {
                    el.child(
                        Button::new(("mcp-uninstall", s))
                            .ghost()
                            .small()
                            .icon(Icon::new(Lucide::Trash).text_color(theme.danger))
                            .tooltip(format!("Uninstall {} from Every Client", server.name))
                            .disabled(self.loading)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_uninstall(s, window, cx)
                            })),
                    )
                })
                .when(to_sign_in > 0, |el| {
                    el.child(
                        Button::new(("mcp-sign-in", s))
                            .outline()
                            .small()
                            .icon(Lucide::KeyRound)
                            .label("Sign In")
                            .tooltip(format!(
                                "Sign In on {}",
                                plural(to_sign_in, "Profile", "Profiles")
                            ))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let queue = this.pending_sign_ins(Some(s));
                                this.start_sign_in(queue, window, cx)
                            })),
                    )
                })
                .child(
                    Button::new(("mcp-open", s))
                        .when(none, |b| {
                            b.primary().icon(Lucide::Download).label("Install")
                        })
                        .when(!none, |b| {
                            b.outline().icon(Lucide::Settings2).label("Manage")
                        })
                        .small()
                        .tooltip(if none {
                            "Choose the Clients to Install Into"
                        } else {
                            "Install, Update or Remove per Client"
                        })
                        .disabled(self.loading || self.targets.is_empty())
                        .on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.open_install(s, window, cx)
                            }),
                        ),
                ),
        )
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(theme.border)
        .hover(|el| el.bg(theme.list_hover))
    }

    /// The list card: column titles, a page of servers and the pager, or
    /// the empty state.
    fn render_list(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        if self.loading {
            return h_flex()
                .py_12()
                .gap_2()
                .justify_center()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(theme.border)
                .text_sm()
                .text_color(muted)
                .child(Spinner::new().small().color(muted))
                .child("Detecting clients…")
                .into_any_element();
        }
        let rows = self.shown(query);
        let total = rows.len();
        let card = v_flex()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden();
        if total == 0 {
            let title = if query.is_empty() {
                "No servers here".to_string()
            } else {
                format!("No servers match “{query}”")
            };
            return card
                .child(catalog::empty(
                    title,
                    Button::new("mcp-clear-filters")
                        .outline()
                        .small()
                        .icon(Lucide::FunnelX)
                        .label("Clear Filters")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.clear_filters(window, cx)),
                        ),
                    cx,
                ))
                .into_any_element();
        }
        let (rows, page) = catalog::page_of(rows, self.page);
        card.child(catalog::header(None, "Server", cx))
            .children(rows.into_iter().map(|s| self.render_server(s, cx)))
            .child(catalog::pager(
                "mcp-pages",
                page,
                total,
                if total == 1 { "server" } else { "servers" },
                cx.listener(|this, page: &usize, _, cx| {
                    this.page = *page;
                    cx.notify();
                }),
                cx,
            ))
            .into_any_element()
    }

    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = Filter::All;
        self.kind = None;
        self.page = 0;
        if let Some(search) = &self.search {
            search.update(cx, |s, cx| s.set_value("", window, cx));
        }
        cx.notify();
    }

    fn render_install_sheet(&self, sheet: &InstallSheet, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let s = sheet.server;
        let server = &SERVERS[s];
        let spec = self.spec(s);
        let selected = &sheet.selected;
        // What Install and Remove act on.
        let to_install = self.selected_for(false).len();
        let to_remove = self.selected_for(true).len();
        let to_sign_in: Vec<Pair> = self
            .pending_sign_ins(Some(s))
            .into_iter()
            .filter(|(_, t)| selected.contains(t))
            .collect();
        let busy = (0..self.targets.len()).any(|t| self.busy.contains(&(s, t)));
        let blocked = (s == 0 && !self.server_exists)
            || sheet.region_invalid
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
                        .child(client_logo(client, 18., cx))
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
                                .child(if sheet.region_invalid {
                                    div()
                                        .text_xs()
                                        .text_color(theme.danger)
                                        .child("Not an AWS Region (like eu-west-1).")
                                } else {
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child("The AWS Region the server works in.")
                                }),
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
                            .debug_selector(|| "mcp-sheet-remove".into())
                            .danger()
                            .outline()
                            .small()
                            .label(format!("Remove from {to_remove}"))
                            .disabled(to_remove == 0 || busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_remove_selected(window, cx)
                            })),
                    )
                    .child(
                        Button::new("mcp-sheet-install")
                            .debug_selector(|| "mcp-sheet-install".into())
                            .primary()
                            .small()
                            .label(format!("Install to {to_install}"))
                            .loading(busy)
                            .disabled(to_install == 0 || busy || blocked)
                            .on_click(cx.listener(|this, _, _, cx| this.install_selected(cx))),
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
        let elsewhere = self.other_region(pair);
        let (badge, color): (String, Hsla) = match &state {
            Status::Installed if self.needs_sign_in(pair) => {
                ("Needs sign-in".into(), theme.warning)
            }
            Status::Installed
                if signed == Some(SignIn::SignedIn) && SERVERS[s].auth == Auth::Browser =>
            {
                ("Signed in".into(), theme.success)
            }
            Status::Installed => ("Installed".into(), theme.success),
            Status::Outdated(_) if let Some(region) = &elsewhere => {
                (format!("In {region}"), theme.info)
            }
            Status::Outdated(_) => ("Other version".into(), theme.warning),
            Status::NotInstalled if self.bridge_blocked(pair) => {
                ("Needs Node.js".into(), theme.warning)
            }
            Status::NotInstalled => ("Not installed".into(), muted),
            Status::Unreadable(_) => ("Edit manually".into(), theme.danger),
            Status::Manual => ("Paste by hand".into(), muted),
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
                        .child(client_logo(target.client, 18., cx))
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
            self._search = Some(cx.subscribe(&search, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.page = 0;
                    cx.notify();
                }
            }));
            self.search = Some(search);
        }
        let query = self
            .search
            .as_ref()
            .map(|s| s.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
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
                        .gap_4()
                        .child(self.render_header(cx))
                        .child(self.render_toolbar(&query, cx))
                        .child(self.render_list(&query, cx)),
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use gpui_kit::{
        AppContext as _, Bounds, Entity, Modifiers, TestAppContext, VisualTestContext,
        WindowBounds, WindowOptions, point, px, size,
    };
    use sawit_core::mcp_clients::{Client, Status, Target};

    use super::McpPage;

    fn target(client: Client, config: &Path) -> Target {
        Target {
            client,
            profile: "default".into(),
            config: config.to_path_buf(),
            profile_dir: None,
        }
    }

    /// The page in a window, with `targets` detected and `setup` applied, before any reload.
    fn open(
        cx: &mut TestAppContext,
        targets: Vec<Target>,
        setup: impl FnOnce(&mut McpPage),
    ) -> (Entity<McpPage>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
        });
        // Installs and removals finish on the Tokio runtime's threads.
        cx.executor().allow_parking();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(1200.), px(900.)),
            })),
            ..Default::default()
        };
        let (window, page) = cx
            .update(|cx| {
                gpui_kit::open_window(options, cx, |_, cx| {
                    cx.new(|cx| {
                        let mut page = McpPage::blank(cx);
                        page.targets = targets;
                        page.loading = false;
                        setup(&mut page);
                        page
                    })
                })
            })
            .unwrap();
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        cx.run_until_parked();
        (page, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} isn't drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// Typing a Region works out the statuses from the configs the last detection read: it
    /// doesn't read every client's config again, on the UI thread, for each character.
    #[gpui_kit::test]
    fn typing_a_region_reads_no_config(cx: &mut TestAppContext) {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".codex/config.toml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            &config,
            "[mcp_servers.aws]\nurl = \"https://aws-mcp.eu-west-1.api.aws/mcp\"\n",
        )
        .unwrap();
        let codex = target(Client::Codex, &config);
        let read = sawit_core::mcp_clients::read_entries(&codex);
        let (page, cx) = open(cx, vec![codex], |page| page.entries = vec![read]);
        // Since the detection, the file has changed (here: it no longer reads).
        std::fs::write(&config, "[mcp_servers\n").unwrap();
        page.update(cx, |page, cx| page.set_region("eu-west-1".into(), cx));
        let aws = super::SERVERS.iter().position(|s| s.id == "aws").unwrap();
        assert_eq!(
            page.read_with(cx, |page, _| page.status.get(&(aws, 0)).cloned()),
            Some(Status::Installed)
        );
    }

    /// The AWS server installed in another Region than the page's default is installed there:
    /// the page takes that Region, and the sheet doesn't offer to move it.
    #[gpui_kit::test]
    fn aws_in_another_region_is_installed_there(cx: &mut TestAppContext) {
        let home = tempfile::tempdir().unwrap();
        let codex = home.path().join(".codex/config.toml");
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        std::fs::write(
            &codex,
            "[mcp_servers.aws]\nurl = \"https://aws-mcp.eu-west-1.api.aws/mcp\"\n",
        )
        .unwrap();
        let cursor = home.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
        let targets = vec![
            target(Client::Codex, &codex),
            target(Client::Cursor, &cursor),
        ];
        let (page, cx) = open(cx, Vec::new(), |_| {});
        page.update(cx, |page, cx| {
            let detected = super::Detected {
                entries: targets
                    .iter()
                    .map(sawit_core::mcp_clients::read_entries)
                    .collect(),
                targets,
                clis: Default::default(),
                programs: Default::default(),
                server_exists: true,
            };
            page.apply(detected, cx)
        });
        let aws = super::SERVERS.iter().position(|s| s.id == "aws").unwrap();
        page.read_with(cx, |page, _| {
            assert_eq!(page.region, "eu-west-1");
            assert_eq!(page.status.get(&(aws, 0)), Some(&Status::Installed));
        });
        // Typed back to us-east-1, it's in another Region: not ticked for Install.
        page.update(cx, |page, cx| page.set_region("us-east-1".into(), cx));
        page.update_in(cx, |page, window, cx| page.open_install(aws, window, cx));
        page.read_with(cx, |page, _| {
            let Some(super::Sheet::Install(sheet)) = &page.sheet else {
                panic!("no sheet")
            };
            assert!(sheet.selected.is_empty(), "{:?}", sheet.selected);
        });
    }

    /// A client that only runs programs (Claude Desktop, …) reaches a remote server through
    /// `npx mcp-remote`: without npx it isn't installed there, and Install leaves it out.
    #[gpui_kit::test]
    fn remote_servers_skip_bridge_only_clients_without_npx(cx: &mut TestAppContext) {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join("claude_desktop_config.json");
        std::fs::write(&config, "{}").unwrap();
        let desktop = target(Client::ClaudeDesktop, &config);
        let read = sawit_core::mcp_clients::read_entries(&desktop);
        let neon = super::SERVERS.iter().position(|s| s.id == "neon").unwrap();
        let (page, cx) = open(cx, vec![desktop], |page| {
            page.entries = vec![read];
            page.recompute();
        });
        page.update_in(cx, |page, window, cx| {
            page.open_install(neon, window, cx);
            if let Some(super::Sheet::Install(sheet)) = &mut page.sheet {
                sheet.selected.insert(0);
            }
            page.install_selected(cx);
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "{}");
    }

    /// A detection can change which client is at which place in the list while an install
    /// runs (a client appeared, or one went): the result lands on the client it was for, and
    /// a place past the end of the list is no client at all.
    #[gpui_kit::test]
    fn results_land_on_their_client_when_the_list_changes(cx: &mut TestAppContext) {
        let home = tempfile::tempdir().unwrap();
        let config = |name: &str| {
            let path = home.path().join(name).join("mcp.json");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "{}").unwrap();
            path
        };
        let (a, b) = (config("a"), config("b"));
        let (page, cx) = open(cx, vec![target(Client::Cursor, &a)], |page| {
            page.entries = vec![sawit_core::mcp_clients::read_entries(&target(
                Client::Cursor,
                &a,
            ))];
            page.recompute();
            page.server_exists = true;
        });
        page.update(cx, |page, cx| {
            page.act((0, 0), false, cx);
            // Detect Again finds another client first.
            let targets = vec![target(Client::Cursor, &b), target(Client::Cursor, &a)];
            page.apply(
                super::Detected {
                    entries: targets
                        .iter()
                        .map(sawit_core::mcp_clients::read_entries)
                        .collect(),
                    targets,
                    clis: Default::default(),
                    programs: Default::default(),
                    server_exists: true,
                },
                cx,
            );
            // And a place past the end of the list.
            page.check_sign_ins(vec![7], cx);
        });
        let done = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            cx.run_until_parked();
            page.read_with(cx, |page, _| page.busy.is_empty())
        });
        assert!(done, "the install didn't finish");
        page.read_with(cx, |page, _| {
            assert_eq!(page.status.get(&(0, 1)), Some(&Status::Installed), "a");
            assert_eq!(page.status.get(&(0, 0)), Some(&Status::NotInstalled), "b");
        });
    }

    /// A row's Uninstall (or Install) that fails for a client says so on the row, which is
    /// where it was started: not only in the Manage sheet, which may never be opened.
    #[gpui_kit::test]
    fn a_failure_shows_on_the_servers_row(cx: &mut TestAppContext) {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        // Comments: Sawit won't rewrite it, so the change fails.
        std::fs::write(&config, "// mine\n{\"mcpServers\": {}}\n").unwrap();
        let (page, cx) = open(cx, vec![target(Client::Cursor, &config)], |page| {
            page.server_exists = true;
        });
        assert!(cx.debug_bounds("mcp-row-error").is_none());
        page.update(cx, |page, cx| page.act((0, 0), false, cx));
        let failed = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            cx.run_until_parked();
            page.read_with(cx, |page, _| !page.errors.is_empty())
        });
        assert!(failed, "the change didn't fail");
        assert!(
            cx.debug_bounds("mcp-row-error").is_some(),
            "the row doesn't say"
        );
    }

    /// The search finds what the rows show, their tags too: "sign" finds the servers tagged
    /// Sign-in, "api key" the one tagged API key.
    #[test]
    fn the_search_matches_the_tags() {
        let at = |id: &str| super::SERVERS.iter().position(|s| s.id == id).unwrap();
        assert!(McpPage::matches(at("aws"), "sign"));
        assert!(McpPage::matches(at("epidemic-sound"), "api key"));
        assert!(!McpPage::matches(at("exa"), "sign"));
    }

    /// The sheet opens with every installed client ticked, so its Remove asks first, as the
    /// row's trash does: one click must not take a server out of every client.
    #[gpui_kit::test]
    fn the_sheets_remove_asks_first(cx: &mut TestAppContext) {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            &config,
            r#"{"mcpServers": {"sawit": {"command": "/x/sawit-mcp"}}}"#,
        )
        .unwrap();
        let (page, cx) = open(cx, vec![target(Client::Cursor, &config)], |page| {
            page.status.insert((0, 0), Status::Installed);
        });
        page.update_in(cx, |page, window, cx| page.open_install(0, window, cx));
        cx.run_until_parked();

        // The removal runs off the UI thread: give it the time it takes.
        let has_sawit = |cx: &mut VisualTestContext| {
            std::thread::sleep(std::time::Duration::from_millis(300));
            cx.run_until_parked();
            std::fs::read_to_string(&config).unwrap().contains("sawit")
        };
        let removed = |cx: &mut VisualTestContext| (0..50).any(|_| !has_sawit(cx));
        click(cx, "mcp-sheet-remove");
        assert!(has_sawit(cx), "removed without asking");
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Remove Sawit from 1 client?".to_string(),
                "Its entry is removed from: Cursor. Restart those clients to drop it.".to_string()
            ))
        );
        cx.simulate_prompt_answer("Cancel");
        assert!(has_sawit(cx), "Cancel removed it");

        click(cx, "mcp-sheet-remove");
        cx.simulate_prompt_answer("Remove");
        assert!(removed(cx), "Remove didn't remove it");
    }
}
