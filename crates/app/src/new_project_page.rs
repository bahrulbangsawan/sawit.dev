//! New Project page: pick a TypeScript stack, see the `create-better-t-stack` command for it,
//! and create the project in Sawit's terminal, which then opens it as a project. The options,
//! rules, presets and command are Better-T-Stack's, ported in [`sawit_core::stack_builder`];
//! this page only draws them. The left column holds the name, the location, the command, what
//! the last pick changed, the stack as tags and the category nav; the right column one section
//! of cards per category. Below [`WIDE`] the columns stack and the page scrolls as one.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::instances::AppPaths;
use sawit_core::new_project::{self, Remembered};
use sawit_core::package_manager::{self, Available, Use};
use sawit_core::stack_builder::{
    self, CLI_VERSION, Category, Change, PRESETS, Pm, Preset, Stack, StackOption,
};

use crate::catalog::{self, Mark, tilde};
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
    /// Handed to the terminal, its job `key` watched; `seen` once its command ran.
    Running {
        dir: PathBuf,
        key: String,
        since: Instant,
        seen: bool,
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

    /// The sections shown: each category with the options the filter leaves, in upstream's
    /// order; a category whose name matches keeps all its options.
    fn sections(&self, query: &str) -> Vec<(Category, Vec<&'static StackOption>)> {
        Category::ALL
            .iter()
            .filter_map(|&category| {
                let all = stack_builder::options(category);
                let options: Vec<&'static StackOption> =
                    if query.is_empty() || category.label().to_lowercase().contains(query) {
                        all.iter().collect()
                    } else {
                        all.iter()
                            .filter(|o| {
                                o.name.to_lowercase().contains(query)
                                    || o.id.contains(query)
                                    || o.description.to_lowercase().contains(query)
                            })
                            .collect()
                    };
                (!options.is_empty()).then_some((category, options))
            })
            .collect()
    }

    /// The cards shown, in order: what ↑, ↓ and ↩ move through.
    fn cards(&self, query: &str) -> Vec<(Category, &'static StackOption)> {
        self.sections(query)
            .into_iter()
            .flat_map(|(c, options)| options.into_iter().map(move |o| (c, o)))
            .collect()
    }

    /// Why option `id` of `category` can't be picked: upstream's rule, or, for a package
    /// manager, that this Mac doesn't have it.
    fn disabled(&self, category: Category, id: &str) -> Option<String> {
        if category == Category::PackageManager
            && let Some(pms) = &self.pms
            && let Some(pm) = Pm::parse(id)
            && !pms.iter().any(|a| a.pm == pm.package_manager())
        {
            return Some(format!("{id} isn't installed on this Mac"));
        }
        stack_builder::disabled_reason(&self.stack, category, id)
    }

    /// Picks option `id` of `category`, as a click on its card does.
    fn pick(&mut self, category: Category, id: &str, cx: &mut Context<Self>) {
        if self.disabled(category, id).is_some() || self.busy() {
            return;
        }
        let resolved = stack_builder::select(&self.stack, category, id);
        if category == Category::PackageManager {
            crate::package_managers::remember(resolved.stack.package_manager.package_manager());
        }
        self.settle(resolved.stack, resolved.changes, cx);
    }

    /// Takes option `id` of `category` out of the stack, as the × on its tag does.
    fn unpick(&mut self, category: Category, id: &str, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let resolved = stack_builder::remove(&self.stack, category, id);
        self.settle(resolved.stack, resolved.changes, cx);
    }

    /// Starts from preset `preset`, keeping the name typed and the package manager picked.
    fn apply_preset(&mut self, preset: &Preset, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let stack = Stack {
            project_name: self.stack.project_name.clone(),
            package_manager: self.stack.package_manager,
            ..preset.stack()
        };
        let resolved = stack_builder::resolve(&stack);
        self.settle(resolved.stack, resolved.changes, cx);
    }

    /// Back to upstream's default stack, keeping the name typed and the package manager picked.
    fn reset(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let stack = Stack {
            project_name: self.stack.project_name.clone(),
            package_manager: self.stack.package_manager,
            ..Stack::default()
        };
        self.settle(stack, Vec::new(), cx);
    }

    /// Takes `stack` as the page's, with what changed, and keeps it for next time.
    fn settle(&mut self, stack: Stack, note: Vec<Change>, cx: &mut Context<Self>) {
        self.stack = stack;
        self.note = note;
        self.problem = None;
        if matches!(self.creating, Creating::Failed { .. } | Creating::Done(_)) {
            self.creating = Creating::Idle;
        }
        self.keep(cx);
        cx.notify();
    }

    /// Writes the stack, location and package manager for the next opening (best effort).
    fn keep(&self, cx: &mut Context<Self>) {
        let Some(paths) = self.paths.clone() else {
            return;
        };
        let remembered = Remembered {
            stack: self.stack.clone(),
            location: Some(self.location.clone()),
        };
        cx.background_spawn(async move {
            if let Err(e) = new_project::save(&paths, &remembered) {
                log::warn!("sawit: can't keep the New Project choices: {e}");
            }
        })
        .detach();
    }

    /// Whether a create runs.
    fn busy(&self) -> bool {
        matches!(self.creating, Creating::Running { .. })
    }

    /// Asks for the folder to create the project in.
    fn choose_location(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            if let Some(dir) = paths.into_iter().next() {
                let _ = this.update(cx, |this, cx| this.set_location(dir, cx));
            }
        })
        .detach();
    }

    /// Creates in `dir` from now on.
    pub fn set_location(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.location = dir;
        self.problem = None;
        self.keep(cx);
        cx.notify();
    }

    /// Takes the stack an agent asked for (named as it is), in `location`.
    pub fn set_stack(
        &mut self,
        stack: Stack,
        location: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let resolved = stack_builder::resolve(&stack);
        if let Some(name) = &self.name {
            let value = resolved.stack.project_name.clone();
            name.update(cx, |s, cx| s.set_value(value, window, cx));
        }
        self.location = location;
        self.settle(resolved.stack, resolved.changes, cx);
    }

    /// Copies the command, and says so for a moment.
    fn copy_command(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(stack_builder::command(
            &self.stack,
        )));
        self.copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Checks the create can start and marks it running: the job for the workspace to run,
    /// or why it can't start (also shown on the page).
    pub fn begin_create(&mut self, cx: &mut Context<Self>) -> Result<CreateJob, String> {
        if self.busy() {
            return Err("A project is being created already.".into());
        }
        let dir = match new_project::target(&self.location, &self.stack) {
            Ok(dir) => dir,
            Err(why) => {
                self.problem = Some(why.clone());
                cx.notify();
                return Err(why);
            }
        };
        let key = format!("new-project:{}", dir.display());
        let name = stack_builder::format_project_name(&self.stack.project_name);
        let job = CreateJob {
            tag: JobTag {
                key: key.clone(),
                name: format!("New Project: {name}"),
            },
            location: self.location.clone(),
            dir: dir.clone(),
            command: stack_builder::run_command(&self.stack),
        };
        self.problem = None;
        self.creating = Creating::Running {
            dir,
            key,
            since: Instant::now(),
            seen: false,
        };
        self.keep(cx);
        cx.notify();
        Ok(job)
    }

    /// Create Project: asks the workspace to run the create.
    fn create(&mut self, cx: &mut Context<Self>) {
        if let Ok(job) = self.begin_create(cx) {
            cx.emit(NewProjectEvent::Create(job));
        }
    }

    /// The create runs in `terminal`: watches its job until it ends.
    pub fn watch(&mut self, terminal: WeakEntity<TerminalPanel>, cx: &mut Context<Self>) {
        self.terminal = Some(terminal);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let Ok(over) = this.update(cx, |this, cx| this.poll(cx)) else {
                    return;
                };
                if over {
                    return;
                }
            }
        })
        .detach();
    }

    /// The terminal couldn't start the create: says why.
    pub fn not_started(&mut self, why: String, cx: &mut Context<Self>) {
        self.creating = Creating::Idle;
        self.problem = Some(why);
        cx.notify();
    }

    /// One look at the create's job; true once it is over and the page has said how it went.
    fn poll(&mut self, cx: &mut Context<Self>) -> bool {
        let Creating::Running {
            key, since, seen, ..
        } = &mut self.creating
        else {
            return true;
        };
        let running = self
            .terminal
            .as_ref()
            .and_then(|t| t.upgrade())
            .and_then(|t| t.read(cx).job_status(key, cx))
            .map(|s| s.running.is_some());
        if !job_over(running, seen, since.elapsed()) {
            return false;
        }
        self.finish(cx);
        true
    }

    /// The create's job ended: opens the project when it was made, else says it failed.
    fn finish(&mut self, cx: &mut Context<Self>) {
        let Creating::Running { dir, .. } = &self.creating else {
            return;
        };
        let dir = dir.clone();
        if new_project::created(&dir) {
            self.creating = Creating::Done(dir.clone());
            cx.emit(NewProjectEvent::Open(dir));
        } else {
            self.creating = Creating::Failed { dir };
        }
        cx.notify();
    }

    /// Selects the next (or previous) card, scrolling its section into view, and gives the
    /// page the keyboard.
    fn step(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query(cx);
        let cards = self.cards(&query);
        self.cursor = catalog::step(self.cursor, cards.len(), down);
        if let Some(&(category, _)) = self.cursor.and_then(|ix| cards.get(ix)) {
            self.scroll_to(category, &query);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// ↩ on a card: picks it.
    fn confirm(&mut self, cx: &mut Context<Self>) {
        let cards = self.cards(&self.query(cx));
        if let Some(&(category, option)) = self.cursor.and_then(|ix| cards.get(ix)) {
            self.pick(category, option.id, cx);
        }
    }

    /// Scrolls `category`'s section to the top of the cards.
    fn scroll_to(&self, category: Category, query: &str) {
        let Some(ix) = self
            .sections(query)
            .iter()
            .position(|(c, _)| *c == category)
        else {
            return;
        };
        // Narrow, the page's own top block comes before the sections.
        let ix = if self.wide() { ix } else { ix + 1 };
        self.scroll.scroll_to_top_of_item(ix);
    }

    fn wide(&self) -> bool {
        self.width.get() >= WIDE
    }

    /// How many cards share a line: as many as fit at [`CARD_MIN`] wide.
    fn columns(&self) -> usize {
        let page = self.width.get().max(CARD_MIN + 2. * INSET);
        let cards = if self.wide() {
            page - 2. * INSET - LEFT_W - COLUMN_GAP
        } else {
            page - 2. * INSET
        };
        ((cards + CARD_GAP) / (CARD_MIN + CARD_GAP)).floor().max(1.) as usize
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let page = cx.entity().downgrade();
        let blocked = stack_builder::validate_project_name(&self.stack.project_name).is_some();
        h_flex()
            .gap_4()
            .flex_wrap()
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(260.))
                    .gap_1()
                    .child(div().text_xl().font_semibold().child("New Project"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(format!(
                                "Pick a TypeScript stack from Better-T-Stack's builder. Sawit runs \
                                 create-better-t-stack {CLI_VERSION} in its terminal and opens the project."
                            )),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(
                        Button::new("new-project-presets")
                            .outline()
                            .small()
                            .icon(Lucide::LayoutTemplate)
                            .label("Presets")
                            .dropdown_caret(true)
                            .disabled(self.busy())
                            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                                for preset in PRESETS {
                                    let page = page.clone();
                                    menu = menu.item(PopupMenuItem::new(preset.name).on_click(
                                        move |_, _, cx| {
                                            let _ = page.update(cx, |this, cx| {
                                                this.apply_preset(preset, cx)
                                            });
                                        },
                                    ));
                                }
                                menu
                            }),
                    )
                    .child(
                        Button::new("new-project-reset")
                            .ghost()
                            .small()
                            .icon(Lucide::RotateCcw)
                            .label("Reset")
                            .tooltip("Back to the Default Stack")
                            .disabled(self.busy())
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    )
                    .child(
                        Button::new("new-project-create")
                            .debug_selector(|| "new-project-create".into())
                            .primary()
                            .small()
                            .icon(Lucide::FolderPlus)
                            .label("Create Project")
                            .loading(self.busy())
                            .disabled(self.busy() || blocked)
                            .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                    ),
            )
    }

    /// A small heading over a part of the left column.
    fn heading(text: &'static str, cx: &App) -> Div {
        div()
            .text_xs()
            .font_medium()
            .text_color(cx.theme().muted_foreground)
            .child(text)
    }

    fn render_left(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let name_error = stack_builder::validate_project_name(&self.stack.project_name);
        let command = stack_builder::command(&self.stack);
        let pms = self.pms.clone().unwrap_or_default();
        let pm_menu = crate::package_managers::menu(
            "new-project-pm",
            &pms,
            Use::Run,
            Some(self.stack.package_manager.package_manager()),
            cx.listener(|this, pm: &package_manager::PackageManager, _, cx| {
                if let Some(pm) = Pm::from_package_manager(*pm) {
                    this.pick(Category::PackageManager, pm.id(), cx);
                }
            }),
        );
        let quiet = stack_builder::quiet_flags(&self.stack).join(" ");
        v_flex()
            .gap_4()
            .child(
                v_flex()
                    .gap_1p5()
                    .child(Self::heading("Project Name", cx))
                    .when_some(self.name.as_ref(), |el, name| {
                        el.child(Input::new(name).small())
                    })
                    .when_some(name_error, |el, error| {
                        el.child(
                            catalog::label("new-project-name-error", error)
                                .text_xs()
                                .text_color(theme.danger),
                        )
                    }),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(Self::heading("Location", cx))
                    .child(
                        Button::new("new-project-location")
                            .outline()
                            .small()
                            .w_full()
                            .icon(Lucide::FolderOpen)
                            .label(tilde(&self.location))
                            .tooltip("Choose the Folder to Create the Project In")
                            .disabled(self.busy())
                            .on_click(cx.listener(|this, _, _, cx| this.choose_location(cx))),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Self::heading("Command", cx))
                            .child(div().flex_1())
                            .children(pm_menu)
                            .child(
                                Button::new("new-project-copy")
                                    .outline()
                                    .xsmall()
                                    .icon(if self.copied {
                                        Lucide::Check
                                    } else {
                                        Lucide::Copy
                                    })
                                    .label(if self.copied { "Copied" } else { "Copy Command" })
                                    .on_click(cx.listener(|this, _, _, cx| this.copy_command(cx))),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "new-project-command".into())
                            .p_3()
                            .rounded(theme.radius)
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.secondary)
                            .font_family(theme.mono_font_family.clone())
                            .text_xs()
                            .whitespace_normal()
                            .child(catalog::label("new-project-command-text", command)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .whitespace_normal()
                            .child(format!(
                                "Create Project also passes {quiet}, so the CLI doesn't stop to ask."
                            )),
                    ),
            )
            .children(self.render_note(cx))
            .child(self.render_tags(cx))
            .children(self.render_status(cx))
            .child(self.render_nav(cx))
    }

    /// What the last pick changed, and upstream's desktop build note.
    fn render_note(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notes = stack_builder::resolve(&self.stack).notes;
        if self.note.is_empty() && notes.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let lines = self
            .note
            .iter()
            .map(|c| c.message.clone())
            .chain(notes)
            .enumerate()
            .map(|(ix, text)| {
                h_flex()
                    .gap_1p5()
                    .items_start()
                    .child(
                        div().pt(px(2.)).child(
                            Icon::new(Lucide::Info)
                                .xsmall()
                                .text_color(theme.muted_foreground),
                        ),
                    )
                    .child(
                        catalog::label(("new-project-note", ix), text)
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal(),
                    )
            });
        Some(
            v_flex()
                .debug_selector(|| "new-project-note".into())
                .gap_1()
                .px_3()
                .py_2()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.info.opacity(0.35))
                .bg(theme.info.opacity(0.08))
                .text_xs()
                .children(lines)
                .into_any_element(),
        )
    }

    /// The stack as tags, each with a × that takes it out.
    fn render_tags(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let tags = stack_builder::selected(&self.stack)
            .into_iter()
            .enumerate()
            .map(|(ix, (category, option))| {
                let id = option.id;
                h_flex()
                    .id(("new-project-tag", ix))
                    .role(Role::ListItem)
                    .aria_label(format!("{}: {}", category.label(), option.name))
                    .h(px(24.))
                    .pl_1p5()
                    .pr_0p5()
                    .gap_1()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.secondary)
                    .text_xs()
                    .child(logo(category, id, 12., cx))
                    .child(option.name)
                    .child(
                        Button::new(("new-project-untag", ix))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::X).xsmall())
                            .accessibility_label(format!("Remove {}", option.name))
                            .disabled(self.busy())
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.unpick(category, id, cx)),
                            ),
                    )
            });
        v_flex()
            .gap_1p5()
            .child(Self::heading("Your Stack", cx))
            .child(h_flex().flex_wrap().gap_1p5().children(tags))
    }

    /// Creating, failed, made, or why Create Project didn't start.
    fn render_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let name = |dir: &PathBuf| tilde(dir);
        let box_ = |color: Hsla| {
            h_flex()
                .gap_2()
                .px_3()
                .py_2()
                .items_start()
                .rounded(theme.radius)
                .border_1()
                .border_color(color.opacity(0.35))
                .bg(color.opacity(0.08))
                .text_xs()
        };
        Some(match (&self.creating, &self.problem) {
            (Creating::Running { dir, .. }, _) => catalog::pill(
                Mark::Spinner,
                format!("Creating {}…", name(dir)),
                theme.primary,
            )
            .into_any_element(),
            (Creating::Done(dir), _) => catalog::pill(
                Mark::Icon(Lucide::CircleCheck),
                format!("Created {}, opening it…", name(dir)),
                theme.success,
            )
            .into_any_element(),
            (Creating::Failed { .. }, _) => box_(theme.danger)
                .child(
                    Icon::new(Lucide::TriangleAlert)
                        .small()
                        .text_color(theme.danger),
                )
                .child(
                    catalog::label("new-project-failed", "Creation failed, see the terminal.")
                        .flex_1(),
                )
                .child(
                    Button::new("new-project-retry")
                        .outline()
                        .xsmall()
                        .icon(Lucide::RotateCcw)
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                )
                .into_any_element(),
            (Creating::Idle, Some(problem)) => box_(theme.warning)
                .child(
                    Icon::new(Lucide::TriangleAlert)
                        .small()
                        .text_color(theme.warning),
                )
                .child(
                    catalog::label("new-project-problem", problem.clone())
                        .flex_1()
                        .min_w_0()
                        .whitespace_normal(),
                )
                .into_any_element(),
            (Creating::Idle, None) => return None,
        })
    }

    /// The categories as chips that scroll to their section; a dot when one is picked.
    fn render_nav(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let query = self.query(cx);
        let chips = self.sections(&query).into_iter().map(|(category, _)| {
            let chosen = stack_builder::selected(&self.stack)
                .iter()
                .any(|(c, _)| *c == category);
            Button::new(SharedString::from(format!(
                "new-project-nav-{}",
                category.id()
            )))
            .ghost()
            .xsmall()
            .child(
                h_flex()
                    .gap_1()
                    .child(div().size(px(5.)).rounded_full().bg(if chosen {
                        theme.primary
                    } else {
                        theme.muted_foreground.opacity(0.4)
                    }))
                    .child(category.label()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let query = this.query(cx);
                this.scroll_to(category, &query);
                cx.notify();
            }))
        });
        v_flex()
            .gap_1p5()
            .child(Self::heading("Jump To", cx))
            .child(h_flex().flex_wrap().gap_1().children(chips))
    }

    fn render_filter(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let filter = self.filter.as_ref()?;
        Some(
            h_flex()
                .debug_selector(|| "new-project-filter".into())
                .child(catalog::search(filter, cx))
                .into_any_element(),
        )
    }

    /// One section per category shown, its cards wrapping.
    fn render_sections(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.query(cx);
        // Each card takes its share of the line, with the gap on its right and below; the line
        // reaches one gap past the column, so the last card's gap falls outside it.
        let share = 1. / self.columns() as f32;
        let cursor = self
            .cursor
            .and_then(|ix| self.cards(&query).get(ix).copied())
            .map(|(c, o)| (c, o.id));
        let muted = cx.theme().muted_foreground;
        self.sections(&query)
            .into_iter()
            .map(|(category, options)| {
                let cards: Vec<AnyElement> = options
                    .into_iter()
                    .map(|option| {
                        let at = cursor == Some((category, option.id));
                        div()
                            .w(relative(share))
                            .pr(px(CARD_GAP))
                            .pb(px(CARD_GAP))
                            .child(self.render_card(category, option, at, cx))
                            .into_any_element()
                    })
                    .collect();
                v_flex()
                    .id(SharedString::from(format!(
                        "new-project-section-{}",
                        category.id()
                    )))
                    .debug_selector(move || format!("new-project-section-{}", category.id()))
                    .pb_5()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .font_semibold()
                            .text_color(muted)
                            .child(category.label().to_uppercase())
                            .when(category.multiple(), |el| {
                                el.child(div().font_normal().child("· any number"))
                            }),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_stretch()
                            .mr(px(-CARD_GAP))
                            .mb(px(-CARD_GAP))
                            .children(cards),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn render_card(
        &self,
        category: Category,
        option: &'static StackOption,
        at: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let picked = self.stack.is_picked(category, option.id);
        let reason = self.disabled(category, option.id);
        let id = option.id;
        let mut label = option.name.to_string();
        if picked {
            label.push_str(", selected");
        }
        if let Some(reason) = &reason {
            label.push_str(&format!(". {reason}"));
        }
        let border = if picked {
            theme.primary
        } else if at {
            theme.ring
        } else {
            theme.border
        };
        let tag = |text: &'static str, color: Hsla| {
            div()
                .flex_none()
                .px_1()
                .rounded(px(3.))
                .border_1()
                .border_color(color.opacity(0.35))
                .bg(color.opacity(0.1))
                .text_color(color)
                .text_size(px(10.))
                .child(text)
        };
        h_flex()
            .id(SharedString::from(format!(
                "new-project-card-{}-{id}",
                category.id()
            )))
            .debug_selector(move || format!("new-project-card-{}-{id}", category.id()))
            .role(if category.multiple() {
                Role::CheckBox
            } else {
                Role::RadioButton
            })
            .aria_label(label)
            .aria_toggled(picked.into())
            .size_full()
            .min_h(px(64.))
            .p_3()
            .gap_3()
            .items_start()
            .rounded(theme.radius_lg)
            .border_1()
            .when(at && !picked, |el| el.border_2())
            .border_color(border)
            .when(picked, |el| el.bg(theme.primary.opacity(0.06)))
            .when(reason.is_some(), |el| el.border_dashed().opacity(0.55))
            .when(reason.is_none(), |el| {
                el.cursor_pointer()
                    .hover(|el| el.border_color(theme.primary.opacity(0.6)))
            })
            .child(
                div()
                    .size(px(28.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme.radius)
                    .bg(theme.secondary)
                    .child(logo(category, id, 16., cx)),
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
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .font_medium()
                                    .when(picked, |el| el.text_color(theme.primary))
                                    .child(option.name),
                            )
                            .when(option.experimental, |el| {
                                el.child(tag("Experimental", theme.warning))
                            })
                            .when(option.default && !picked, |el| {
                                el.child(tag("Default", theme.muted_foreground))
                            })
                            .when(picked, |el| {
                                el.child(
                                    Icon::new(Lucide::CircleCheck)
                                        .xsmall()
                                        .text_color(theme.primary),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .whitespace_normal()
                            .child(option.description),
                    )
                    .when_some(reason.clone(), |el, reason| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(theme.danger)
                                .whitespace_normal()
                                .line_clamp(2)
                                .child(reason),
                        )
                    }),
            )
            .when_some(reason, |el, reason| {
                el.tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.pick(category, id, cx);
            }))
            .into_any_element()
    }
}

