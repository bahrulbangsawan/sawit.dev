//! New Project page: pick a TypeScript stack, see the `create-better-t-stack` command for it,
//! and create the project in Sawit's terminal, which then opens it as a project. The options,
//! rules, presets and command are Better-T-Stack's, ported in [`sawit_core::stack_builder`];
//! this page only draws them. The left column holds the name, the location, the command, what
//! the last pick changed, the stack as tags and the category nav; the right column one section
//! of cards per category. Below [`WIDE`] the columns stack and the page scrolls as one.

mod cards;
mod create;
mod left;
mod render;
mod stack;

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_core::instances::AppPaths;
use sawit_core::new_project;
use sawit_core::package_manager::{self, Available, Use};
use sawit_core::stack_builder::{self, Change, Pm, Stack};

use crate::terminal::{JobTag, TerminalPanel};

/// The page width from which the two columns sit side by side.
pub const WIDE: f32 = 760.;
/// The left column's width beside the cards.
const LEFT_W: f32 = 340.;
/// The gap between the columns.
const COLUMN_GAP: f32 = 24.;
/// The page's inset (`p_6`).
const INSET: f32 = 24.;
/// The narrowest a card gets before the cards take one column fewer.
const CARD_MIN: f32 = 220.;
/// The gap between cards (`gap_2`).
const CARD_GAP: f32 = 8.;
/// How often a running create is checked on.
const POLL: Duration = Duration::from_millis(250);
/// A create whose command was never seen running counts as over after this long idle.
const NEVER_RAN: Duration = Duration::from_secs(20);
/// A command that ran and made no project counts as the create failing once the shell has been
/// idle this long after it: a login shell runs programs of its own as it starts (nvm's
/// `printf`, a greeting), each then idle a while, before it reads the create typed ahead.
const SETTLE: Duration = Duration::from_secs(3);

/// What the page asks the workspace to do.
pub enum NewProjectEvent {
    /// Run the create in a terminal, then hand the terminal back ([`NewProjectPage::watch`]).
    Create(CreateJob),
    /// The project was created in this folder: open it as a project.
    Open(PathBuf),
}

impl EventEmitter<NewProjectEvent> for NewProjectPage {}

/// A create to run: in a terminal tab of its own (`tag`), in `location`, making `dir`.
#[derive(Clone)]
pub struct CreateJob {
    pub tag: JobTag,
    pub location: PathBuf,
    pub dir: PathBuf,
    pub command: String,
}

/// Where a create is.
#[derive(Debug, Clone, PartialEq)]
enum Creating {
    Idle,
    /// Handed to the terminal, its job `key` watched; `seen` once a command ran, `idle` since
    /// the shell was idle again after one that made no project.
    Running {
        dir: PathBuf,
        key: String,
        since: Instant,
        seen: bool,
        idle: Option<Instant>,
    },
    /// Over without a project: the terminal says why.
    Failed {
        dir: PathBuf,
    },
    /// Made, and being opened.
    Done(PathBuf),
}

/// Every New Project page, with its window: what quitting and closing a window ask about
/// ([`at_work`]).
#[derive(Default)]
struct Pages(Vec<(AnyWindowHandle, WeakEntity<NewProjectPage>)>);

impl Global for Pages {}

/// What each New Project page is creating, with its window ("Creating my-app"): closing the
/// window and quitting ask first, as for the Dev Tools page ([`crate::terminal::end_all`]).
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

pub struct NewProjectPage {
    stack: Stack,
    /// What the last pick, preset or reset changed, upstream's words.
    note: Vec<Change>,
    /// The folder the project goes in.
    location: PathBuf,
    /// The package managers on this Mac the CLI runs with, once detected.
    pms: Option<Vec<Available>>,
    /// Why Create Project didn't start.
    problem: Option<String>,
    creating: Creating,
    /// The terminal a create runs in, to watch its job.
    terminal: Option<WeakEntity<TerminalPanel>>,
    /// The card ↑ and ↓ select and ↩ picks, by its place among the cards shown.
    cursor: Option<usize>,
    /// Copy Command was just clicked.
    copied: bool,
    focus: FocusHandle,
    /// The cards' scroll (the whole page's when narrow).
    scroll: ScrollHandle,
    /// The page's width as last laid out.
    width: Rc<Cell<f32>>,
    name: Option<Entity<InputState>>,
    filter: Option<Entity<InputState>>,
    last_filter: String,
    /// Where the choices are kept between openings; none in tests.
    paths: Option<AppPaths>,
    _subscriptions: Vec<Subscription>,
}

impl NewProjectPage {
    /// The page as the user left it last time: the stack and package manager kept, in the
    /// location kept (else beside `open_project`, else `~/Developer`, else home), named with
    /// the first free name there.
    pub fn new(open_project: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let paths = AppPaths::default_macos();
        let kept = new_project::load(&paths).unwrap_or_default();
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let location = kept
            .location
            .filter(|l| l.is_dir())
            .unwrap_or_else(|| new_project::default_location(open_project.as_deref(), &home));
        let mut this = Self::blank(kept.stack, location, cx);
        this.paths = Some(paths);
        this.make_inputs(window, cx);
        this.detect(cx);
        Self::register(window, cx);
        this
    }

