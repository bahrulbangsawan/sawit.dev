//! Dev Tools page: developer tools that install with a cURL one-liner
//! (Claude Code, Bun, uv, …). Install one, a selection, or everything that
//! is missing; prerequisites (nvm before Node.js, uv before Python) are
//! queued first. Rows change state as soon as they are queued and show
//! "Installed" the moment the installer succeeds; the next detection
//! confirms it. Installed tools with a newer release offer an upgrade, any
//! installed tool can be uninstalled, and the installer, upgrade or
//! uninstaller that is running can be stopped.

mod detect;
mod list;
mod queue;
mod render;
mod row;

use std::collections::{HashMap, HashSet, VecDeque};

use futures::channel::oneshot;
use futures::future::AbortHandle;
use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::dev_tools::{Category, Found, Tool, find};
use sawit_core::package_manager::{Available, PackageManager};

use crate::catalog;

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
    /// The page of the list shown, and the rows a page holds.
    paging: catalog::Paging,
    /// The search as the list last filtered by it.
    last_query: String,
    /// The row ↑ and ↓ select and ↩ acts on, by its place in the list.
    cursor: Option<usize>,
    focus: FocusHandle,
    /// Made with the page when it has a window, else on the first render.
    search: Option<Entity<InputState>>,
    _search: Option<Subscription>,
    /// The package managers on this Mac, and the one the tools npm installs
    /// globally (Gemini CLI, the Cloudflare CLI) install with.
    pms: Vec<Available>,
    pm: Option<PackageManager>,
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
        "cf" => ("agent_logos/cloudflare-color.svg", true),
        // The app's own icon.
        "puremac" => ("tool_logos/puremac.png", true),
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
            paging: catalog::Paging::new("tools", cx),
            cursor: None,
            last_query: String::new(),
            focus: cx.focus_handle(),
            search: None,
            _search: None,
            pms: Vec::new(),
            pm: None,
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
mod tests;