/// Whether the create's job is over: `running` is its terminal's state (gone: `None`; its
/// command running: `Some(true)`; the shell idle: `Some(false)`). Seen running once, it is over
/// when the shell is idle again; never seen running (it failed at once, between two looks), once
/// [`NEVER_RAN`] has passed.
fn job_over(running: Option<bool>, seen: &mut bool, waited: Duration) -> bool {
    match running {
        None => true,
        Some(true) => {
            *seen = true;
            false
        }
        Some(false) => *seen || waited >= NEVER_RAN,
    }
}

/// An option's logo: its Simple Icons mark (assets/stack_logos), else a Lucide glyph, else its
/// initial, `size` px, tinted with the theme.
pub fn logo(category: Category, id: &str, size: f32, cx: &App) -> AnyElement {
    let color = cx.theme().foreground;
    let mark = |name: &str| {
        svg()
            .path(SharedString::from(format!("stack_logos/{name}.svg")))
            .size(px(size))
            .flex_none()
            .text_color(color)
            .into_any_element()
    };
    let glyph = |icon: Lucide| {
        Icon::new(icon)
            .size(px(size))
            .text_color(cx.theme().muted_foreground)
            .into_any_element()
    };
    let initial = |text: &'static str| {
        div()
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size * 0.8))
            .font_semibold()
            .text_color(color)
            .child(text)
            .into_any_element()
    };
    if matches!(id, "none" | "false") {
        return glyph(Lucide::Ban);
    }
    match (category, id) {
        (_, "tanstack-router" | "tanstack-start" | "self-tanstack-start") => mark("tanstack"),
        (_, "react-router") => mark("reactrouter"),
        (_, "next" | "self-next") => mark("nextdotjs"),
        (_, "nuxt" | "self-nuxt") => mark("nuxt"),
        (_, "svelte" | "self-svelte") => mark("svelte"),
        (_, "solid" | "self-solid") => mark("solid"),
        (_, "astro" | "self-astro") => mark("astro"),
        (Category::NativeFrontend, _) => mark("expo"),
        (_, "node") => mark("nodedotjs"),
        (_, "workers" | "d1") => mark("cloudflareworkers"),
        (_, "postgres") => mark("postgresql"),
        (_, "prisma-postgres") => mark("prisma"),
        (_, "mongodb-atlas") => mark("mongodb"),
        (_, "better-auth") => mark("betterauth"),
        (_, "oxlint") => mark("oxc"),
        (_, "mcp") => mark("modelcontextprotocol"),
        (Category::Git, "true") => mark("git"),
        (Category::Install, "true") => glyph(Lucide::Download),
        (_, "orpc") => initial("o"),
        (_, "elysia") => initial("E"),
        (_, "polar") => initial("P"),
        (_, "husky") => initial("H"),
        (_, "ultracite") => initial("U"),
        (_, "axiom") => initial("A"),
        (_, "starlight" | "fumadocs") => glyph(Lucide::BookOpen),
        (_, "electrobun") => glyph(Lucide::Monitor),
        (_, "opentui") => glyph(Lucide::Terminal),
        (_, "skills") => glyph(Lucide::Sparkles),
        (_, "evlog") => glyph(Lucide::ScrollText),
        (_, "vite-plus") => glyph(Lucide::Zap),
        (_, "todo") => glyph(Lucide::ListChecks),
        (_, "ai") => glyph(Lucide::Bot),
        (_, name) => mark(name),
    }
}

