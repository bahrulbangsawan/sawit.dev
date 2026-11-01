//! Dev Tools page: developer tools that install with a cURL one-liner
//! (Claude Code, Bun, uv, …). Install one, a selection, or everything that
//! is missing; prerequisites (nvm before Node.js, uv before Python) are
//! queued first. Rows change state as soon as they are queued and show
//! "Installed" the moment the installer succeeds; the next detection
//! confirms it. Installed tools with a newer release offer an upgrade, any
//! installed tool can be uninstalled, and the installer, upgrade or
//! uninstaller that is running can be stopped.

use std::collections::{HashMap, HashSet, VecDeque};

use futures::StreamExt as _;
use futures::channel::oneshot;
use futures::future::{AbortHandle, abortable};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::dev_tools::{
    Category, Detection, Found, TOOLS, Tool, dependents, detect, find, in_terminal, is_newer,
    latest_versions, plan, run, uninstall, uninstall_command,
};

use crate::catalog::{self, Mark, tilde};
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
    Uninstalling,
    /// The installer succeeded; shown as installed before detection agrees.
    Done,
    /// The uninstaller succeeded; shown as removed before detection agrees.
    Removed,
    /// Stopped while it ran.
    Stopped,
    /// The run failed: why, and the job Retry runs again. `None` when there is none: a copy
    /// installed another way, found after its uninstall worked.
    Failed(String, Option<Job>),
    /// Not run because a prerequisite isn't installed.
    Skipped(&'static str),
    /// Handed to the terminal (it asks for a password).
    InTerminal,
}

#[derive(Debug, Clone, PartialEq)]
enum Job {
    Install,
    /// Runs this command.
    Uninstall(String),
}

/// Which rows the filter tabs show.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Filter {
    All,
    Installed,
    Updates,
    Missing,
}

impl Filter {
    const ALL: [Filter; 4] = [
        Filter::All,
        Filter::Installed,
        Filter::Updates,
        Filter::Missing,
    ];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Installed => "Installed",
            Filter::Updates => "Updates",
            Filter::Missing => "Not Installed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Sort {
    Category,
    Name,
    NameDesc,
    Status,
}

impl Sort {
    const ALL: [Sort; 4] = [Sort::Category, Sort::Name, Sort::NameDesc, Sort::Status];

    fn label(self) -> &'static str {
        match self {
            Sort::Category => "Category",
            Sort::Name => "Name (A–Z)",
            Sort::NameDesc => "Name (Z–A)",
            Sort::Status => "Needs Attention",
        }
    }
}

fn category_name(category: Category) -> &'static str {
    match category {
        Category::PackageManager => "Package Managers",
        Category::Runtime => "Languages & Runtimes",
        Category::Agent => "AI Agents",
        Category::Other => "Terminal & Apps",
    }
}

pub enum ToolsEvent {
    RunInTerminal(String),
}

impl EventEmitter<ToolsEvent> for ToolsPage {}

/// Every Dev Tools page, with the window it is in: what quitting, Restart to
/// Update and closing a window ask about ([`at_work`]).
#[derive(Default)]
struct Pages(Vec<(AnyWindowHandle, WeakEntity<ToolsPage>)>);

impl Global for Pages {}

/// What each Dev Tools page runs and has queued, with its window ("Installing
/// Bun, 2 more queued"): closing the window, quitting and Restart to Update
/// stop it ([`crate::terminal::end_all`]).
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

