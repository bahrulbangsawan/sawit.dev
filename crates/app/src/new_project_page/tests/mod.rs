//! The New Project page's tests: its layout at the display's width and at the narrowest window,
//! picking from cards and the keyboard, and the create, refused, run in a real terminal and
//! watched to its end.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::{
    AppContext as _, Bounds, Entity, Pixels, TestAppContext, VisualTestContext, WindowBounds,
    WindowOptions, point, px, size,
};
use sawit_core::package_manager::{Available, PackageManager};
use sawit_core::stack_builder::{Backend, Category, Orm, Stack, WebFrontend};

use super::create::job_over;
use super::{Creating, NewProjectEvent, NewProjectPage, SETTLE};
use crate::terminal::{Origin, TerminalPanel};

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::catalog::init(cx);
    });
}

/// The page, creating in `location`, in a `width` × 900 window, bun and npm on this Mac.
fn open_at(
    cx: &mut TestAppContext,
    width: f32,
    location: PathBuf,
) -> (Entity<NewProjectPage>, &mut VisualTestContext) {
    init(cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(width), px(900.)),
        })),
        ..Default::default()
    };
    let (window, page) = cx
        .update(|cx| {
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| {
                    let mut page = NewProjectPage::blank(Stack::default(), location, cx);
                    page.make_inputs(window, cx);
                    page.pms = Some(
                        [PackageManager::Npm, PackageManager::Bun]
                            .into_iter()
                            .map(|pm| Available {
                                pm,
                                runner: PathBuf::from(format!("/bin/{}", pm.runner())),
                                version: None,
                            })
                            .collect(),
                    );
                    page
                })
            })
        })
        .expect("the test window opens");
    let vcx = VisualTestContext::from_window(window, cx).into_mut();
    settle(vcx);
    (page, vcx)
}

/// Draws until the page has measured itself and laid out for its width.
fn settle(vcx: &mut VisualTestContext) {
    for _ in 0..3 {
        vcx.run_until_parked();
        vcx.update(|window, _| window.refresh());
    }
    vcx.run_until_parked();
}

fn at(edge: Pixels) -> f32 {
    f32::from(edge).round()
}

fn bounds(vcx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    vcx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} isn't drawn"))
}

/// At the display's width the page fills the window inside the 24 px inset: the left column
/// beside the cards, which reach the right inset. No column caps it.
#[gpui_kit::test]
fn wide_the_columns_sit_side_by_side_and_fill_the_window(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-wide");
    let (_page, vcx) = open_at(cx, 1470., dir);
    let left = bounds(vcx, "new-project-left");
    let cards = bounds(vcx, "new-project-cards");
    assert_eq!(at(left.origin.x), 24.);
    assert_eq!(at(left.size.width), 340.);
    assert_eq!(at(cards.origin.x), 24. + 340. + 24.);
    assert_eq!(at(cards.right()), 1470. - 24.);
    assert_eq!(
        at(cards.top()),
        at(bounds(vcx, "new-project-filter").bottom()) + 12.
    );
    // The cards share a line: a section is as wide as the column.
    let web = bounds(vcx, "new-project-section-webFrontend");
    assert_eq!(at(web.right()), 1470. - 24.);
    let first = bounds(vcx, "new-project-card-webFrontend-tanstack-router");
    let second = bounds(vcx, "new-project-card-webFrontend-react-router");
    assert_eq!(at(first.top()), at(second.top()), "two cards on one line");
    assert!(first.right() < second.left());
    // A card whose description takes a line more makes its whole line that tall.
    let third = bounds(vcx, "new-project-card-webFrontend-tanstack-start");
    assert_eq!(at(third.top()), at(first.top()));
    assert_eq!(at(first.size.height), at(third.size.height));
    // Four share the line, the last one reaching the right inset.
    let fourth = bounds(vcx, "new-project-card-webFrontend-next");
    assert_eq!(at(fourth.top()), at(first.top()));
    assert_eq!(at(fourth.right()), 1470. - 24.);
    let command = bounds(vcx, "new-project-command");
    assert!(left.contains(&command.center()), "{left:?} {command:?}");
}

/// The narrowest page (the 720 × 480 window less the 240 px sidebar is 480 wide, and 720 with
/// the sidebar closed): the left column stacks over the cards, everything inside the inset.
#[gpui_kit::test]
fn narrow_the_columns_stack_and_stay_inside_the_window(cx: &mut TestAppContext) {
    for width in [720., 480.] {
        let dir = crate::testing::scratch_dir("new-project-narrow");
        let (_page, vcx) = open_at(cx, width, dir);
        let left = bounds(vcx, "new-project-left");
        assert_eq!(at(left.origin.x), 24., "{width}");
        assert_eq!(at(left.size.width), width - 48., "{width}");
        let web = bounds(vcx, "new-project-section-webFrontend");
        assert!(web.top() >= left.bottom(), "{width}: {left:?} {web:?}");
        assert_eq!(at(web.origin.x), 24., "{width}");
        assert!(at(web.right()) <= width - 24., "{width}: {web:?}");
        let card = bounds(vcx, "new-project-card-webFrontend-tanstack-router");
        assert!(at(card.right()) <= width - 24., "{width}: {card:?}");
        let create = bounds(vcx, "new-project-create");
        assert!(at(create.right()) <= width - 24., "{width}: {create:?}");
        assert!(vcx.debug_bounds("new-project-cards").is_none());
    }
}

