//! Skills page: agent skill packs from their official sources (Superpowers,
//! shadcn's improve, Jakub Krehel's interface skills, unslop, Animate, MCP
//! Apps, Cloudflare's), installed into the agents found on this Mac the way
//! each source documents: the `skills` CLI run with the package manager picked
//! (npx, pnpm dlx, bunx, yarn dlx), a Claude Code plugin, or a clone and copy.
//! Sawit's own Canvas Design installs Built In: a copy of the folder Sawit
//! ships, written out into its data folder; an agent's copy that differs from
//! this Sawit's version says so, and is kept in Sawit's data folder (`skill-backups/<agent>/<skill>`) when replaced.
//! A pack's sheet picks the agents and shows the exact commands before they
//! run; they run one pack at a time in the login shell, as the Dev Tools
//! page's installers do, with their output under the row. Removing deletes the
//! pack's skill folders from the agents picked, or uninstalls its plugin.

mod detect;
mod jobs;
mod list;
mod render;
mod row;
mod sheet;

use std::collections::{HashMap, HashSet, VecDeque};

use futures::channel::oneshot;
use futures::future::AbortHandle;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::package_manager::{Available, PackageManager};
use sawit_core::skills::{self, Action, Category, Method, Pack, Report};

use crate::catalog;

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

/// A pack's tile: its publisher's logo when there is one (Sawit's mark for
/// the skill it ships), else its category's icon.
fn pack_logo(pack: &Pack, cx: &App) -> AnyElement {
    let logo = match pack.by {
        "Cloudflare" => Some("agent_logos/cloudflare-color.svg"),
        "Sawit" => Some(crate::assets::LOGO_MARK),
        _ => None,
    };
    if let Some(logo) = logo {
        return img(logo).size(px(22.)).flex_none().into_any_element();
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
mod tests;
