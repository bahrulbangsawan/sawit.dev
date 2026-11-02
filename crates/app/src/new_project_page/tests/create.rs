//! The create itself, in a real terminal: a project made is opened (or, from Create Project
//! Only, made only), a create that fails says so.

use super::*;

/// Runs the page's create with `command` standing in for the CLI, in a real terminal, watching
/// it as the page does until it is over; returns the page and what it asked to open.
fn create_with(
    cx: &mut TestAppContext,
    location: &Path,
    command: &str,
) -> (Entity<NewProjectPage>, Vec<PathBuf>) {
    create_with_open(cx, location, command, true)
}

/// [`create_with`], as Create Project (`open`) or Create Project Only.
fn create_with_open(
    cx: &mut TestAppContext,
    location: &Path,
    command: &str,
    open: bool,
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
        .update(cx, |p, cx| p.begin_create_and(open, cx))
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
        assert_eq!(
            p.creating,
            Creating::Done {
                dir: dir.join("made"),
                open: true
            }
        );
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
        assert_eq!(
            p.creating,
            Creating::Done {
                dir: dir.join("made"),
                open: true
            }
        );
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

/// Create Project Only: the project is made and the page says so, with Open Project; nothing opens
/// until that is clicked, and then the project made does.
#[gpui_kit::test]
fn create_project_only_makes_it_without_opening_it(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-only")
        .canonicalize()
        .unwrap();
    let made = "mkdir made && echo '{}' > made/package.json && sleep 1";
    let (page, opened) = create_with_open(cx, &dir, made, false);
    assert!(opened.is_empty(), "nothing opened: {opened:?}");
    let done = page.read_with(cx, |p, _| p.creating.clone());
    assert_eq!(
        done,
        Creating::Done {
            dir: dir.join("made"),
            open: false
        }
    );

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
    page.update(cx, |p, cx| p.open_made(cx));
    cx.run_until_parked();
    assert_eq!(*opened.borrow(), [dir.join("made")]);
    let opening = page.read_with(cx, |p, _| p.creating.clone());
    assert_eq!(
        opening,
        Creating::Done {
            dir: dir.join("made"),
            open: true
        }
    );
}

/// A Create Project Only that fails remembers it was one, so Try Again doesn't open the project.
#[gpui_kit::test]
fn a_failed_create_only_tries_again_without_opening(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("new-project-only-failed")
        .canonicalize()
        .unwrap();
    let (page, opened) = create_with_open(cx, &dir, "sleep 1; false", false);
    assert!(opened.is_empty());
    let failed = page.read_with(cx, |p, _| p.creating.clone());
    assert!(
        matches!(failed, Creating::Failed { open: false, .. }),
        "{failed:?}"
    );
}