/// A click on a card picks it and resolves the stack, with what changed noted; a disabled
/// card stays put.
#[gpui_kit::test]
fn a_click_picks_a_card_and_a_disabled_one_does_nothing(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-click");
    let (page, vcx) = open_at(cx, 1470., dir);
    let convex = bounds(vcx, "new-project-card-backend-convex");
    vcx.simulate_click(convex.center(), Default::default());
    settle(vcx);
    page.read_with(vcx, |page, _| {
        assert_eq!(page.stack.backend, Backend::Convex);
        assert_eq!(page.stack.orm, Orm::None);
        assert!(
            page.note
                .iter()
                .any(|c| c.message == "Orm set to 'none' (managed by convex)"),
            "{:?}",
            page.note
        );
        assert_eq!(
            page.disabled(Category::Orm, "prisma").as_deref(),
            Some("Convex provides its own orm")
        );
    });
    assert!(vcx.debug_bounds("new-project-note").is_some());
    let prisma = bounds(vcx, "new-project-card-orm-prisma");
    vcx.simulate_click(prisma.center(), Default::default());
    settle(vcx);
    page.read_with(vcx, |page, _| assert_eq!(page.stack.orm, Orm::None));
}

/// ⌘F gives the filter the keyboard; typing narrows the cards; ↩ there selects the first card,
/// ↓ the next, and ↩ picks it.
#[gpui_kit::test]
fn the_keyboard_filters_moves_and_picks(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-keys");
    let (page, vcx) = open_at(cx, 1470., dir);
    vcx.update(|window, cx| page.update(cx, |p, cx| window.focus(&p.focus, cx)));
    vcx.simulate_keystrokes("cmd-f");
    vcx.simulate_input("svelte");
    settle(vcx);
    let shown: Vec<(Category, &str)> = page.read_with(vcx, |page, cx| {
        page.cards(&page.query(cx))
            .into_iter()
            .map(|(c, o)| (c, o.id))
            .collect()
    });
    assert_eq!(
        shown,
        [
            (Category::WebFrontend, "svelte"),
            (Category::Backend, "self-svelte")
        ]
    );
    assert!(vcx.debug_bounds("new-project-section-runtime").is_none());
    vcx.simulate_keystrokes("enter");
    settle(vcx);
    page.read_with(vcx, |page, _| assert_eq!(page.cursor, Some(0)));
    vcx.simulate_keystrokes("enter");
    settle(vcx);
    page.read_with(vcx, |page, _| {
        assert_eq!(page.stack.web_frontend, WebFrontend::Svelte);
        assert_eq!(page.stack.api.id(), "orpc", "svelte has no tRPC");
    });
    vcx.simulate_keystrokes("down enter");
    settle(vcx);
    page.read_with(vcx, |page, _| {
        assert_eq!(page.cursor, Some(1));
        assert_eq!(page.stack.backend, Backend::SelfSvelte);
    });
}

/// A tag's × takes its option out, as upstream's badge does.
#[gpui_kit::test]
fn a_tags_cross_takes_it_out(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-tags");
    let (page, vcx) = open_at(cx, 1470., dir);
    vcx.update(|_, cx| {
        page.update(cx, |p, cx| p.unpick(Category::Addons, "turborepo", cx));
    });
    page.read_with(vcx, |page, _| assert!(page.stack.addons.is_empty()));
}

/// Create Project never starts over a folder that is there: it says so on the page.
#[gpui_kit::test]
fn create_refuses_a_folder_that_exists(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-exists");
    std::fs::create_dir_all(dir.join("taken")).unwrap();
    let (page, vcx) = open_at(cx, 1470., dir);
    let refused = vcx.update(|_, cx| {
        page.update(cx, |p, cx| {
            p.stack.project_name = "taken".into();
            p.begin_create(cx)
        })
    });
    let Err(why) = refused else {
        panic!("it started over a folder that exists")
    };
    assert!(why.contains("already exists"), "{why}");
    settle(vcx);
    page.read_with(vcx, |page, _| {
        assert_eq!(page.creating, Creating::Idle);
        assert!(page.problem.is_some());
        assert_eq!(page.work(), None);
    });
}

/// The poll's state for one create's job, looked at as [`NewProjectPage::poll`] does.
struct Looks {
    since: Instant,
    seen: bool,
    idle: Option<Instant>,
}

impl Looks {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            seen: false,
            idle: None,
        }
    }

    /// A look `ms` after the create started: whether its job is over.
    fn at(&mut self, ms: u64, running: Option<bool>, made: bool) -> bool {
        let now = self.since + Duration::from_millis(ms);
        job_over(
            running,
            made,
            &mut self.seen,
            &mut self.idle,
            self.since,
            now,
        )
    }
}

