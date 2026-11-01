//! Skills page: agent skill packs from their official sources (Superpowers,
//! Jakub Krehel's interface skills, unslop, Animate, MCP Apps, Cloudflare's),
//! installed into the agents found on this Mac the way each source documents:
//! the `skills` CLI run with the package manager picked (npx, pnpm dlx, bunx,
//! yarn dlx), a Claude Code plugin, or a clone and copy. A pack's sheet picks
//! the agents and shows the exact commands before they run; they run one pack
//! at a time in the login shell, as the Dev Tools page's installers do, with
//! their output under the row. Removing deletes the pack's skill folders from
//! the agents picked, or uninstalls its plugin.

use std::collections::{HashMap, HashSet, VecDeque};

use futures::StreamExt as _;
use futures::channel::oneshot;
use futures::future::{AbortHandle, abortable};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::dev_tools::find_program;
use sawit_core::package_manager::{self, Available, PackageManager, Use};
use sawit_core::skills::{self, Action, Category, Method, PACKS, Pack, Report, Status};

use crate::catalog::{self, Mark, tilde};
use crate::rt;

/// Lines of output kept per pack.
const LOG_LINES: usize = 200;

#[derive(Debug, Clone, PartialEq)]
enum State {
    Queued(Action),
    Running(Action),
    /// It worked; shown until detection agrees.
    Done(Action),
    Failed(Action, String),
    Stopped,
}

/// A script queued or running: what it does to which pack, and its lines.
#[derive(Debug, Clone)]
struct Job {
    pack: &'static Pack,
    action: Action,
    lines: Vec<String>,
}

/// Which packs the filter tabs show.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Filter {
    All,
    Installed,
    Missing,
}

impl Filter {
    const ALL: [Filter; 3] = [Filter::All, Filter::Installed, Filter::Missing];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Installed => "Installed",
            Filter::Missing => "Not Installed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
            Sort::Catalog => "Category",
            Sort::Name => "Name (A–Z)",
            Sort::NameDesc => "Name (Z–A)",
            Sort::Installed => "Most Installed",
        }
    }
}

/// A pack's sheet: how to install it and the agents picked, by their place
/// in the report's targets.
struct Sheet {
    pack: &'static Pack,
    method: Method,
    selected: HashSet<usize>,
}

/// Every Skills page, with the window it is in: what quitting and closing a
/// window ask about ([`at_work`]).
#[derive(Default)]
struct Pages(Vec<(AnyWindowHandle, WeakEntity<SkillsPage>)>);

impl Global for Pages {}

/// What each Skills page runs and has queued, with its window: closing the
/// window and quitting ask first and stop it.
pub fn at_work(cx: &App) -> Vec<(AnyWindowHandle, String)> {
    let Some(pages) = cx.try_global::<Pages>() else {
        return Vec::new();
    };
    pages
        .0
        .iter()
        .filter_map(|(window, page)| Some((*window, page.upgrade()?.read(cx).work()?)))
        .collect()
}

/// What detection finds, off the UI thread.
struct Detected {
    report: Report,
    claude: bool,
    git: bool,
}

pub struct SkillsPage {
    report: Report,
    loading: bool,
    /// The package managers on this Mac.
    pms: Vec<Available>,
    /// The one the skills CLI runs with.
    pm: Option<PackageManager>,
    /// Claude Code's CLI is installed: plugins install through it.
    claude: bool,
    /// git is installed: copies clone with it.
    git: bool,
    /// The home folder and `CLAUDE_CONFIG_DIR` the commands are written for.
    env: skills::Env,
    states: HashMap<&'static str, State>,
    logs: HashMap<&'static str, Vec<String>>,
    expanded: HashSet<&'static str>,
    queue: VecDeque<Job>,
    running: Option<Job>,
    /// Stops the running script.
    stop: Option<AbortHandle>,
    /// Ends once the running script is over, or stopped and its process group
    /// told to end: what quitting waits for.
    ended: Option<oneshot::Receiver<()>>,
    _quit: Subscription,
    sheet: Option<Sheet>,
    filter: Filter,
    category: Option<Category>,
    sort: Sort,
    /// The page of the list shown, and how many rows a page holds.
    paging: catalog::Paging,
    /// The search as the list last filtered by it.
    last_query: String,
    /// The row ↑ and ↓ select and ↩ opens, by its place in the list.
    cursor: Option<usize>,
    /// The page's own keyboard focus.
    focus: FocusHandle,
    /// The sheet's.
    sheet_focus: FocusHandle,
    /// A sheet closed: the list gets the keyboard back on the next render.
    refocus: bool,
    /// Made with the page when it has a window, else on the first render.
    search: Option<Entity<InputState>>,
    _search: Option<Subscription>,
}