    /// A page with `stack`, creating in `location`, before anything is detected.
    fn blank(stack: Stack, location: PathBuf, cx: &mut Context<Self>) -> Self {
        let resolved = stack_builder::resolve(&stack);
        let mut stack = resolved.stack;
        stack.project_name = new_project::free_name(&location, stack_builder::DEFAULT_NAME);
        Self {
            stack,
            note: Vec::new(),
            location,
            pms: None,
            problem: None,
            creating: Creating::Idle,
            terminal: None,
            cursor: None,
            copied: false,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            width: Rc::new(Cell::new(0.)),
            name: None,
            filter: None,
            last_filter: String::new(),
            paths: None,
            _subscriptions: Vec::new(),
        }
    }

    /// A page creating `dir` in `window`, as one the user opened is, for tests elsewhere.
    #[cfg(test)]
    pub(crate) fn creating_in(dir: PathBuf, window: &Window, cx: &mut Context<Self>) -> Self {
        let location = dir.parent().map(PathBuf::from).unwrap_or_default();
        let mut page = Self::blank(Stack::default(), location, cx);
        page.creating = Creating::Running {
            key: format!("new-project:{}", dir.display()),
            dir,
            since: Instant::now(),
            seen: true,
            idle: None,
        };
        Self::register(window, cx);
        page
    }

    /// Lists the page made in `window` ([`at_work`]).
    fn register(window: &Window, cx: &mut Context<Self>) {
        let (me, here) = (cx.weak_entity(), window.window_handle());
        let pages = cx.default_global::<Pages>();
        pages.0.retain(|(_, page)| page.upgrade().is_some());
        pages.0.push((here, me));
    }

    /// The name and filter fields: typing a name changes the command; typing a filter narrows
    /// the cards, and ↩ there moves the keyboard to the first card.
    fn make_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name =
            cx.new(|cx| InputState::new(window, cx).placeholder(stack_builder::DEFAULT_NAME));
        let project_name = self.stack.project_name.clone();
        name.update(cx, |s, cx| s.set_value(project_name, window, cx));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter Options"));
        self._subscriptions = vec![
            cx.subscribe_in(&name, window, |this, state, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::Change) {
                    let value = state.read(cx).value().to_string();
                    if value != this.stack.project_name {
                        this.stack.project_name = value;
                        this.problem = None;
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(
                &filter,
                window,
                |this, _, e: &InputEvent, window, cx| match e {
                    InputEvent::Change if this.query(cx) != this.last_filter => {
                        this.last_filter = this.query(cx);
                        this.cursor = None;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => {
                        this.cursor = None;
                        this.step(true, window, cx);
                    }
                    _ => {}
                },
            ),
        ];
        self.name = Some(name);
        self.filter = Some(filter);
    }

    /// Finds the package managers the CLI can run with (npm, pnpm, bun).
    fn detect(&mut self, cx: &mut Context<Self>) {
        let rx = crate::rt::spawn(cx, package_manager::detect());
        cx.spawn(async move |this, cx| {
            if let Ok(found) = rx.await {
                let _ = this.update(cx, |this, cx| this.detected(found, cx));
            }
        })
        .detach();
    }

    /// What detection found: only the CLI's package managers, and the stack's moved to one of
    /// them when it isn't here.
    fn detected(&mut self, found: Vec<Available>, cx: &mut Context<Self>) {
        let found: Vec<Available> = found
            .into_iter()
            .filter(|a| Pm::from_package_manager(a.pm).is_some())
            .collect();
        let here = |pm: Pm| found.iter().any(|a| a.pm == pm.package_manager());
        if !found.is_empty() && !here(self.stack.package_manager) {
            let wanted = crate::package_managers::remembered();
            let pick = package_manager::choose(&found, None, wanted, Use::Run)
                .and_then(Pm::from_package_manager);
            if let Some(pm) = pick {
                self.stack.package_manager = pm;
            }
        }
        self.pms = Some(found);
        cx.notify();
    }

    /// Gives the filter the keyboard, as opening the page does.
    pub fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(filter) = &self.filter {
            filter.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    /// What the page is doing that closing it would leave unwatched: "Creating my-app".
    pub fn work(&self) -> Option<String> {
        match &self.creating {
            Creating::Running { dir, .. } => Some(format!(
                "Creating {}",
                dir.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )),
            _ => None,
        }
    }

    /// The filter's text, trimmed and lower case.
    fn query(&self, cx: &App) -> String {
        self.filter
            .as_ref()
            .map(|f| f.read(cx).value().trim().to_lowercase())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