impl Render for NewProjectPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.name.is_none() {
            self.make_inputs(window, cx);
        }
        let measure = {
            let (width, page) = (self.width.clone(), cx.entity_id());
            canvas(
                move |bounds, _, cx| {
                    let now = f32::from(bounds.size.width);
                    if (now - width.get()).abs() >= 1. {
                        width.set(now);
                        // After this frame: a notify while it is drawn may draw nothing more.
                        cx.defer(move |cx| cx.notify(page));
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        };
        let sections = self.render_sections(cx);
        let body = if self.wide() {
            v_flex()
                .size_full()
                .p_6()
                .gap_4()
                .child(self.render_header(cx))
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .gap_6()
                        .items_start()
                        .child(
                            div()
                                .id("new-project-left")
                                .debug_selector(|| "new-project-left".into())
                                .w(px(LEFT_W))
                                .flex_none()
                                .h_full()
                                .overflow_y_scroll()
                                .child(self.render_left(cx).pb_4()),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .gap_3()
                                .children(self.render_filter(cx))
                                .child(
                                    div()
                                        .id("new-project-cards")
                                        .debug_selector(|| "new-project-cards".into())
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.scroll)
                                        .children(sections),
                                ),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .id("new-project-page")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(
                    v_flex()
                        .p_6()
                        .pb_4()
                        .gap_4()
                        .child(self.render_header(cx))
                        .child(
                            div()
                                .debug_selector(|| "new-project-left".into())
                                .child(self.render_left(cx)),
                        )
                        .children(self.render_filter(cx)),
                )
                .children(sections.into_iter().map(|s| div().px_6().child(s)))
                .into_any_element()
        };
        div()
            .relative()
            .size_full()
            .key_context("NewProjectPage")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &catalog::FocusSearch, window, cx| {
                this.focus_filter(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &catalog::SelectNext, window, cx| {
                    this.step(true, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &catalog::SelectPrevious, window, cx| {
                    this.step(false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &catalog::Confirm, _, cx| this.confirm(cx)))
            .bg(cx.theme().background)
            .child(measure)
            .child(body)
    }
}

#[cfg(test)]
mod tests;