/// A pack's tile: its publisher's logo when there is one, else its
/// category's icon.
fn pack_logo(pack: &Pack, cx: &App) -> AnyElement {
    if pack.by == "Cloudflare" {
        return img("agent_logos/cloudflare-color.svg")
            .size(px(22.))
            .flex_none()
            .into_any_element();
    }
    let icon = match pack.category {
        Category::Design => Lucide::Palette,
        Category::Workflow => Lucide::Workflow,
        Category::Platform => Lucide::Blocks,
        Category::Media => Lucide::Clapperboard,
    };
    Icon::new(icon)
        .size(px(18.))
        .text_color(cx.theme().muted_foreground)
        .into_any_element()
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl SkillsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self::blank(cx);
        this.make_search(window, cx);
        this.reload(cx);
        Self::register(window, cx);
        this
    }

    /// Lists the page being made in `window`'s ([`at_work`]).
    fn register(window: &Window, cx: &mut Context<Self>) {
        let (me, here) = (cx.weak_entity(), window.window_handle());
        let pages = cx.default_global::<Pages>();
        pages.0.retain(|(_, page)| page.upgrade().is_some());
        pages.0.push((here, me));
    }

    /// The page before anything is detected.
    fn blank(cx: &mut Context<Self>) -> Self {
        Self {
            report: Report {
                targets: Vec::new(),
                installed: Vec::new(),
            },
            loading: true,
            pms: Vec::new(),
            pm: None,
            claude: false,
            git: false,
            env: skills::Env::current(),
            states: HashMap::new(),
            logs: HashMap::new(),
            expanded: HashSet::new(),
            queue: VecDeque::new(),
            running: None,
            stop: None,
            ended: None,
            _quit: cx.on_app_quit(|this, _| this.stop_for_quit()),
            sheet: None,
            filter: Filter::All,
            category: None,
            sort: Sort::Catalog,
            paging: catalog::Paging::new("skills", cx),
            last_query: String::new(),
            cursor: None,
            focus: cx.focus_handle(),
            sheet_focus: cx.focus_handle(),
            refocus: false,
            search: None,
            _search: None,
        }
    }

    /// The search field: typing filters, ↩ moves the keyboard to the first row.
    fn make_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search skills"));
        self._search = Some(cx.subscribe_in(
            &search,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                // Only when the text changed: the field also says so when it loses the
                // keyboard.
                InputEvent::Change if this.query(cx) != this.last_query => {
                    this.last_query = this.query(cx);
                    this.paging.page = 0;
                    this.cursor = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.cursor = None;
                    this.select(true, window, cx);
                }
                _ => {}
            },
        ));
        self.search = Some(search);
    }

    /// Gives the search field the keyboard, as opening the page does.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sheet.is_some() {
            return;
        }
        if self.search.is_none() {
            self.make_search(window, cx);
        }
        if let Some(search) = &self.search {
            search.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    fn query(&self, cx: &App) -> String {
        self.search
            .as_ref()
            .map(|s| s.read(cx).value().trim().to_lowercase())
            .unwrap_or_default()
    }

    /// Selects the next (or previous) row, turning the list's page to it, and
    /// gives the list the keyboard.
    fn select(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.shown(&self.query(cx)).len();
        self.cursor = catalog::step(self.cursor, len, down);
        if let Some(ix) = self.cursor {
            self.paging.show(ix);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// ↩: in the sheet, Install (when it can); on the list, the selected
    /// pack's sheet. Never Remove.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(sheet) = &self.sheet {
            let pack = sheet.pack;
            let install = self.picked(Pick::Install);
            if !install.is_empty() && !self.busy(pack.id) && self.blocked(sheet.method).is_none() {
                self.run_sheet(Action::Install, cx);
            }
            return;
        }
        let rows = self.shown(&self.query(cx));
        if !self.loading
            && let Some(&pack) = self.cursor.and_then(|ix| rows.get(ix))
        {
            self.open_sheet(pack, window, cx);
        }
    }

    /// Detects the agents, what each has, and the package managers, `claude`
    /// and `git` (off the UI thread).
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let rx = rt::spawn_blocking(cx, || Detected {
            report: skills::report(),
            claude: find_program("claude").is_some(),
            git: find_program("git").is_some() || std::path::Path::new("/usr/bin/git").exists(),
        });
        let pms = rt::spawn(cx, package_manager::detect());
        cx.spawn(async move |this, cx| {
            let (Ok(detected), Ok(pms)) = (rx.await, pms.await) else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.detected(detected, pms, cx));
        })
        .detach();
        cx.notify();
    }

    fn detected(&mut self, detected: Detected, pms: Vec<Available>, cx: &mut Context<Self>) {
        // A sheet's rows are places in the old list, which may now hold others.
        if self.sheet.is_some() && detected.report.targets != self.report.targets {
            self.sheet = None;
            self.refocus = true;
        }
        self.report = detected.report;
        self.claude = detected.claude;
        self.git = detected.git;
        self.pm = package_manager::choose(
            &pms,
            self.pm,
            crate::package_managers::remembered(),
            Use::Run,
        );
        self.pms = pms;
        self.loading = false;
        // Detection now agrees with what ran.
        self.states
            .retain(|_, s| !matches!(s, State::Done(_) | State::Stopped));
        cx.notify();
    }

    fn status(&self, pack: &Pack, t: usize) -> Status {
        self.report.status(pack, t)
    }

    /// The agents that have the pack, or part of it.
    fn having(&self, pack: &Pack) -> Vec<usize> {
        (0..self.report.targets.len())
            .filter(|&t| self.status(pack, t).any())
            .collect()
    }

    fn busy(&self, id: &str) -> bool {
        matches!(
            self.states.get(id),
            Some(State::Queued(_) | State::Running(_))
        )
    }

    /// What is running and waiting, when anything is: "Installing
    /// Superpowers, 1 more queued".
    pub fn work(&self) -> Option<String> {
        let running = self
            .running
            .as_ref()
            .map(|job| format!("{} {}", doing(job.action), job.pack.name));
        match (running, self.queue.len()) {
            (None, 0) => None,
            (Some(running), 0) => Some(running),
            (Some(running), n) => Some(format!("{running}, {n} more queued")),
            (None, n) => Some(format!("{n} queued")),
        }
    }

    /// Why `method` can't run here, when it can't: a program it needs is
    /// missing.
    fn blocked(&self, method: Method) -> Option<String> {
        match method {
            Method::SkillsCli { .. } if self.pm.is_none() => Some(
                "The skills CLI runs with npx, pnpm dlx, bunx or yarn dlx, and none is installed: install Node.js, Bun or pnpm from the Dev Tools page."
                    .into(),
            ),
            Method::Plugin { .. } if !self.claude => Some(
                "Plugins install through Claude Code's own CLI, `claude`, which isn't installed: install Claude Code from the Dev Tools page."
                    .into(),
            ),
            Method::Copy { .. } if !self.git => {
                Some("The copy clones with git, which isn't installed: run xcode-select --install.".into())
            }
            _ => None,
        }
    }

    // Jobs

    /// Queues `lines` for `pack` and starts the queue.
    fn enqueue(
        &mut self,
        pack: &'static Pack,
        action: Action,
        lines: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.busy(pack.id) {
            return;
        }
        self.states.insert(pack.id, State::Queued(action));
        self.queue.push_back(Job {
            pack,
            action,
            lines,
        });
        self.pump(cx);
        cx.notify();
    }

    /// Starts the next queued script unless one is running.
    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let Some(job) = self.queue.pop_front() else {
            return;
        };
        let id = job.pack.id;
        self.states.insert(id, State::Running(job.action));
        let mut log = vec![format!("$ {}", job.lines.join("\n$ "))];
        log.truncate(LOG_LINES);
        self.logs.insert(id, log);
        self.running = Some(job.clone());
        let (tx, mut lines) = futures::channel::mpsc::unbounded::<String>();
        let (run, stop) = abortable({
            let job = job.clone();
            async move {
                skills::run(job.pack, &job.lines, move |line| {
                    let _ = tx.unbounded_send(line);
                })
                .await
                .map_err(|e| e.to_string())
            }
        });
        self.stop = Some(stop);
        let (ended, ended_rx) = oneshot::channel::<()>();
        self.ended = Some(ended_rx);
        // `None` when it was stopped.
        let done = rt::spawn(cx, async move {
            let result = run.await.ok();
            // Only now: the run, and the process group it ends when dropped, are gone.
            drop(ended);
            result
        });
        cx.spawn(async move |this, cx| {
            // Ends when the script exits and drops the sender.
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
                .unwrap_or_else(|_| Some(Err("The script stopped.".into())));
            let _ = this.update(cx, |this, cx| this.finish(job, result, cx));
        })
        .detach();
    }

    fn finish(&mut self, job: Job, result: Option<Result<(), String>>, cx: &mut Context<Self>) {
        self.running = None;
        self.stop = None;
        self.ended = None;
        let id = job.pack.id;
        match result {
            None => {
                self.states.insert(id, State::Stopped);
            }
            Some(Ok(())) => {
                self.states.insert(id, State::Done(job.action));
                self.expanded.remove(id);
            }
            Some(Err(e)) => {
                self.states.insert(id, State::Failed(job.action, e));
                self.expanded.insert(id);
            }
        }
        // What it changed, read again; then the next one.
        self.reload(cx);
        self.pump(cx);
        cx.notify();
    }

    /// Stops the running script and everything it started; the queue moves on.
    fn stop_running(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.stop.take() {
            stop.abort();
        }
        cx.notify();
    }

    fn dequeue(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.queue.iter().position(|j| j.pack.id == id) {
            self.queue.remove(ix);
            self.states.remove(id);
        }
        cx.notify();
    }

    /// Quitting: stops the running script and what's queued, and has the app
    /// wait (as long as it waits for quit handlers) until its process group
    /// was told to end, as the Dev Tools page does.
    fn stop_for_quit(&mut self) -> impl Future<Output = ()> + use<> {
        self.queue.clear();
        if let Some(stop) = self.stop.take() {
            stop.abort();
        }
        let ended = self.ended.take();
        async move {
            if let Some(ended) = ended {
                let _ = ended.await;
            }
        }
    }

    // Sheet

    fn open_sheet(&mut self, pack: &'static Pack, window: &mut Window, cx: &mut Context<Self>) {
        let method = pack.methods[0];
        // What has it, to update or remove; else the agents the first way reaches.
        let mut selected: HashSet<usize> = self.having(pack).into_iter().collect();
        if selected.is_empty() {
            selected = (0..self.report.targets.len())
                .filter(|&t| method.reaches(&self.report.targets[t]))
                .take(1)
                .collect();
        }
        self.sheet = Some(Sheet {
            pack,
            method,
            selected,
        });
        self.sheet_focus.focus(window, cx);
        cx.notify();
    }

    fn close_sheet(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        self.refocus = true;
        cx.notify();
    }

    /// The ticked agents an action of the sheet acts on.
    fn picked(&self, pick: Pick) -> Vec<usize> {
        let Some(sheet) = &self.sheet else {
            return Vec::new();
        };
        let mut out: Vec<usize> = sheet
            .selected
            .iter()
            .copied()
            .filter(|&t| t < self.report.targets.len())
            .filter(|&t| {
                let reaches = sheet.method.reaches(&self.report.targets[t]);
                match (pick, self.status(sheet.pack, t)) {
                    (Pick::Install, Status::Installed { .. }) => false,
                    (Pick::Install, _) => reaches,
                    (Pick::Update, Status::Installed { .. }) => reaches,
                    (Pick::Update, _) => false,
                    (Pick::Remove, status) => status.any(),
                }
            })
            .collect();
        out.sort();
        out
    }

    /// The script `action` runs on `targets` from the sheet.
    fn sheet_script(&self, action: Action, targets: &[usize]) -> Result<Vec<String>, String> {
        let Some(sheet) = &self.sheet else {
            return Err("No sheet".into());
        };
        let places: Vec<skills::Place> = targets
            .iter()
            .map(|&t| (&self.report.targets[t], &self.report.installed[t]))
            .collect();
        skills::script(
            sheet.pack,
            sheet.method,
            action,
            &places,
            self.pm.unwrap_or(PackageManager::Npm),
            &self.env,
        )
    }

    /// Install or Update from the sheet: queues the script for the ticked agents.
    fn run_sheet(&mut self, action: Action, cx: &mut Context<Self>) {
        let Some(pack) = self.sheet.as_ref().map(|s| s.pack) else {
            return;
        };
        let pick = if action == Action::Update {
            Pick::Update
        } else {
            Pick::Install
        };
        match self.sheet_script(action, &self.picked(pick)) {
            Ok(lines) => {
                self.enqueue(pack, action, lines, cx);
                self.close_sheet(cx);
            }
            Err(e) => {
                self.states.insert(pack.id, State::Failed(action, e));
                cx.notify();
            }
        }
    }

    /// Asks, then removes `pack` from `targets`, naming them and what runs.
    fn confirm_remove(
        &mut self,
        pack: &'static Pack,
        targets: Vec<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let places: Vec<skills::Place> = targets
            .iter()
            .map(|&t| (&self.report.targets[t], &self.report.installed[t]))
            .collect();
        let lines = match skills::script(
            pack,
            pack.methods[0],
            Action::Remove,
            &places,
            PackageManager::Npm,
            &self.env,
        ) {
            Ok(lines) => lines,
            // Nothing of its own there (another pack's plugin brings its skills): say why.
            Err(why) => {
                let answer = window.prompt(
                    PromptLevel::Info,
                    &format!("Nothing to remove for {}", pack.name),
                    Some(&why),
                    &["OK"],
                    cx,
                );
                cx.spawn(async move |_, _| {
                    let _ = answer.await;
                })
                .detach();
                return;
            }
        };
        let names: Vec<&str> = targets
            .iter()
            .map(|&t| self.report.targets[t].label.as_str())
            .collect();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Remove {} from {}?",
                pack.name,
                plural(targets.len(), "agent", "agents")
            ),
            Some(&format!(
                "From: {}.\n\nRuns:\n{}",
                names.join(", "),
                lines.join("\n")
            )),
            &["Remove", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    this.sheet = None;
                    this.refocus = true;
                    this.enqueue(pack, Action::Remove, lines, cx);
                });
            }
        })
        .detach();
    }

    // The list

    fn matches(pack: &Pack, query: &str) -> bool {
        let has = |s: &str| s.to_lowercase().contains(query);
        query.is_empty()
            || has(pack.name)
            || has(pack.id)
            || has(pack.by)
            || has(pack.about)
            || has(pack.category.name())
            || pack.skills.iter().any(|s| has(s))
    }

    /// Matches the search and the category: what the filter tabs count.
    fn in_scope(&self, pack: &Pack, query: &str) -> bool {
        Self::matches(pack, query) && self.category.is_none_or(|c| pack.category == c)
    }

    fn passes(&self, pack: &Pack, filter: Filter) -> bool {
        match filter {
            Filter::All => true,
            Filter::Installed => !self.having(pack).is_empty(),
            Filter::Missing => self.having(pack).is_empty(),
        }
    }

    /// The packs the search, category, tab and sort leave.
    fn shown(&self, query: &str) -> Vec<&'static Pack> {
        let mut rows: Vec<(usize, &'static Pack)> = PACKS
            .iter()
            .enumerate()
            .filter(|(_, p)| self.in_scope(p, query) && self.passes(p, self.filter))
            .collect();
        match self.sort {
            Sort::Catalog => rows
                .sort_by_key(|(ix, p)| (Category::ALL.iter().position(|c| *c == p.category), *ix)),
            Sort::Name => rows.sort_by_key(|(_, p)| p.name.to_lowercase()),
            Sort::NameDesc => rows.sort_by_key(|(_, p)| std::cmp::Reverse(p.name.to_lowercase())),
            Sort::Installed => {
                rows.sort_by_key(|(ix, p)| (std::cmp::Reverse(self.having(p).len()), *ix))
            }
        }
        rows.into_iter().map(|(_, p)| p).collect()
    }

    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = Filter::All;
        self.category = None;
        self.paging.page = 0;
        if let Some(search) = &self.search {
            search.update(cx, |s, cx| s.set_value("", window, cx));
        }
        cx.notify();
    }

    // Rendering

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let chips = self.report.targets.iter().map(|t| {
            h_flex()
                .id(SharedString::from(format!(
                    "skills-agent-{}",
                    t.dir.display()
                )))
                .h(px(24.))
                .px_2()
                .gap_1p5()
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .text_xs()
                .when_some(t.clients.first(), |el, client| {
                    el.child(crate::mcp_page::client_logo(*client, 14., cx))
                })
                .child(t.label.clone())
                .tooltip({
                    let dir = tilde(&t.dir);
                    move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(dir.clone()).build(window, cx)
                    }
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
                            .child(div().text_xl().font_semibold().child("Skills"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .whitespace_normal()
                                    .child(
                                        "Agent skills from their official sources, installed into the agents on this Mac \
                                         the way each source documents. See the commands before they run; start a new \
                                         session of the agent to pick a skill up.",
                                    ),
                            ),
                    )
                    .child(
                        h_flex().flex_none().gap_2().child(
                            Button::new("skills-reload")
                                .ghost()
                                .small()
                                .icon(Lucide::RefreshCw)
                                .tooltip(if self.running.is_none() {
                                    "Detect Again"
                                } else {
                                    "Detect Again When the Installs Finish"
                                })
                                .loading(self.loading)
                                // A detection can reorder the agents under a running script.
                                .disabled(self.running.is_some())
                                .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                        ),
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
                                .child(if self.report.targets.is_empty() {
                                    "No agents with skills found on this Mac.".to_string()
                                } else {
                                    format!(
                                        "Installs into {} on this Mac:",
                                        plural(self.report.targets.len(), "skills folder", "skills folders")
                                    )
                                }),
                        )
                        .children(chips),
                )
            })
    }

    /// Search, the filter tabs with their counts, category and sort.
    fn render_toolbar(&self, query: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scope: Vec<&Pack> = PACKS.iter().filter(|p| self.in_scope(p, query)).collect();
        let choices = Filter::ALL
            .iter()
            .map(|f| {
                (
                    f.label(),
                    scope.iter().filter(|p| self.passes(p, *f)).count(),
                )
            })
            .collect();
        let current = Filter::ALL
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        let mut categories = vec!["All Categories"];
        categories.extend(Category::ALL.iter().map(|c| c.name()));
        let category = self
            .category
            .and_then(|c| Category::ALL.iter().position(|x| *x == c))
            .map_or(0, |ix| ix + 1);
        let sort = Sort::ALL.iter().position(|s| *s == self.sort).unwrap_or(0);
        h_flex()
            .gap_2()
            .flex_wrap()
            .when_some(self.search.as_ref(), |el, search| {
                el.child(catalog::search(search, cx))
            })
            .child(catalog::tabs(
                "skills-filter",
                choices,
                current,
                cx.listener(|this, ix: &usize, _, cx| {
                    this.filter = Filter::ALL[*ix];
                    this.paging.page = 0;
                    this.cursor = None;
                    cx.notify();
                }),
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .ml_auto()
                    .child(catalog::menu(
                        "skills-category",
                        Lucide::Funnel,
                        categories[category].to_string(),
                        categories,
                        category,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.category = ix.checked_sub(1).map(|ix| Category::ALL[ix]);
                            this.paging.page = 0;
                            this.cursor = None;
                            cx.notify();
                        }),
                    ))
                    .child(catalog::menu(
                        "skills-sort",
                        Lucide::ArrowUpDown,
                        format!("Sort: {}", self.sort.label()),
                        Sort::ALL.iter().map(|s| s.label()).collect(),
                        sort,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.sort = Sort::ALL[*ix];
                            this.paging.page = 0;
                            this.cursor = None;
                            cx.notify();
                        }),
                    )),
            )
    }

    fn render_row(
        &self,
        ix: usize,
        pack: &'static Pack,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let id = pack.id;
        let state = self.states.get(id).cloned();
        let total = self.report.targets.len();
        let having = self.having(pack);
        let full = having
            .iter()
            .filter(|&&t| matches!(self.status(pack, t), Status::Installed { .. }))
            .count();
        let busy = self.busy(id);

        let (mark, badge, color): (Mark, String, Hsla) = match &state {
            Some(State::Queued(_)) => (Mark::Icon(Lucide::Clock), "Queued".into(), muted),
            Some(State::Running(action)) => {
                (Mark::Spinner, format!("{}…", doing(*action)), theme.warning)
            }
            Some(State::Failed(..)) => (Mark::Icon(Lucide::CircleX), "Failed".into(), theme.danger),
            Some(State::Stopped) => (
                Mark::Icon(Lucide::CircleStop),
                "Stopped".into(),
                theme.warning,
            ),
            _ if self.loading => (Mark::Spinner, "Checking…".into(), muted),
            _ if having.is_empty() => (
                Mark::Icon(Lucide::CircleDashed),
                "Not installed".into(),
                muted,
            ),
            _ if full == 0 => (
                Mark::Icon(Lucide::CircleAlert),
                format!("Some skills · {} of {total}", having.len()),
                theme.warning,
            ),
            _ => (
                Mark::Icon(Lucide::CircleCheck),
                format!("Installed · {} of {total}", having.len()),
                theme.success,
            ),
        };

        let log = self.logs.get(id);
        let running = matches!(state, Some(State::Running(_)));
        let last_line = log.filter(|_| running).and_then(|l| l.last().cloned());
        let expanded = self.expanded.contains(id) && log.is_some();
        let failure = match &state {
            Some(State::Failed(_, e)) if !expanded => Some(e.clone()),
            _ => None,
        };
        let ways: Vec<&str> = pack.methods.iter().map(|m| m.label()).collect();
        let home = pack.home;
        // Under the name: past the tile and its gap.
        let indent = px(catalog::TILE + 12.);

        let action = match &state {
            Some(State::Running(_)) => Button::new(("skill-stop", ix))
                .outline()
                .small()
                .icon(Lucide::CircleStop)
                .label("Stop")
                .tooltip("Stop the Script")
                .on_click(cx.listener(|this, _, _, cx| this.stop_running(cx))),
            Some(State::Queued(_)) => Button::new(("skill-dequeue", ix))
                .ghost()
                .small()
                .icon(Lucide::X)
                .label("Cancel")
                .tooltip("Take It Out of the Queue")
                .on_click(cx.listener(move |this, _, _, cx| this.dequeue(id, cx))),
            _ => Button::new(("skill-open", ix))
                .when(having.is_empty(), |b| {
                    b.primary().icon(Lucide::Download).label("Install")
                })
                .when(!having.is_empty(), |b| {
                    b.outline().icon(Lucide::Settings2).label("Manage")
                })
                .small()
                .tooltip(if having.is_empty() {
                    "Choose the Agents and See the Commands"
                } else {
                    "Install, Update or Remove per Agent"
                })
                .disabled(self.loading || total == 0)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.open_sheet(pack, window, cx)),
                ),
        };

        v_flex()
            .px_4()
            .py_3()
            .gap_1p5()
            .border_b_1()
            .border_color(theme.border)
            .hover(|el| el.bg(theme.list_hover))
            .when(running, |el| el.bg(theme.list_hover))
            .when(selected, |el| el.bg(theme.list_active))
            .child(catalog::columns(
                None,
                catalog::tile(pack_logo(pack, cx), cx),
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(div().text_sm().font_semibold().child(pack.name))
                            .child(catalog::tag(None, pack.category.name(), cx))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("by {}", pack.by)),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(pack.about),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(mono.clone())
                            .text_color(muted)
                            .whitespace_normal()
                            .child(format!(
                                "{}: {}",
                                plural(pack.skills.len(), "skill", "skills"),
                                pack.skills.join(" · ")
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(mono.clone())
                                    .child(pack.repo.trim_start_matches("https://")),
                            )
                            .child(format!("· {}", ways.join(", ")))
                            .child(
                                Button::new(("skill-docs", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::ExternalLink)
                                    .tooltip("Open Its Docs")
                                    .on_click(move |_, _, cx| cx.open_url(home)),
                            ),
                    )
                    .when_some(pack.note, |el, note| {
                        el.child(
                            h_flex()
                                .gap_1p5()
                                .items_start()
                                .text_xs()
                                .text_color(muted)
                                .child(div().pt(px(1.)).child(Icon::new(Lucide::Info).xsmall()))
                                .child(div().whitespace_normal().child(note)),
                        )
                    }),
                catalog::pill(mark, badge, color),
                h_flex()
                    .gap_1()
                    .when(log.is_some(), |el| {
                        el.child(
                            Button::new(("skill-log", ix))
                                .ghost()
                                .small()
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
                    .when(!busy && !having.is_empty(), |el| {
                        let having = having.clone();
                        el.child(
                            Button::new(("skill-remove", ix))
                                .ghost()
                                .small()
                                .icon(Icon::new(Lucide::Trash).text_color(theme.danger))
                                .tooltip(format!("Remove {} from Every Agent", pack.name))
                                .disabled(self.loading)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_remove(pack, having.clone(), window, cx)
                                })),
                        )
                    })
                    .child(action),
            ))
            .when_some(last_line.filter(|_| !expanded), |el, line| {
                el.child(
                    div()
                        .pl(indent)
                        .text_xs()
                        .font_family(mono.clone())
                        .text_color(muted)
                        .truncate()
                        .child(line),
                )
            })
            .when_some(failure, |el, e| {
                el.child(
                    h_flex()
                        .pl(indent)
                        .gap_1p5()
                        .items_start()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(
                            div()
                                .pt(px(1.))
                                .child(Icon::new(Lucide::CircleAlert).xsmall()),
                        )
                        .child(div().whitespace_normal().child(e)),
                )
            })
            .when(expanded, |el| {
                let lines = log
                    .map(|l| l[l.len().saturating_sub(14)..].join("\n"))
                    .unwrap_or_default();
                el.child(
                    div()
                        .ml(indent)
                        .p_2()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.secondary)
                        .text_xs()
                        .font_family(mono)
                        .text_color(muted)
                        .whitespace_normal()
                        .child(lines),
                )
            })
            .into_any_element()
    }

    /// The list card: column titles, a page of packs and the pager, or the
    /// empty state.
    fn render_list(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let rows = self.shown(query);
        let total = rows.len();
        let card = v_flex()
            .debug_selector(|| "skills-list".into())
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden();
        if total == 0 {
            let title = if query.is_empty() {
                "No skills here".to_string()
            } else {
                format!("No skills match “{query}”")
            };
            return card
                .child(catalog::empty(
                    title,
                    Button::new("skills-clear-filters")
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
        let selected = self.cursor.and_then(|c| rows.get(c)).map(|p| p.id);
        let rows: Vec<_> = rows.into_iter().enumerate().collect();
        let (rows, paging) = catalog::page_of(rows, self.paging);
        card.child(catalog::header(None, "Skill", cx))
            .children(
                rows.into_iter()
                    .map(|(ix, p)| self.render_row(ix, p, selected == Some(p.id), cx)),
            )
            .child(catalog::pager(
                "skills",
                paging,
                total,
                if total == 1 { "pack" } else { "packs" },
                self.cursor,
                cx.listener(|this, paging: &catalog::Paging, _, cx| {
                    // A turn of the page leaves the selected row behind; a new size keeps it shown.
                    if paging.size == this.paging.size {
                        this.cursor = None;
                    }
                    this.paging = *paging;
                    cx.notify();
                }),
                cx,
            ))
            .into_any_element()
    }

    fn render_sheet(&self, sheet: &Sheet, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let pack = sheet.pack;
        let method = sheet.method;
        let install = self.picked(Pick::Install);
        let update = self.picked(Pick::Update);
        let remove = self.picked(Pick::Remove);
        let busy = self.busy(pack.id);
        let blocked = self.blocked(method);
        // What Install (else Update) would run.
        let preview = if !install.is_empty() {
            self.sheet_script(Action::Install, &install)
        } else if !update.is_empty() {
            self.sheet_script(Action::Update, &update)
        } else {
            Err("Tick the agents to install into.".to_string())
        };
        let ways = pack.methods.iter().copied().enumerate().map(|(ix, m)| {
            Button::new(("skills-way", ix))
                .ghost()
                .xsmall()
                .selected(m == method)
                .label(m.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(sheet) = &mut this.sheet {
                        sheet.method = m;
                    }
                    cx.notify();
                }))
        });
        let pm_menu = method
            .uses_package_manager()
            .then(|| {
                crate::package_managers::menu(
                    "skills-pm",
                    &self.pms,
                    Use::Run,
                    self.pm,
                    cx.listener(|this, pm: &PackageManager, _, cx| {
                        this.pm = Some(*pm);
                        crate::package_managers::remember(*pm);
                        cx.notify();
                    }),
                )
            })
            .flatten();
        let rows = self.report.targets.iter().enumerate().map(|(t, target)| {
            let status = self.status(pack, t);
            let reaches = method.reaches(target);
            let (badge, color) = match status {
                Status::Installed { plugin: true } => {
                    ("Installed (plugin)".to_string(), theme.success)
                }
                Status::Installed { plugin: false } => ("Installed".to_string(), theme.success),
                Status::Partial { have, of } => (format!("{have} of {of} skills"), theme.warning),
                Status::Missing if !reaches => ("Not with this way".to_string(), muted),
                Status::Missing => ("Not installed".to_string(), muted),
            };
            let checked = sheet.selected.contains(&t);
            h_flex()
                .px_3()
                .py_1p5()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div().w(px(200.)).flex_none().child(
                        Checkbox::new(("skills-target", t))
                            .label(target.label.clone())
                            .checked(checked)
                            .disabled(!reaches && !status.any())
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                if let Some(sheet) = &mut this.sheet {
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
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .font_family(mono.clone())
                        .child(tilde(&target.dir)),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .w(px(132.))
                        .flex_none()
                        .text_xs()
                        .text_color(color)
                        .child(div().size(px(6.)).rounded_full().bg(color))
                        .child(badge),
                )
        });
        let copy = preview.as_ref().ok().map(|lines| lines.join("\n"));
        v_flex()
            .w(px(760.))
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
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(pack_logo(pack, cx))
                            .child(
                                div()
                                    .text_base()
                                    .font_semibold()
                                    .flex_1()
                                    .child(format!("Install {}", pack.name)),
                            )
                            .child(
                                Button::new("skills-sheet-close")
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
                            .text_color(muted)
                            .whitespace_normal()
                            .child(format!(
                                "by {} · {} · {}",
                                pack.by,
                                plural(pack.skills.len(), "skill", "skills"),
                                pack.repo.trim_start_matches("https://")
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(div().text_xs().text_color(muted).child("Install with"))
                            .child(
                                h_flex()
                                    .p(px(2.))
                                    .gap(px(2.))
                                    .rounded(theme.radius)
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(theme.secondary)
                                    .children(ways),
                            )
                            .when_some(pm_menu, |el, menu| {
                                el.child(div().text_xs().text_color(muted).child("run by"))
                                    .child(menu)
                            }),
                    )
                    .when_some(blocked.clone(), |el, why| {
                        el.child(
                            h_flex()
                                .gap_1p5()
                                .items_start()
                                .text_xs()
                                .text_color(theme.warning)
                                .child(
                                    div()
                                        .pt(px(1.))
                                        .child(Icon::new(Lucide::TriangleAlert).xsmall()),
                                )
                                .child(div().whitespace_normal().child(why)),
                        )
                    }),
            )
            .child(
                div()
                    .id("skills-sheet-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(v_flex().children(rows)),
            )
            .child(
                v_flex()
                    .p_3()
                    .gap_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().text_xs().text_color(muted).child(
                                if !install.is_empty() {
                                    "Runs in your login shell:"
                                } else {
                                    "Updating runs:"
                                },
                            ))
                            .when_some(copy, |el, text| {
                                el.child(
                                    Button::new("skills-sheet-copy")
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::Copy)
                                        .tooltip("Copy the Commands")
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                text.clone(),
                                            ))
                                        }),
                                )
                            }),
                    )
                    .child(
                        div()
                            .debug_selector(|| "skills-sheet-commands".into())
                            .p_2()
                            .rounded(theme.radius)
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.secondary)
                            .text_xs()
                            .font_family(mono)
                            .text_color(if preview.is_ok() {
                                theme.foreground
                            } else {
                                muted
                            })
                            .whitespace_normal()
                            .child(match &preview {
                                Ok(lines) => lines.join("\n"),
                                Err(why) => why.clone(),
                            }),
                    ),
            )
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(div().flex_1())
                    .child(
                        Button::new("skills-sheet-remove")
                            .debug_selector(|| "skills-sheet-remove".into())
                            .danger()
                            .outline()
                            .small()
                            .label(format!("Remove from {}", remove.len()))
                            .disabled(remove.is_empty() || busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_remove(pack, remove.clone(), window, cx)
                            })),
                    )
                    .child(
                        Button::new("skills-sheet-update")
                            .outline()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .label(format!("Update {}", update.len()))
                            .tooltip("Fetch the Latest Where It's Installed")
                            .disabled(update.is_empty() || busy || blocked.is_some())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.run_sheet(Action::Update, cx)),
                            ),
                    )
                    .child(
                        Button::new("skills-sheet-install")
                            .debug_selector(|| "skills-sheet-install".into())
                            .primary()
                            .small()
                            .label(format!("Install to {}", install.len()))
                            .loading(busy)
                            .disabled(install.is_empty() || busy || blocked.is_some())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.run_sheet(Action::Install, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// What a sheet's button acts on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Pick {
    Install,
    Update,
    Remove,
}

/// "Installing", "Updating", "Removing".
fn doing(action: Action) -> &'static str {
    match action {
        Action::Install => "Installing",
        Action::Update => "Updating",
        Action::Remove => "Removing",
    }
}

