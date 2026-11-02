//! The workspace's tests: its keys, tabs, canvases and browsers, and where the keyboard goes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::Modifiers;

use super::areas::{settle_canvas_fill, toggle_canvas_fill};
use super::browser::browser_key;
use super::canvas::canvas_project;
use super::file_tabs::moved_to;
use super::{CENTER_ONLY, bindings};
use crate::layout::{Area, Layout};

/// Each project has its own Browser tab, as it has its own canvas: its cookies, its
/// annotation screenshots and its design exports are the project's ("a browser tab per
/// project"). A window used to keep one, tied to the project it was opened in.
#[test]
fn each_project_has_its_own_browser() {
    let (a, b) = (Path::new("/work/a"), Path::new("/work/b"));
    assert_ne!(browser_key(Some(a)), browser_key(Some(b)));
    assert_ne!(browser_key(Some(a)), browser_key(None));
    assert_eq!(
        browser_key(Some(a)),
        browser_key(Some(&PathBuf::from("/work/a")))
    );
    // `app_state` names a tab's kind by its key's first part.
    assert_eq!(browser_key(Some(a)).split('\u{0}').next(), Some("browser"));
    assert_eq!(browser_key(None).as_ref(), "browser");
}

/// A keystroke written as `ctrl-alt-shift-cmd-key`.
fn page_key(m: &Modifiers, key: &str) -> String {
    let mut out = String::new();
    for (on, name) in [
        (m.control, "ctrl-"),
        (m.alt, "alt-"),
        (m.shift, "shift-"),
        (m.platform, "cmd-"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    out + key
}

/// The Dev Tools tab counts as unsaved while it installs: closing it alone asks first
/// (closing stops the installer), and closing others or all leaves it open.
#[gpui_kit::test]
fn a_dev_tools_tab_that_installs_counts_as_unsaved(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::AppContext as _;
    let busy = super::PaneItem::Other {
        view: cx
            .new(|cx| crate::tools_page::ToolsPage::installing("bun", cx))
            .into(),
        key: "tools".into(),
        title: "Dev Tools".into(),
        icon: gpui_kit::assets::IconName::Package,
        file: None,
    };
    cx.update(|cx| {
        assert_eq!(
            super::pages::tools_work(&busy, cx).as_deref(),
            Some("Installing Bun")
        );
        assert_eq!(super::pages::tools_work(&super::PaneItem::Server, cx), None);
    });
}

#[test]
fn a_projects_canvas_file_is_that_projects_canvas() {
    let app = Path::new("/support/sawit");
    let (a, b) = (PathBuf::from("/work/a"), PathBuf::from("/work/b"));
    let roots = vec![Some(a.clone()), Some(b.clone()), None];
    let project = |file: &str| canvas_project(Path::new(file), roots.clone(), app);
    // Each project's canvas file, and for the project with no folder the app's own.
    assert_eq!(project("/work/b/.nyawit/canvas.sawit"), Some(Some(b)));
    assert_eq!(project("/work/a/.nyawit/canvas.sawit"), Some(Some(a)));
    assert_eq!(project("/support/sawit/canvas.sawit"), Some(None));
    // Any other file is one of its own: another file in `.nyawit`, a canvas file elsewhere,
    // the canvas file of a project the window doesn't have.
    assert_eq!(project("/work/a/.nyawit/sketch.sawit"), None);
    assert_eq!(project("/work/a/canvas.sawit"), None);
    assert_eq!(project("/work/c/.nyawit/canvas.sawit"), None);
    // With every project in a folder, the app's canvas file is one of its own too.
    let app_canvas = Path::new("/support/sawit/canvas.sawit");
    let folders = [Some(PathBuf::from("/work/a"))];
    assert_eq!(canvas_project(app_canvas, folders, app), None);
}

/// A renamed or moved file, and the files of a moved folder, are where the move put them:
/// the tabs of file editors and of canvas files follow them there (CANVAS-UI-3).
#[test]
fn a_moved_path_is_found_where_the_move_put_it() {
    let (from, to) = (Path::new("/p/sub"), Path::new("/p/moved"));
    let moved = |path: &str| moved_to(Path::new(path), from, to);
    assert_eq!(moved("/p/sub"), Some(PathBuf::from("/p/moved")));
    assert_eq!(
        moved("/p/sub/a.sawit"),
        Some(PathBuf::from("/p/moved/a.sawit"))
    );
    assert_eq!(
        moved("/p/sub/deeper/b.sawit"),
        Some(PathBuf::from("/p/moved/deeper/b.sawit"))
    );
    // Beside it, or with a name that only starts the same: where it was.
    assert_eq!(moved("/p/other.sawit"), None);
    assert_eq!(moved("/p/sub2/a.sawit"), None);
    assert_eq!(moved("/p"), None);
}

/// CANVAS-UI-4: a project's canvas file named through a link (an agent's `app_open` of
/// `/tmp/p/.nyawit/canvas.sawit` for the project at `/private/tmp/p`, `/tmp` being a link
/// on macOS) is that project's canvas still, whether `.nyawit/` is there yet or not: one
/// canvas per file. Compared as written, it was a file of its own, and a second canvas of it
/// opened in a tab, whose saves the first one's refused.
#[test]
fn a_projects_canvas_file_named_through_a_link_is_that_projects_canvas() {
    let scratch = std::env::temp_dir().join(format!(
        "sawit-workspace-{}-canvas-link",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let link = scratch.join("link");
    std::os::unix::fs::symlink(&project, &link).unwrap();
    let app = Path::new("/support/sawit");
    let roots = || vec![Some(project.clone()), None];
    let aliased = link.join(".nyawit").join("canvas.sawit");
    assert_eq!(
        canvas_project(&aliased, roots(), app),
        Some(Some(project.clone()))
    );
    std::fs::create_dir_all(project.join(".nyawit")).unwrap();
    std::fs::write(project.join(".nyawit/canvas.sawit"), b"").unwrap();
    assert_eq!(
        canvas_project(&aliased, roots(), app),
        Some(Some(project.clone()))
    );
    // Another file through the link is still one of its own.
    let other = link.join(".nyawit").join("sketch.sawit");
    assert_eq!(canvas_project(&other, roots(), app), None);
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn no_two_shortcuts_share_a_keystroke() {
    let mut seen = HashSet::new();
    let global = ["cmd-q", "shift-cmd-n", "cmd-o"];
    for binding in bindings() {
        let keys: Vec<String> = binding
            .keystrokes()
            .iter()
            .map(|k| page_key(k.modifiers(), k.key()))
            .collect();
        let keys = keys.join(" ");
        assert!(
            !global.contains(&keys.as_str()),
            "{keys} is taken by the app"
        );
        assert!(seen.insert(keys.clone()), "{keys} is bound twice");
    }
    // A chord's first keystroke can't be a shortcut of its own.
    for chord in seen.iter().filter(|k| k.contains(' ')) {
        let first = chord.split(' ').next().unwrap();
        assert!(!seen.contains(first), "{first} shadows {chord}");
    }
}

/// ⇧⌘↩ fills the window with the canvas and a second ⇧⌘↩ puts the layout back; anything else
/// that changes what the window shows ends it too, and the tab bar comes back (ruling P8-C3).
/// The tab bar shows unless the canvas fills the window (`render_center`), and each render
/// settles the fill against what is on screen (`sync_canvas_maximized`).
#[test]
fn maximize_still_works_without_webview() {
    let before = Layout::default();
    let (mut maximized, mut layout, mut unmaximized) = (false, before, None);
    toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
    assert!(maximized, "the tab bar is hidden");
    assert_eq!(layout, CENTER_ONLY);
    // While the canvas stays the tab on screen, renders leave it filling the window.
    settle_canvas_fill(&mut maximized, &mut unmaximized, layout, true);
    assert!(maximized);
    toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
    assert!(!maximized, "the tab bar is back");
    assert_eq!(layout, before);

    // ⌘B shows the sidebar: the next render ends the fill, and the sidebar stays.
    toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
    layout.toggle(Area::Sidebar);
    settle_canvas_fill(&mut maximized, &mut unmaximized, layout, true);
    assert!(!maximized, "the tab bar is back");
    assert_eq!(unmaximized, None);
    assert!(layout.sidebar && layout.center && !layout.terminal);

    // Another tab is activated: the canvas is no longer on screen, which ends the fill too.
    let (mut maximized, mut layout, mut unmaximized) = (false, before, None);
    toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
    settle_canvas_fill(&mut maximized, &mut unmaximized, layout, false);
    assert!(!maximized, "the tab bar is back");
    assert_eq!(unmaximized, None);
    // ⇧⌘↩ on the canvas again fills the window from the layout as it is now.
    toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
    assert!(maximized);
    assert_eq!(unmaximized, Some(CENTER_ONLY));
}

mod keyboard;

/// What had the keyboard and goes off screen gives it to the workspace
/// ([`super::KeyboardHome`]), whatever it is: a terminal pane once ⌘J hides the dock, a file
/// editor's text once ⌃Tab shows another tab. Before, it kept the keyboard unseen, GPUI sent
/// the keys from the window's root, and the second ⌘J or ⌃Tab did nothing. Each of three
/// presses works, and the keyboard ends where it belongs: with the workspace once the dock is
/// hidden, with the text once ⌃Tab has gone round to its tab again. (The browser's page and
/// address field: `browser::tests`.)
#[gpui_kit::test]
fn what_goes_off_screen_gives_the_keyboard_to_the_workspace(cx: &mut gpui_kit::TestAppContext) {
    use self::keyboard::{Keyboard, has, open};
    use gpui_kit::Focusable as _;

    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let mut failed = Vec::new();
    let home: Keyboard = |this, _| this.focus.clone();
    let pane: Keyboard = |this, _| this.pane.clone();
    let text: Keyboard = |this, cx| this.text.read(cx).focus_handle(cx);
    for (what, keyboard, keys, ends) in [
        ("a terminal pane", pane, "cmd-j", ("the workspace", home)),
        ("a file's text", text, "ctrl-tab", ("the text", text)),
    ] {
        let (window, workspace) = open(keyboard, cx);
        for _ in 0..3 {
            cx.simulate_keystrokes(window, keys);
            cx.run_until_parked();
        }
        let presses = cx.update(|cx| workspace.read(cx).presses);
        let has_it = has(ends.1, window, &workspace, cx);
        if presses != 3 || !has_it {
            failed.push(format!(
                "with the keyboard in {what}, {keys} pressed 3 times worked {presses} times; \
                     {} has the keyboard = {has_it}",
                ends.0
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// What went off screen with the keyboard has it again once it is shown again
/// ([`super::KeyboardHome::give_back`], review I4): a terminal pane hidden and shown by the
/// title bar's button (clicks, no key) or by ⌘J twice, and a file's text whose tab is left and
/// come back to by clicks, take the typing that follows, as they did before the workspace took
/// the keyboard from what is hidden. What has been clicked meanwhile keeps the keyboard, and
/// the browser's page (here the stand-in's page) waits for a click, as it always did.
#[gpui_kit::test]
fn what_is_shown_again_has_the_keyboard_again(cx: &mut gpui_kit::TestAppContext) {
    use self::keyboard::{Keyboard, has, open, show};
    use gpui_kit::{AppContext as _, Focusable as _};

    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let mut failed = Vec::new();
    let home: Keyboard = |this, _| this.focus.clone();
    let pane: Keyboard = |this, _| this.pane.clone();
    let text: Keyboard = |this, cx| this.text.read(cx).focus_handle(cx);
    let page: Keyboard = |this, _| this.page.clone();

    let (window, workspace) = open(pane, cx);
    for dock in [false, true] {
        show(&workspace, cx, |w| w.dock = dock);
    }
    cx.simulate_keystrokes(window, "l s");
    let typed = cx.update(|cx| workspace.read(cx).typed.clone());
    if typed != ["l", "s"] {
        failed.push(format!(
            "the terminal pane hidden and shown by clicks got {typed:?} of `l s`"
        ));
    }

    let (window, workspace) = open(pane, cx);
    for _ in 0..2 {
        cx.simulate_keystrokes(window, "cmd-j");
        cx.run_until_parked();
    }
    cx.simulate_keystrokes(window, "l s");
    let (dock, typed) = cx.update(|cx| {
        let workspace = workspace.read(cx);
        (workspace.dock, workspace.typed.clone())
    });
    if !dock || typed != ["l", "s"] {
        failed.push(format!(
            "after cmd-j twice the dock shows = {dock}, and the pane got {typed:?} of `l s`"
        ));
    }

    let (window, workspace) = open(text, cx);
    for tab in [1, 0] {
        show(&workspace, cx, |w| w.tab = tab);
    }
    cx.simulate_input(window, "hi");
    let value = cx.update(|cx| workspace.read(cx).text.read(cx).value().to_string());
    if value != "hi" {
        failed.push(format!(
            "the file's text, its tab left and come back to, got {value:?} of \"hi\""
        ));
    }

    let (window, workspace) = open(text, cx);
    show(&workspace, cx, |w| w.tab = 1);
    let to = cx.update(|cx| pane(workspace.read(cx), cx));
    cx.update_window(window, |_, window, cx| window.focus(&to, cx))
        .unwrap();
    cx.run_until_parked();
    show(&workspace, cx, |w| w.tab = 0);
    if !has(pane, window, &workspace, cx) {
        failed.push(
            "the terminal pane clicked while the text was hidden lost the keyboard when \
                 the text was shown again"
                .into(),
        );
    }

    let (window, workspace) = open(page, cx);
    for tab in [1, 0] {
        show(&workspace, cx, |w| w.tab = tab);
    }
    if !has(home, window, &workspace, cx) {
        failed.push("the page, hidden and shown, took the keyboard back".into());
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Given back while it is still off screen, the keyboard comes home ([`super::KeyboardHome`],
/// review I5): GPUI takes it from what isn't drawn only in a frame after one with a keyboard
/// path, and the frame that hid it had none. So two changes on screen in a row, the second
/// showing something else again, left the keyboard on the hidden view, past every workspace
/// shortcut, until a click: ⌘J, ⌃Tab, ⌘J from a terminal pane left the dock hidden; ⌃Tab,
/// ⌘J, ⌃Tab from a file's text lost the last ⌃Tab. Also: what is clicked meanwhile makes the
/// workspace forget what it took, and a redraw that changes nothing gives nothing back (one
/// would, given back to what is hidden, come home and ask for the next frame, without end).
#[gpui_kit::test]
fn the_keyboard_never_stays_on_what_is_hidden(cx: &mut gpui_kit::TestAppContext) {
    use self::keyboard::{Keyboard, has, open, show};
    use gpui_kit::{AppContext as _, Focusable as _};

    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let mut failed = Vec::new();
    let home: Keyboard = |this, _| this.focus.clone();
    let pane: Keyboard = |this, _| this.pane.clone();
    let text: Keyboard = |this, cx| this.text.read(cx).focus_handle(cx);

    for (what, from, keys, presses, ends) in [
        (
            "⌘J, ⌃Tab, ⌘J",
            pane,
            "cmd-j ctrl-tab cmd-j",
            3,
            ("the pane", pane),
        ),
        (
            "⌃Tab, ⌘J, ⌃Tab, ⌃Tab",
            text,
            "ctrl-tab cmd-j ctrl-tab ctrl-tab",
            4,
            ("the text", text),
        ),
    ] {
        let (window, workspace) = open(from, cx);
        for key in keys.split(' ') {
            cx.simulate_keystrokes(window, key);
            cx.run_until_parked();
        }
        let worked = cx.update(|cx| workspace.read(cx).presses);
        let has_it = has(ends.1, window, &workspace, cx);
        if worked != presses || !has_it {
            failed.push(format!(
                "{what}: {worked} of {presses} presses worked; {} has the keyboard = {has_it}",
                ends.0
            ));
        }
    }

    // The text's tab left, then the dock hidden, both by clicks: ⌘J still works.
    let (window, workspace) = open(text, cx);
    show(&workspace, cx, |w| w.tab = 1);
    show(&workspace, cx, |w| w.dock = false);
    cx.simulate_keystrokes(window, "cmd-j");
    cx.run_until_parked();
    let (worked, dock) = cx.update(|cx| (workspace.read(cx).presses, workspace.read(cx).dock));
    if worked != 1 || !dock || !has(home, window, &workspace, cx) {
        failed.push(format!(
            "after the text's tab and the dock were hidden by clicks, cmd-j worked {worked} \
                 times and the dock shows = {dock}"
        ));
    }

    // What is clicked meanwhile makes the workspace forget the text: the workspace gets the
    // keyboard back another way (as a canvas going off screen gives it), and the text's tab
    // shown again doesn't take it.
    let (window, workspace) = open(text, cx);
    show(&workspace, cx, |w| w.tab = 1);
    for to in [pane, home] {
        let to = cx.update(|cx| to(workspace.read(cx), cx));
        cx.update_window(window, |_, window, cx| window.focus(&to, cx))
            .unwrap();
        cx.run_until_parked();
    }
    show(&workspace, cx, |w| w.tab = 0);
    if !has(home, window, &workspace, cx) {
        failed.push("the text took the keyboard back after the pane had it meanwhile".into());
    }

    // A redraw that changes nothing gives nothing back; the text's tab shown again does.
    let (window, workspace) = open(text, cx);
    show(&workspace, cx, |w| w.tab = 1);
    show(&workspace, cx, |_| {});
    let home_had_it = has(home, window, &workspace, cx);
    show(&workspace, cx, |w| w.tab = 0);
    if !home_had_it || !has(text, window, &workspace, cx) {
        failed.push(format!(
            "after a redraw with the text's tab hidden the workspace had the keyboard = \
                 {home_had_it}, and the text didn't get it back when shown"
        ));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