/// The poll's reading of a job: over once seen running and idle again with the project made,
/// when its terminal is gone, or after a long while never seen running.
#[test]
fn a_job_is_over_once_it_ran_and_the_shell_is_idle() {
    let mut job = Looks::new();
    assert!(!job.at(1000, Some(false), false));
    assert!(!job.at(2000, Some(true), false));
    assert!(job.seen);
    assert!(job.at(3000, Some(false), true));
    assert!(Looks::new().at(21_000, Some(false), false));
    assert!(Looks::new().at(0, None, false));
}

/// A program the login shell ran as it started, then the shell idle with no project, isn't
/// the create's end: that takes [`SETTLE`] idle, and a command running meanwhile (the create)
/// starts the wait over.
#[test]
fn a_job_that_made_nothing_is_over_only_once_the_shell_settles() {
    let mut job = Looks::new();
    assert!(!job.at(150, Some(true), false));
    assert!(!job.at(400, Some(false), false));
    assert!(!job.at(650, Some(true), false));
    assert_eq!(job.idle, None);
    assert!(!job.at(900, Some(false), false));
    let settled = 900 + SETTLE.as_millis() as u64;
    assert!(!job.at(settled - 1, Some(false), false));
    assert!(job.at(settled, Some(false), false));
}

/// Runs the page's create with `command` standing in for the CLI, in a real terminal, watching
/// it as the page does until it is over; returns the page and what it asked to open.
fn create_with(
    cx: &mut TestAppContext,
    location: &Path,
    command: &str,
) -> (Entity<NewProjectPage>, Vec<PathBuf>) {
    init(cx);
    cx.executor().allow_parking();
    let (window, terminal) = cx.update(|cx| {
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| cx.new(TerminalPanel::new))
            .expect("the test window opens")
    });
    cx.run_until_parked();
    let page = cx.new(|cx| {
        let mut page = NewProjectPage::blank(Stack::default(), location.to_path_buf(), cx);
        page.stack.project_name = "made".into();
        page
    });
    let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    cx.update(|cx| {
        let opened = opened.clone();
        cx.subscribe(&page, move |_, e: &NewProjectEvent, _| {
            if let NewProjectEvent::Open(dir) = e {
                opened.borrow_mut().push(dir.clone());
            }
        })
        .detach();
    });
    let job = page
        .update(cx, |p, cx| p.begin_create(cx))
        .expect("the create starts");
    assert_eq!(job.dir, location.join("made"));
    assert_eq!(
        page.read_with(cx, |p, _| p.work()),
        Some("Creating made".into())
    );
    cx.update_window(window, |_, window, cx| {
        terminal.update(cx, |t, cx| {
            t.start_job(
                job.tag,
                job.location,
                command,
                Some(Origin::User),
                window,
                cx,
            )
        })
    })
    .expect("the window is open")
    .expect("the terminal starts the job");
    page.update(cx, |p, _| p.terminal = Some(terminal.downgrade()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !page.update(cx, |p, cx| p.poll(cx)) {
        assert!(Instant::now() < deadline, "the create never ended");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(50));
    }
    cx.run_until_parked();
    let opened = opened.borrow().clone();
    (page, opened)
}

/// A create whose command makes the project's package.json: once its job ends, the page asks to
/// open the project.
#[gpui_kit::test]
fn a_created_project_is_opened(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-made")
        .canonicalize()
        .unwrap();
    let (page, opened) = create_with(
        cx,
        &dir,
        "mkdir made && echo '{}' > made/package.json && sleep 1",
    );
    assert_eq!(opened, [dir.join("made")]);
    page.read_with(cx, |p, _| {
        assert_eq!(p.creating, Creating::Done(dir.join("made")));
        assert_eq!(p.work(), None);
    });
}

/// A login shell runs programs of its own as it starts (nvm's `printf`, a greeting) before it
/// reads the create typed ahead, and may stay busy a while after one: a program that ran and
/// made nothing, the shell then idle, isn't the create ending. Here the line does the same:
/// a program, the shell itself busy (a `wait` keeps the terminal), then the create.
#[gpui_kit::test]
fn a_job_before_the_create_isnt_its_end(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-late")
        .canonicalize()
        .unwrap();
    let (page, opened) = create_with(
        cx,
        &dir,
        "sleep 0.6; sleep 0.6 & wait; mkdir made && echo '{}' > made/package.json && sleep 1",
    );
    assert_eq!(opened, [dir.join("made")]);
    page.read_with(cx, |p, _| {
        assert_eq!(p.creating, Creating::Done(dir.join("made")));
    });
}

/// A create that fails leaves the terminal and says so, with Try Again; nothing opens.
#[gpui_kit::test]
fn a_failed_create_says_so(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-failed")
        .canonicalize()
        .unwrap();
    let (page, opened) = create_with(cx, &dir, "sleep 1; false");
    assert!(opened.is_empty());
    page.read_with(cx, |p, _| {
        assert!(
            matches!(p.creating, Creating::Failed { .. }),
            "{:?}",
            p.creating
        );
    });
}