/// Closing the tab stops the script that runs (and everything it started),
/// rather than leaving it running unseen with nobody to show its end.
impl Drop for SkillsPage {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.abort();
        }
    }
}

impl Render for SkillsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.search.is_none() {
            self.make_search(window, cx);
        }
        if std::mem::take(&mut self.refocus) {
            window.focus(&self.focus, cx);
        }
        let query = self.query(cx);
        let overlay = self
            .sheet
            .as_ref()
            .map(|sheet| self.render_sheet(sheet, cx));
        div()
            .relative()
            .size_full()
            .key_context("SkillsPage")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &catalog::FocusSearch, window, cx| {
                this.focus_search(window, cx)
            }))
            .on_action(cx.listener(|this, _: &catalog::SelectNext, window, cx| {
                this.select(true, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &catalog::SelectPrevious, window, cx| {
                    this.select(false, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &catalog::Confirm, window, cx| this.confirm(window, cx)),
            )
            .bg(cx.theme().background)
            .child(
                div()
                    .id("skills-page")
                    .size_full()
                    .overflow_y_scroll()
                    .child(
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
                        .id("skills-sheet")
                        .key_context("SkillsSheet")
                        .track_focus(&self.sheet_focus)
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
    use std::path::PathBuf;

    use gpui_kit::{
        AppContext as _, Bounds, Entity, TestAppContext, VisualTestContext, WindowBounds,
        WindowOptions, point, px, size,
    };
    use sawit_core::package_manager::{Available, PackageManager};
    use sawit_core::skills::{self, ClaudeHome, Installed, Report, Target};

    use super::{Action, SkillsPage, State};

    fn targets() -> Report {
        let claude = Target {
            label: "Claude Code".into(),
            clients: vec![],
            dir: "/Users/me/.claude/skills".into(),
            claude: Some(ClaudeHome {
                dir: "/Users/me/.claude".into(),
                profile_dir: None,
            }),
            cli_agent: Some("claude-code"),
        };
        let shared = Target {
            label: "Codex, Zed".into(),
            clients: vec![],
            dir: "/Users/me/.agents/skills".into(),
            claude: None,
            cli_agent: Some("codex"),
        };
        let has_cf = Installed {
            folder: ["cf-cli".to_string()].into(),
            ..Default::default()
        };
        Report {
            targets: vec![claude, shared],
            installed: vec![Installed::default(), has_cf],
        }
    }

    /// The page in a 1200 × 900 window with `report` detected, bun and npm installed.
    fn open(
        cx: &mut TestAppContext,
        setup: impl FnOnce(&mut SkillsPage),
    ) -> (Entity<SkillsPage>, &mut VisualTestContext) {
        open_at(cx, 1200., setup)
    }

    /// [`open`] in a window `width` wide.
    fn open_at(
        cx: &mut TestAppContext,
        width: f32,
        setup: impl FnOnce(&mut SkillsPage),
    ) -> (Entity<SkillsPage>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::catalog::init(cx);
            cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
        });
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(900.)),
            })),
            ..Default::default()
        };
        let (window, page) = cx
            .update(|cx| {
                gpui_kit::open_window(options, cx, |_, cx| {
                    cx.new(|cx| {
                        let mut page = SkillsPage::blank(cx);
                        page.report = targets();
                        page.loading = false;
                        page.claude = true;
                        page.git = true;
                        page.env = skills::Env {
                            home: "/Users/me".into(),
                            claude_config_dir_set: false,
                        };
                        page.pms = [PackageManager::Npm, PackageManager::Bun]
                            .into_iter()
                            .map(|pm| Available {
                                pm,
                                runner: PathBuf::from(format!("/bin/{}", pm.runner())),
                                version: None,
                            })
                            .collect();
                        page.pm = Some(PackageManager::Bun);
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

    /// The page fills the window as the other catalogs do: the list spans it, inside the 24 px
    /// inset, at the display's width and at the window's narrowest; no column caps it.
    #[gpui_kit::test]
    fn the_list_fills_the_window(cx: &mut TestAppContext) {
        for width in [1470., 720.] {
            let (_page, vcx) = open_at(cx, width, |_| {});
            let list = vcx.debug_bounds("skills-list").expect("the list is drawn");
            assert_eq!(f32::from(list.origin.x).round(), 24., "{width}");
            assert_eq!(f32::from(list.size.width).round(), width - 48., "{width}");
        }
    }

    /// A pack's sheet ticks the agents that have it, shows the exact commands with the package
    /// manager picked before anything runs, and changes them with the way it installs.
    #[gpui_kit::test]
    fn the_sheet_shows_the_commands_before_they_run(cx: &mut TestAppContext) {
        let (page, cx) = open(cx, |_| {});
        let mcp_apps = skills::get("mcp-apps").unwrap();
        cx.update(|window, cx| page.update(cx, |page, cx| page.open_sheet(mcp_apps, window, cx)));
        cx.run_until_parked();
        assert!(cx.debug_bounds("skills-sheet-commands").is_some());
        let script = |page: &Entity<SkillsPage>, cx: &mut VisualTestContext| {
            page.read_with(cx, |page, _| {
                page.sheet_script(Action::Install, &page.picked(super::Pick::Install))
            })
        };
        // Its plugin first, into Claude Code (the one agent it reaches).
        assert_eq!(
            script(&page, cx).unwrap(),
            [
                "claude plugin marketplace add modelcontextprotocol/ext-apps",
                "claude plugin install mcp-apps@mcp-apps"
            ]
        );
        // The skills CLI, run with bunx, into both.
        page.update(cx, |page, _| {
            let sheet = page.sheet.as_mut().unwrap();
            sheet.method = mcp_apps.methods[1];
            sheet.selected = [0, 1].into();
        });
        assert_eq!(
            script(&page, cx).unwrap(),
            [
                "bunx skills add modelcontextprotocol/ext-apps --skill '*' -g -a claude-code -a codex -y"
            ]
        );
        // Nothing ran.
        assert!(page.read_with(cx, |page, _| page.running.is_none()
            && page.queue.is_empty()));
    }

    /// The sheet opens with the agents that have the pack ticked: Update and Remove act on them,
    /// Install on none.
    #[gpui_kit::test]
    fn the_sheet_ticks_where_it_is_installed(cx: &mut TestAppContext) {
        let (page, cx) = open(cx, |_| {});
        let cf = skills::get("cf-cli").unwrap();
        cx.update(|window, cx| page.update(cx, |page, cx| page.open_sheet(cf, window, cx)));
        page.read_with(cx, |page, _| {
            assert_eq!(page.picked(super::Pick::Update), [1]);
            assert_eq!(page.picked(super::Pick::Remove), [1]);
            assert!(page.picked(super::Pick::Install).is_empty());
        });
    }

    /// Closing the Skills tab drops the page: the script it runs stops with it.
    #[gpui_kit::test]
    fn closing_the_page_stops_its_script(cx: &mut TestAppContext) {
        use futures::FutureExt as _;
        let (run, stop) = futures::future::abortable(futures::future::pending::<()>());
        let page = cx.new(|cx| {
            let mut page = SkillsPage::blank(cx);
            page.running = Some(super::Job {
                pack: skills::get("superpowers").unwrap(),
                action: Action::Install,
                lines: vec!["true".into()],
            });
            page.states
                .insert("superpowers", State::Running(Action::Install));
            page.stop = Some(stop);
            page
        });
        assert_eq!(
            page.read_with(cx, |page, _| page.work()).as_deref(),
            Some("Installing Superpowers")
        );
        drop(page);
        cx.update(|_| {});
        cx.run_until_parked();
        assert_eq!(run.now_or_never(), Some(Err(futures::future::Aborted)));
    }
}