pub struct ToolsPage {
    found: HashMap<&'static str, Found>,
    /// Why the login shell couldn't say which tools it finds, when it
    /// couldn't: `found` has only the tools at their usual places then.
    problem: Option<String>,
    loading: bool,
    /// Latest released versions, by tool id.
    latest: HashMap<&'static str, String>,
    /// Queued or failed runs that upgrade instead of install.
    upgrades: HashSet<&'static str>,
    states: HashMap<&'static str, State>,
    logs: HashMap<&'static str, Vec<String>>,
    selected: HashSet<&'static str>,
    expanded: HashSet<&'static str>,
    queue: VecDeque<(&'static Tool, Job)>,
    running: Option<(&'static str, Job)>,
    /// Stops the running job.
    stop: Option<AbortHandle>,
    /// Ends once the running job is over, or stopped and its process group
    /// told to end: what quitting waits for.
    ended: Option<oneshot::Receiver<()>>,
    /// Quitting stops the running job ([`Self::stop_for_quit`]).
    _quit: Subscription,
    /// Progress through the current run of the queue.
    batch: (usize, usize),
    filter: Filter,
    category: Option<Category>,
    sort: Sort,
    /// The page of the list shown, from 0.
    page: usize,
    /// The search as the list last filtered by it.
    last_query: String,
    /// The row ↑ and ↓ select and ↩ acts on, by its place in the list.
    cursor: Option<usize>,
    focus: FocusHandle,
    /// Made with the page when it has a window, else on the first render.
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

impl ToolsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self::blank(cx);
        this.make_search(window, cx);
        this.refresh(cx);
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

    /// The search field: typing filters, ↩ moves the keyboard to the first row.
    fn make_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search tools"));
        self._search = Some(cx.subscribe_in(
            &search,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                // Only when the text changed: the field also says so when it loses the
                // keyboard.
                InputEvent::Change if this.query(cx) != this.last_query => {
                    this.last_query = this.query(cx);
                    this.page = 0;
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
        if self.search.is_none() {
            self.make_search(window, cx);
        }
        if let Some(search) = &self.search {
            search.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    /// Selects the next (or previous) row, turning the list's page to it, and
    /// gives the list the keyboard.
    fn select(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.shown(&self.query(cx)).len();
        self.cursor = catalog::step(self.cursor, len, down);
        if let Some(ix) = self.cursor {
            self.page = ix / catalog::PER_PAGE;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// ↩ on a row: what its main button does, when that's safe from one key: Install,
    /// Upgrade, Retry of an install or upgrade, and taking it out of the queue. Never
    /// Reinstall, Stop, Uninstall or Retry of an uninstall.
    fn confirm(&mut self, cx: &mut Context<Self>) {
        let rows = self.shown(&self.query(cx));
        let Some(&(_, tool)) = self.cursor.and_then(|ix| rows.get(ix)) else {
            return;
        };
        let id = tool.id;
        match self.states.get(id) {
            Some(State::Queued { .. }) => self.dequeue(id, cx),
            Some(State::Failed(_, Some(Job::Install)) | State::Skipped(_)) => {
                self.enqueue(&[id], cx)
            }
            // An uninstall that failed, or a copy found after it: not from one key, and never
            // an install over the copy the user meant to remove.
            Some(State::Failed(..) | State::Installing | State::Uninstalling) => {}
            _ if self.newer(id).is_some() => self.upgrade(&[id], cx),
            _ if !self.installed(id) && !self.busy(id) => self.enqueue(&[id], cx),
            _ => {}
        }
    }

    /// The page before anything is detected.
    fn blank(cx: &mut Context<Self>) -> Self {
        Self {
            found: HashMap::new(),
            problem: None,
            loading: true,
            latest: HashMap::new(),
            upgrades: HashSet::new(),
            states: HashMap::new(),
            logs: HashMap::new(),
            selected: HashSet::new(),
            expanded: HashSet::new(),
            queue: VecDeque::new(),
            running: None,
            stop: None,
            ended: None,
            _quit: cx.on_app_quit(|this, _| this.stop_for_quit()),
            batch: (0, 0),
            filter: Filter::All,
            category: None,
            sort: Sort::Category,
            page: 0,
            cursor: None,
            last_query: String::new(),
            focus: cx.focus_handle(),
            search: None,
            _search: None,
        }
    }

    /// A page running `id`'s installer, for tests elsewhere.
    #[cfg(test)]
    pub(crate) fn installing(id: &'static str, cx: &mut Context<Self>) -> Self {
        let mut page = Self::blank(cx);
        page.running = Some((id, Job::Install));
        page
    }

    /// [`Self::installing`], in `window` as a page the user opened is.
    #[cfg(test)]
    pub(crate) fn installing_in(id: &'static str, window: &Window, cx: &mut Context<Self>) -> Self {
        let page = Self::installing(id, cx);
        Self::register(window, cx);
        page
    }

    /// What is running and waiting, when anything is: "Installing Bun, 2
    /// more queued". Closing the tab or its window, and quitting, stop it.
    pub fn work(&self) -> Option<String> {
        let running = self.running.as_ref().and_then(|(id, job)| {
            let tool = find(id)?;
            Some(match job {
                Job::Install if self.upgrades.contains(id) => format!("Upgrading {}", tool.name),
                Job::Install => format!("Installing {}", tool.name),
                Job::Uninstall(_) => format!("Uninstalling {}", tool.name),
            })
        });
        let queued = self.queue.len();
        match (running, queued) {
            (None, 0) => None,
            (Some(running), 0) => Some(running),
            (Some(running), n) => Some(format!("{running}, {n} more queued")),
            (None, n) => Some(format!("{n} queued")),
        }
    }

    /// Detects again and rereads the latest versions.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reload(cx);
        let rx = rt::spawn(cx, latest_versions());
        cx.spawn(async move |this, cx| {
            if let Ok(latest) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.latest = latest;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Detects installed tools again (off the UI thread).
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let rx = rt::spawn(cx, detect());
        cx.spawn(async move |this, cx| {
            if let Ok(detection) = rx.await {
                let _ = this.update(cx, |this, cx| this.detected(detection, cx));
            }
        })
        .detach();
        cx.notify();
    }

    /// What detection found.
    fn detected(&mut self, detection: Detection, cx: &mut Context<Self>) {
        self.found = detection.found;
        self.problem = detection.problem;
        self.loading = false;
        // Detection now agrees with these; the rest keep their
        // optimistic or failed state.
        for (id, s) in self.states.iter_mut() {
            if *s == State::Removed
                && let Some(f) = self.found.get(id)
            {
                *s = State::Failed(
                    format!(
                        "Still found at {}: that copy was installed another way (Homebrew, say).",
                        tilde(&f.path)
                    ),
                    None,
                );
            }
        }
        self.states.retain(|id, s| {
            let found = self.found.contains_key(id);
            // A copy "still found" that is gone now: detection agrees with the uninstall.
            !(found && matches!(s, State::Done | State::InTerminal | State::Skipped(_))
                || !found && matches!(s, State::Removed | State::Failed(_, None)))
        });
        self.selected.retain(|id| !self.found.contains_key(id));
        cx.notify();
    }

    /// Installed per detection, or the installer just succeeded; not when
    /// the uninstaller just did.
    fn installed(&self, id: &str) -> bool {
        match self.states.get(id) {
            Some(State::Done) => true,
            Some(State::Removed) => false,
            _ => self.found.contains_key(id),
        }
    }

    /// The newer release an installed tool can upgrade to.
    fn newer(&self, id: &str) -> Option<&String> {
        let installed = self.found.get(id)?.version.as_ref()?;
        let latest = self.latest.get(id)?;
        (is_newer(latest, installed) && self.states.get(id) != Some(&State::Done)).then_some(latest)
    }

    /// Installed tools with a newer release, not already queued.
    fn upgradable(&self) -> Vec<&'static str> {
        TOOLS
            .iter()
            .filter(|t| self.newer(t.id).is_some() && !self.busy(t.id))
            .map(|t| t.id)
            .collect()
    }

    /// The command a run of `tool` uses: its upgrade or its installer.
    fn command(&self, tool: &Tool) -> &'static str {
        if self.upgrades.contains(tool.id) {
            tool.upgrade_command()
        } else {
            tool.install
        }
    }

    fn upgrade(&mut self, ids: &[&'static str], cx: &mut Context<Self>) {
        self.upgrades.extend(ids);
        self.enqueue(ids, cx);
    }

    fn busy(&self, id: &str) -> bool {
        matches!(
            self.states.get(id),
            Some(State::Queued { .. } | State::Installing | State::Uninstalling)
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
                cx.emit(ToolsEvent::RunInTerminal(self.command(tool).to_string()));
                self.upgrades.remove(tool.id);
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
            self.queue.push_back((tool, Job::Install));
            self.batch.1 += 1;
        }
        self.pump(cx);
        cx.notify();
    }

    /// Asks, then queues `tool`'s uninstaller (or hands it to the terminal
    /// when it asks for a password).
    fn confirm_uninstall(
        &mut self,
        tool: &'static Tool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self.uninstall_command(tool) else {
            return;
        };
        self.ask_uninstall(tool, command, window, cx);
    }

    /// Asks, then queues `command`, which removes `tool` (or hands it to the
    /// terminal when it asks for a password).
    fn ask_uninstall(
        &mut self,
        tool: &'static Tool,
        command: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let needed_by: Vec<&str> = dependents(tool.id)
            .into_iter()
            .filter(|t| self.installed(t.id))
            .map(|t| t.name)
            .collect();
        let mut detail = format!("Runs: {command}");
        if !needed_by.is_empty() {
            detail = format!(
                "{} need{} it and will stop working.\n\n{detail}",
                needed_by.join(", "),
                if needed_by.len() == 1 { "s" } else { "" }
            );
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Uninstall {}?", tool.name),
            Some(&detail),
            &["Uninstall", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    if in_terminal(tool, &command) {
                        this.states.insert(tool.id, State::InTerminal);
                        cx.emit(ToolsEvent::RunInTerminal(command));
                        cx.notify();
                        return;
                    }
                    if this.running.is_none() && this.queue.is_empty() {
                        this.batch = (0, 0);
                    }
                    this.selected.remove(tool.id);
                    this.states.insert(tool.id, State::Queued { for_: None });
                    this.queue.push_back((tool, Job::Uninstall(command)));
                    this.batch.1 += 1;
                    this.pump(cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Retry: runs the job that failed again. An install or upgrade (or one
    /// skipped for a missing prerequisite) is queued again; an uninstall asks
    /// again, as the trash button does, then runs the same command. Never the
    /// installer after an uninstall: that would put back what the user removes.
    fn retry(&mut self, tool: &'static Tool, window: &mut Window, cx: &mut Context<Self>) {
        match self.states.get(tool.id) {
            Some(State::Failed(_, Some(Job::Uninstall(command)))) => {
                let command = command.clone();
                self.ask_uninstall(tool, command, window, cx);
            }
            Some(State::Failed(_, Some(Job::Install)) | State::Skipped(_)) => {
                self.enqueue(&[tool.id], cx)
            }
            _ => {}
        }
    }

    /// How to remove the copy detection found, if Sawit can.
    fn uninstall_command(&self, tool: &Tool) -> Option<String> {
        uninstall_command(tool, &self.found.get(tool.id)?.path)
    }

    /// Starts the next queued installer unless one is running.
    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let (tool, job) = loop {
            let Some((tool, job)) = self.queue.pop_front() else {
                // The queue is done: let detection confirm the results.
                if self.batch.1 > 0 {
                    self.reload(cx);
                }
                return;
            };
            if matches!(job, Job::Uninstall(_)) {
                break (tool, job);
            }
            match tool.needs.iter().find(|n| !self.installed(n)) {
                Some(need) => {
                    let name = find(need).map_or(*need, |t| t.name);
                    self.states.insert(tool.id, State::Skipped(name));
                    self.batch.0 += 1;
                }
                None => break (tool, job),
            }
        };
        let id = tool.id;
        self.running = Some((id, job.clone()));
        let (state, command) = match &job {
            Job::Install => (State::Installing, self.command(tool).to_string()),
            Job::Uninstall(command) => (State::Uninstalling, command.clone()),
        };
        self.states.insert(id, state);
        self.logs.insert(id, vec![format!("$ {command}")]);
        let finished = job.clone();
        let (tx, mut lines) = futures::channel::mpsc::unbounded::<String>();
        let (run, stop) = abortable(async move {
            let on_line = move |line| {
                let _ = tx.unbounded_send(line);
            };
            match job {
                Job::Install => run(tool, &command, on_line).await,
                Job::Uninstall(command) => uninstall(tool, &command, on_line).await,
            }
            .map_err(|e| e.to_string())
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
                .unwrap_or_else(|_| Some(Err("The installer stopped.".into())));
            let _ = this.update(cx, |this, cx| this.finish(id, finished, result, cx));
        })
        .detach();
    }

    fn finish(
        &mut self,
        id: &'static str,
        job: Job,
        result: Option<Result<(), String>>,
        cx: &mut Context<Self>,
    ) {
        self.running = None;
        self.stop = None;
        self.ended = None;
        self.batch.0 += 1;
        match result {
            None => {
                self.states.insert(id, State::Stopped);
            }
            Some(Ok(())) => {
                self.upgrades.remove(id);
                let done = match job {
                    Job::Install => State::Done,
                    Job::Uninstall(_) => State::Removed,
                };
                self.states.insert(id, done);
                self.expanded.remove(id);
            }
            Some(Err(e)) => {
                self.states.insert(id, State::Failed(e, Some(job)));
                self.expanded.insert(id);
            }
        }
        self.pump(cx);
        cx.notify();
    }

    /// Stops the running installer or uninstaller and everything it
    /// started; the queue moves on.
    fn stop_running(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.stop.take() {
            stop.abort();
        }
        cx.notify();
    }

    /// Quitting: stops the running job and what's queued, as closing the page
    /// does, and has the app wait (as long as it waits for quit handlers) until
    /// the job's process group was told to end. The page's drop, which also
    /// stops it, comes as the app lets go of its windows, and the app could
    /// exit before the stop got through: the installer, in a session of its
    /// own, then ran on with nobody to show its end.
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

    /// Drops everything still waiting; the running installer finishes.
    fn stop_queue(&mut self, cx: &mut Context<Self>) {
        for (tool, _) in self.queue.drain(..) {
            self.states.remove(tool.id);
            self.upgrades.remove(tool.id);
        }
        self.batch.1 = self.batch.0 + usize::from(self.running.is_some());
        cx.notify();
    }

    fn dequeue(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.queue.iter().position(|(t, _)| t.id == id) {
            self.queue.remove(ix);
            self.states.remove(id);
            self.upgrades.remove(id);
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
            || has(category_name(tool.category))
    }

    /// Matches the search and the category: what the filter tabs count.
    fn in_scope(&self, tool: &Tool, query: &str) -> bool {
        Self::matches(tool, query) && self.category.is_none_or(|c| tool.category == c)
    }

    fn passes(&self, tool: &Tool, filter: Filter) -> bool {
        match filter {
            Filter::All => true,
            Filter::Installed => self.installed(tool.id),
            Filter::Updates => self.newer(tool.id).is_some(),
            Filter::Missing => !self.installed(tool.id),
        }
    }

    /// Sorts by what needs attention: running or failed, then updates,
    /// then missing, then installed.
    fn rank(&self, tool: &Tool) -> u8 {
        match self.states.get(tool.id) {
            Some(State::Removed) | None => {}
            Some(State::Done) => return 3,
            Some(_) => return 0,
        }
        if self.newer(tool.id).is_some() {
            1
        } else if self.installed(tool.id) {
            3
        } else {
            2
        }
    }

    /// The rows the search, category, tab and sort leave, with their index
    /// in `TOOLS`.
    fn shown(&self, query: &str) -> Vec<(usize, &'static Tool)> {
        let mut rows: Vec<(usize, &'static Tool)> = TOOLS
            .iter()
            .enumerate()
            .filter(|(_, t)| self.in_scope(t, query) && self.passes(t, self.filter))
            .collect();
        match self.sort {
            Sort::Category => rows.sort_by_key(|(ix, t)| (t.category as u8, *ix)),
            Sort::Name => rows.sort_by_key(|(_, t)| t.name.to_lowercase()),
            Sort::NameDesc => rows.sort_by_key(|(_, t)| std::cmp::Reverse(t.name.to_lowercase())),
            Sort::Status => rows.sort_by_key(|(ix, t)| (self.rank(t), *ix)),
        }
        rows
    }

    /// Selectable rows (missing, idle, not handed to the terminal).
    fn selectable(&self, rows: &[(usize, &'static Tool)]) -> Vec<&'static str> {
        rows.iter()
            .filter(|(_, t)| !t.interactive && !self.installed(t.id) && !self.busy(t.id))
            .map(|(_, t)| t.id)
            .collect()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let missing = self.missing();
        let upgradable = self.upgradable();
        h_flex()
            .gap_4()
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_xl().font_semibold().child("Dev Tools"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(
                                "Official installers, run one at a time in your login shell, prerequisites first. \
                                 Open a new terminal tab afterwards to pick up the new PATH.",
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(
                        Button::new("tools-reload")
                            .ghost()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Detect Again")
                            .loading(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    )
                    .when(!upgradable.is_empty(), |el| {
                        el.child(
                            Button::new("tools-upgrade-all")
                                .outline()
                                .small()
                                .icon(Lucide::CircleArrowUp)
                                .label(format!("Upgrade All ({})", upgradable.len()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.upgrade(&upgradable, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new("tools-install-all")
                            .primary()
                            .small()
                            .icon(Lucide::Download)
                            .label(format!("Install Missing ({})", missing.len()))
                            // Unknown which are missing: it could run installers over copies
                            // that are there.
                            .disabled(missing.is_empty() || self.loading || self.problem.is_some())
                            .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&missing, cx))),
                    ),
            )
    }

    /// Says when the login shell couldn't tell which tools are installed,
    /// instead of a page of "Not installed".
    fn render_problem(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let problem = self.problem.as_ref().filter(|_| !self.loading)?;
        let warning = cx.theme().warning;
        Some(
            h_flex()
                .px_4()
                .py_2()
                .gap_2()
                .items_start()
                .rounded(cx.theme().radius_lg)
                .border_1()
                .border_color(warning.opacity(0.35))
                .bg(warning.opacity(0.08))
                .text_sm()
                .child(
                    div()
                        .pt(px(2.))
                        .child(Icon::new(Lucide::TriangleAlert).small().text_color(warning)),
                )
                .child(div().flex_1().min_w_0().whitespace_normal().child(format!(
                    "Couldn't ask your login shell which tools are installed ({problem}). Only tools in \
                     their usual folders show as installed, so Install Missing is off until Detect Again \
                     works."
                )))
                .into_any_element(),
        )
    }

    /// Search, the filter tabs with their counts, category and sort.
    fn render_toolbar(&self, query: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scope: Vec<&Tool> = TOOLS.iter().filter(|t| self.in_scope(t, query)).collect();
        let choices = Filter::ALL
            .iter()
            .map(|f| {
                (
                    f.label(),
                    scope.iter().filter(|t| self.passes(t, *f)).count(),
                )
            })
            .collect();
        let current = Filter::ALL
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        let mut categories = vec!["All Categories"];
        categories.extend(Category::ALL.iter().map(|c| category_name(*c)));
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
                "tools-filter",
                choices,
                current,
                cx.listener(|this, ix: &usize, _, cx| {
                    this.filter = Filter::ALL[*ix];
                    this.page = 0;
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
                        "tools-category",
                        Lucide::Funnel,
                        categories[category].to_string(),
                        categories,
                        category,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.category = ix.checked_sub(1).map(|ix| Category::ALL[ix]);
                            this.page = 0;
                            this.cursor = None;
                            cx.notify();
                        }),
                    ))
                    .child(catalog::menu(
                        "tools-sort",
                        Lucide::ArrowUpDown,
                        format!("Sort: {}", self.sort.label()),
                        Sort::ALL.iter().map(|s| s.label()).collect(),
                        sort,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.sort = Sort::ALL[*ix];
                            this.page = 0;
                            this.cursor = None;
                            cx.notify();
                        }),
                    )),
            )
    }

    /// "3 selected" with Clear and Install, while anything is ticked.
    fn render_selection(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let selected: Vec<&'static str> = TOOLS
            .iter()
            .filter(|t| self.selected.contains(t.id))
            .map(|t| t.id)
            .collect();
        if selected.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let extra = self
            .plan_for(&selected)
            .len()
            .saturating_sub(selected.len());
        Some(
            h_flex()
                .px_4()
                .py_2()
                .gap_2()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(theme.primary.opacity(0.35))
                .bg(theme.primary.opacity(0.08))
                .text_sm()
                .child(
                    Icon::new(Lucide::SquareCheck)
                        .small()
                        .text_color(theme.primary),
                )
                .child(
                    div()
                        .font_medium()
                        .child(format!("{} selected", selected.len())),
                )
                .when(extra > 0, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("+ {} needed first", extra)),
                    )
                })
                .child(div().flex_1())
                .child(
                    Button::new("tools-clear-selection")
                        .ghost()
                        .small()
                        .icon(Lucide::X)
                        .label("Clear")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.selected.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("tools-install-selected")
                        .primary()
                        .small()
                        .icon(Lucide::Download)
                        .label(format!("Install {}", selected.len() + extra))
                        .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&selected, cx))),
                )
                .into_any_element(),
        )
    }

    /// "Installing Node.js · 2 of 5" and a bar, while the queue runs.
    fn render_progress(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (done, total) = self.batch;
        let running = self
            .running
            .as_ref()
            .and_then(|(id, job)| Some((find(id)?, job)));
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
                .px_4()
                .py_3()
                .rounded(cx.theme().radius_lg)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(Spinner::new().xsmall().color(muted))
                        .when_some(running, |el, (t, _)| el.child(logo(t.id, 16., cx)))
                        .child(div().flex_1().font_medium().child(match running {
                            Some((t, Job::Install)) => format!(
                                "{} {} · {} of {total}",
                                if self.upgrades.contains(t.id) {
                                    "Upgrading"
                                } else {
                                    "Installing"
                                },
                                t.name,
                                done + 1
                            ),
                            Some((t, Job::Uninstall(_))) => {
                                format!("Uninstalling {} · {} of {total}", t.name, done + 1)
                            }
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
                                    .outline()
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

    fn render_row(
        &self,
        ix: usize,
        tool: &'static Tool,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let id = tool.id;
        let state = self.states.get(id).cloned();
        let found = self.found.get(id);
        let installed = self.installed(id);
        let busy = self.busy(id);
        let newer = self.newer(id).cloned();
        let upgrading = self.upgrades.contains(id);

        let (mark, badge, color): (Mark, String, Hsla) = match (&state, found) {
            (Some(State::Queued { for_: Some(f) }), _) => {
                (Mark::Icon(Lucide::Clock), format!("Queued for {f}"), muted)
            }
            (Some(State::Queued { for_: None }), _) => {
                (Mark::Icon(Lucide::Clock), "Queued".into(), muted)
            }
            (Some(State::Installing), _) if upgrading => {
                (Mark::Spinner, "Upgrading…".into(), theme.warning)
            }
            (Some(State::Installing), _) => (Mark::Spinner, "Installing…".into(), theme.warning),
            (Some(State::Uninstalling), _) => {
                (Mark::Spinner, "Uninstalling…".into(), theme.warning)
            }
            (Some(State::Removed), _) => (
                Mark::Icon(Lucide::CircleMinus),
                "Removed · restart shell".into(),
                muted,
            ),
            (Some(State::Stopped), _) => (
                Mark::Icon(Lucide::CircleStop),
                "Stopped".into(),
                theme.warning,
            ),
            (Some(State::Failed(..)), _) => {
                (Mark::Icon(Lucide::CircleX), "Failed".into(), theme.danger)
            }
            (Some(State::Skipped(need)), _) => (
                Mark::Icon(Lucide::TriangleAlert),
                format!("Skipped · needs {need}"),
                theme.danger,
            ),
            (Some(State::InTerminal), _) => (
                Mark::Icon(Lucide::SquareTerminal),
                "In terminal".into(),
                theme.warning,
            ),
            (Some(State::Done), None) if !self.loading => (
                Mark::Icon(Lucide::CircleCheck),
                "Installed · restart shell".into(),
                theme.success,
            ),
            (
                _,
                Some(Found {
                    version: Some(v), ..
                }),
            ) if newer.is_some() => (
                Mark::Icon(Lucide::CircleArrowUp),
                format!("Update · {v} → {}", newer.as_deref().unwrap_or_default()),
                theme.info,
            ),
            (_, Some(f)) => (
                Mark::Icon(Lucide::CircleCheck),
                f.version
                    .as_ref()
                    .map_or("Installed".into(), |v| format!("Installed · {v}")),
                theme.success,
            ),
            (Some(State::Done), None) => (
                Mark::Icon(Lucide::CircleCheck),
                "Installed".into(),
                theme.success,
            ),
            (None, None) if self.loading => (Mark::Spinner, "Checking…".into(), muted),
            (None, None) => (
                Mark::Icon(Lucide::CircleDashed),
                "Not installed".into(),
                muted,
            ),
        };

        let retry = || {
            Button::new(("tool-retry", ix))
                .primary()
                .small()
                .icon(Lucide::RotateCcw)
                .label("Retry")
        };
        let action = match &state {
            Some(State::Installing | State::Uninstalling) => Some(
                Button::new(("tool-stop", ix))
                    .outline()
                    .small()
                    .icon(Lucide::CircleStop)
                    .label("Stop")
                    .tooltip(match state {
                        Some(State::Installing) if upgrading => "Stop the Upgrade",
                        Some(State::Installing) => "Stop the Installer",
                        _ => "Stop the Uninstaller",
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.stop_running(cx))),
            ),
            Some(State::Queued { .. }) => Some(
                Button::new(("tool-dequeue", ix))
                    .ghost()
                    .small()
                    .icon(Lucide::X)
                    .label("Cancel")
                    .tooltip("Take It Out of the Queue")
                    .on_click(cx.listener(move |this, _, _, cx| this.dequeue(id, cx))),
            ),
            // A copy found after its uninstall worked: nothing to run again (the trash button
            // offers that copy's removal when Sawit has one).
            Some(State::Failed(_, None)) => None,
            Some(State::Failed(_, Some(Job::Uninstall(_)))) => Some(
                retry()
                    .tooltip("Run the Uninstaller Again")
                    .on_click(cx.listener(move |this, _, window, cx| this.retry(tool, window, cx))),
            ),
            Some(State::Failed(..) | State::Skipped(_)) => Some(
                retry()
                    .on_click(cx.listener(move |this, _, window, cx| this.retry(tool, window, cx))),
            ),
            _ if newer.is_some() => Some(
                Button::new(("tool-upgrade", ix))
                    .primary()
                    .small()
                    .icon(Lucide::CircleArrowUp)
                    .label("Upgrade")
                    .tooltip(tool.upgrade_command())
                    .on_click(cx.listener(move |this, _, _, cx| this.upgrade(&[id], cx))),
            ),
            _ if installed => Some(
                Button::new(("tool-reinstall", ix))
                    .outline()
                    .small()
                    .icon(Lucide::RotateCw)
                    .label("Reinstall")
                    .tooltip("Run the Installer Again (Updates to the Latest Version)")
                    .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
            ),
            _ if tool.interactive => Some(
                Button::new(("tool-terminal", ix))
                    .primary()
                    .small()
                    .icon(Lucide::SquareTerminal)
                    .label("Install in Terminal")
                    .tooltip("Asks for Your Password, So It Runs in the Terminal")
                    .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
            ),
            _ => Some(
                Button::new(("tool-install", ix))
                    .primary()
                    .small()
                    .icon(Lucide::Download)
                    .label("Install")
                    .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&[id], cx))),
            ),
        };

        let can_uninstall = installed && !busy && self.uninstall_command(tool).is_some();
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
                .h(px(18.))
                .gap_1()
                .px_1p5()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(if ok {
                    cx.theme().success.opacity(0.4)
                } else {
                    cx.theme().border
                })
                .text_xs()
                .text_color(if ok { cx.theme().success } else { muted })
                .child(logo(need.id, 12., cx))
                .child(format!("needs {}", need.name))
                .when(ok, |el| el.child(Icon::new(Lucide::Check).xsmall()))
        });

        let log = self.logs.get(id);
        let running = matches!(state, Some(State::Installing | State::Uninstalling));
        let last_line = log.filter(|_| running).and_then(|l| l.last().cloned());
        let expanded = self.expanded.contains(id) && log.is_some();
        let command = if newer.is_some() || upgrading {
            tool.upgrade_command()
        } else {
            tool.install
        };
        // Under the name: past the checkbox, the tile and their gaps.
        let indent = px(16. + catalog::TILE + 24.);

        v_flex()
            .px_4()
            .py_3()
            .gap_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .hover(|el| el.bg(cx.theme().list_hover))
            .when(running, |el| el.bg(cx.theme().list_hover))
            .when(selected, |el| el.bg(cx.theme().list_active))
            .child(catalog::columns(
                // Level with the middle of the tile.
                Some(
                    h_flex()
                        .h(px(catalog::TILE))
                        .child(checkbox)
                        .into_any_element(),
                ),
                catalog::tile(logo(id, 22., cx), cx),
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(div().text_sm().font_semibold().child(tool.name))
                            .child(catalog::tag(None, category_name(tool.category), cx))
                            .children(needs),
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
                                    .tooltip(if command == tool.install {
                                        "Copy Install Command"
                                    } else {
                                        "Copy Upgrade Command"
                                    })
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            command.to_string(),
                                        ))
                                    }),
                            ),
                    ),
                catalog::pill(mark, badge, color),
                h_flex()
                    .gap_1()
                    .when(log.is_some(), |el| {
                        el.child(
                            Button::new(("tool-log", ix))
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
                    .when(can_uninstall, |el| {
                        el.child(
                            Button::new(("tool-uninstall", ix))
                                .ghost()
                                .small()
                                .icon(Icon::new(Lucide::Trash).text_color(cx.theme().danger))
                                .tooltip(format!("Uninstall {}", tool.name))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_uninstall(tool, window, cx)
                                })),
                        )
                    })
                    .children(action),
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
            .when_some(
                match &state {
                    Some(State::Failed(e, _)) if !expanded => Some(e.clone()),
                    _ => None,
                },
                |el, e| {
                    el.child(
                        h_flex()
                            .pl(indent)
                            .gap_1p5()
                            .items_start()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(
                                div()
                                    .pt(px(1.))
                                    .child(Icon::new(Lucide::CircleAlert).xsmall()),
                            )
                            .child(div().whitespace_normal().child(e)),
                    )
                },
            )
            .when(expanded, |el| {
                let lines = log
                    .map(|l| l[l.len().saturating_sub(14)..].join("\n"))
                    .unwrap_or_default();
                el.child(
                    div()
                        .ml(indent)
                        .p_2()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
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

    /// The list card: column titles, a page of rows and the pager, or the
    /// empty state.
    fn render_list(&self, query: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let rows = self.shown(query);
        let total = rows.len();
        let selectable = self.selectable(&rows);
        let all = !selectable.is_empty() && selectable.iter().all(|id| self.selected.contains(id));
        let pick_all = Checkbox::new("tools-pick-all")
            .checked(all)
            .disabled(selectable.is_empty())
            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                for id in &selectable {
                    if *checked {
                        this.selected.insert(*id);
                    } else {
                        this.selected.remove(*id);
                    }
                }
                cx.notify();
            }));
        let selected = self.cursor.and_then(|c| rows.get(c)).map(|(_, t)| t.id);
        let (rows, page) = catalog::page_of(rows, self.page);
        let card = v_flex()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden();
        if total == 0 {
            let title = match self.query(cx) {
                q if q.is_empty() => "No tools here".to_string(),
                q => format!("No tools match “{q}”"),
            };
            return card.child(catalog::empty(
                title,
                Button::new("tools-clear-filters")
                    .outline()
                    .small()
                    .icon(Lucide::FunnelX)
                    .label("Clear Filters")
                    .on_click(cx.listener(|this, _, window, cx| this.clear_filters(window, cx))),
                cx,
            ));
        }
        card.child(catalog::header(
            Some(pick_all.into_any_element()),
            "Tool",
            cx,
        ))
        .children(
            rows.into_iter()
                .map(|(ix, t)| self.render_row(ix, t, selected == Some(t.id), cx)),
        )
        .child(catalog::pager(
            "tools-pages",
            page,
            total,
            if total == 1 { "tool" } else { "tools" },
            cx.listener(|this, page: &usize, _, cx| {
                this.page = *page;
                this.cursor = None;
                cx.notify();
            }),
            cx,
        ))
    }

    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = Filter::All;
        self.category = None;
        self.page = 0;
        if let Some(search) = &self.search {
            search.update(cx, |s, cx| s.set_value("", window, cx));
        }
        cx.notify();
    }
}

/// Closing the tab stops the installer that runs (and everything it started),
/// rather than leaving it running unseen with nobody to show its end.
impl Drop for ToolsPage {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.abort();
        }
    }
}

impl Render for ToolsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.search.is_none() {
            self.make_search(window, cx);
        }
        let query = self.query(cx);
        div()
            .id("tools-page")
            .key_context("ToolsPage")
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
            .on_action(cx.listener(|this, _: &catalog::Confirm, _, cx| this.confirm(cx)))
            .size_full()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .p_6()
                    .gap_4()
                    .child(self.render_header(cx))
                    .children(self.render_problem(cx))
                    .child(self.render_toolbar(&query, cx))
                    .children(self.render_progress(cx))
                    .children(self.render_selection(cx))
                    .child(self.render_list(&query, cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt as _;
    use futures::future::{Aborted, abortable};
    use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
    use sawit_core::dev_tools::{Detection, Found, find};

    use super::{Job, State, ToolsPage};

    /// Bun's own removal, as its row's trash button runs it.
    fn remove_bun() -> Job {
        Job::Uninstall(r#"rm -rf "$HOME/.bun""#.into())
    }

    /// A Dev Tools page in a window of its own, with Bun installed at `~/.bun`. The tests make
    /// another tool's installer "run" before anything is queued, so nothing they queue starts and
    /// no script runs here.
    fn page_with_bun(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ToolsPage>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::catalog::init(cx);
        });
        cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |_, cx| {
                cx.new(|cx| {
                    let mut page = ToolsPage::blank(cx);
                    page.loading = false;
                    page.found.insert(
                        "bun",
                        Found {
                            path: "/Users/me/.bun/bin/bun".into(),
                            version: Some("1.2.0".into()),
                        },
                    );
                    page
                })
            })
        })
        .unwrap()
    }

    /// Puts ↩'s selection on Bun's row, with Deno's installer "running".
    fn select_bun_while_another_runs(page: &mut ToolsPage) {
        page.running = Some(("deno", Job::Install));
        page.cursor = page.shown("").iter().position(|(_, t)| t.id == "bun");
    }

    fn queued(cx: &mut TestAppContext, page: &Entity<ToolsPage>) -> Vec<(&'static str, Job)> {
        page.read_with(cx, |page, _| {
            page.queue
                .iter()
                .map(|(tool, job)| (tool.id, job.clone()))
                .collect()
        })
    }

    /// Retry, as the row's button runs it.
    fn retry_bun(cx: &mut TestAppContext, window: AnyWindowHandle, page: &Entity<ToolsPage>) {
        let bun = find("bun").unwrap();
        cx.update_window(window, |_, window, cx| {
            page.update(cx, |page, cx| page.retry(bun, window, cx))
        })
        .unwrap();
        cx.run_until_parked();
    }

    /// CATALOG M4: after an uninstall fails, the row says Failed, and neither its Retry nor ↩
    /// may run the tool's installer, a `curl | sh` over the copy the user tried to remove (both
    /// queued an install). Retry asks again, as the trash button does, then runs the uninstall
    /// that failed; ↩ leaves the row alone, since one key never removes anything.
    #[gpui_kit::test]
    fn retrying_a_failed_uninstall_never_runs_the_installer(cx: &mut TestAppContext) {
        let (window, page) = page_with_bun(cx);
        page.update(cx, |page, cx| {
            page.running = Some(("bun", remove_bun()));
            page.finish(
                "bun",
                remove_bun(),
                Some(Err("Permission denied".into())),
                cx,
            );
            select_bun_while_another_runs(page);
            page.confirm(cx);
        });
        assert_eq!(queued(cx, &page), [], "↩ after the uninstall failed");

        retry_bun(cx, window, &page);
        assert_eq!(queued(cx, &page), [], "Retry ran something without asking");
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Uninstall Bun?".to_string(),
                r#"Runs: rm -rf "$HOME/.bun""#.to_string()
            )),
            "Retry didn't ask to uninstall again"
        );
        cx.simulate_prompt_answer("Uninstall");
        cx.run_until_parked();
        assert_eq!(queued(cx, &page), [("bun", remove_bun())]);
    }

    /// An uninstall that worked, after which detection still finds a copy installed another way
    /// (Homebrew's): the row says so, as Failed, and has nothing to run again. Retry and ↩ used
    /// to queue the installer over that copy.
    #[gpui_kit::test]
    fn a_copy_found_after_its_uninstall_is_never_installed_over(cx: &mut TestAppContext) {
        let (window, page) = page_with_bun(cx);
        page.update(cx, |page, cx| {
            page.running = Some(("bun", remove_bun()));
            page.finish("bun", remove_bun(), Some(Ok(())), cx);
            let homebrew = Found {
                path: "/opt/homebrew/bin/bun".into(),
                version: Some("1.2.0".into()),
            };
            page.detected(
                Detection {
                    found: [("bun", homebrew)].into(),
                    problem: None,
                },
                cx,
            );
            assert!(
                matches!(page.states.get("bun"), Some(State::Failed(..))),
                "{:?}",
                page.states.get("bun")
            );
            select_bun_while_another_runs(page);
            page.confirm(cx);
        });
        assert_eq!(queued(cx, &page), [], "↩ on the copy still found");
        retry_bun(cx, window, &page);
        assert!(!cx.has_pending_prompt());
        assert_eq!(queued(cx, &page), [], "Retry on the copy still found");
        // Once that copy is gone too, the row is a missing tool's again, with Install.
        page.update(cx, |page, cx| {
            page.detected(
                Detection {
                    found: Default::default(),
                    problem: None,
                },
                cx,
            )
        });
        assert_eq!(
            page.read_with(cx, |page, _| page.states.get("bun").cloned()),
            None
        );
    }

    /// Closing the Dev Tools tab drops the page: the installer it runs stops with it, instead
    /// of running on unseen, its queue gone, while a reopened page offers to start it again.
    #[gpui_kit::test]
    fn closing_the_page_stops_its_installer(cx: &mut TestAppContext) {
        let (run, stop) = abortable(futures::future::pending::<()>());
        let page = cx.new(|cx| {
            let mut page = ToolsPage::blank(cx);
            page.running = Some(("bun", Job::Install));
            page.queue.push_back((find("deno").unwrap(), Job::Install));
            page.stop = Some(stop);
            page
        });
        assert_eq!(
            page.read_with(cx, |page, _| page.work()).as_deref(),
            Some("Installing Bun, 1 more queued")
        );
        drop(page);
        // The app lets go of a dropped page at the end of its next update.
        cx.update(|_| {});
        cx.run_until_parked();
        assert_eq!(run.now_or_never(), Some(Err(Aborted)), "still running");
    }

    /// Final seams m4: quitting stops the installer the page runs, as closing it does. Only the
    /// page's drop stopped it, and quitting dropped it (with the windows) too late or not at all:
    /// the installer, in a session of its own, ran on with nobody to show its end.
    #[gpui_kit::test]
    fn quitting_stops_the_installer(cx: &mut TestAppContext) {
        let (run, stop) = abortable(futures::future::pending::<()>());
        // Held here, so only quitting can stop it.
        let page = cx.new(|cx| {
            let mut page = ToolsPage::blank(cx);
            page.running = Some(("bun", Job::Install));
            page.stop = Some(stop);
            page
        });
        cx.quit();
        assert_eq!(run.now_or_never(), Some(Err(Aborted)), "still running");
        drop(page);
    }

    /// The app's quit waits (as long as it waits for quit handlers) until the stopped run is
    /// over, its process group told to end, so the installer can't outlive the app.
    #[gpui_kit::test]
    fn quitting_waits_until_the_installer_was_told_to_stop(cx: &mut TestAppContext) {
        let (ended, ended_rx) = futures::channel::oneshot::channel::<()>();
        let page = cx.new(|cx| {
            let mut page = ToolsPage::blank(cx);
            page.running = Some(("bun", Job::Install));
            page.ended = Some(ended_rx);
            page
        });
        let mut quit = page
            .update(cx, |page, _| page.stop_for_quit())
            .boxed_local();
        assert_eq!((&mut quit).now_or_never(), None, "it didn't wait");
        drop(ended);
        assert_eq!(quit.now_or_never(), Some(()));
    }

    /// The list works from the keyboard: the search has it when the page opens, ↩ there moves
    /// it to the first row, ↑ and ↓ select, ↩ runs the row's main action (here: taking a tool
    /// out of the queue, which runs nothing), and ⌘F goes back to the search.
    #[gpui_kit::test]
    fn the_list_works_from_the_keyboard(cx: &mut TestAppContext) {
        use gpui_kit::Focusable as _;
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::catalog::init(cx);
        });
        let (window, page) = cx
            .update(|cx| {
                gpui_kit::open_window(Default::default(), cx, |_, cx| {
                    cx.new(|cx| {
                        let mut page = ToolsPage::blank(cx);
                        page.loading = false;
                        page.states.insert("nvm", State::Queued { for_: None });
                        page.queue.push_back((find("nvm").unwrap(), Job::Install));
                        page
                    })
                })
            })
            .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            page.update(cx, |page, cx| page.focus_search(window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        // nvm, and Node.js, whose description names it.
        cx.simulate_keystrokes(window, "n v m");
        cx.run_until_parked();
        cx.simulate_keystrokes(window, "enter");
        cx.run_until_parked();
        let state = |cx: &mut TestAppContext| {
            page.read_with(cx, |page, _| (page.cursor, page.states.get("nvm").cloned()))
        };
        let queued = Some(State::Queued { for_: None });
        assert_eq!(state(cx), (Some(0), queued.clone()), "↩ in the search");
        cx.simulate_keystrokes(window, "down");
        assert_eq!(state(cx), (Some(1), queued.clone()));
        cx.simulate_keystrokes(window, "down up");
        assert_eq!(state(cx), (Some(0), queued));
        cx.simulate_keystrokes(window, "enter");
        assert_eq!(
            state(cx),
            (Some(0), None),
            "↩ didn't take it out of the queue"
        );
        cx.simulate_keystrokes(window, "cmd-f");
        cx.run_until_parked();
        let search = page.read_with(cx, |page, cx| {
            page.search.as_ref().unwrap().focus_handle(cx)
        });
        cx.update_window(window, |_, window, _| {
            assert!(search.is_focused(window), "⌘F")
        })
        .unwrap();
    }
}
