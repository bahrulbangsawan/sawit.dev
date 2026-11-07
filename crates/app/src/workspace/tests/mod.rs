//! The workspace's tests: its keys, tabs, canvases and browsers, and where the keyboard goes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::Modifiers;

use super::browser::browser_key;
use super::canvas::canvas_project;
use super::file_tabs::moved_to;
use super::fill::settle_tab_fill;
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
    // The app's own (`cli::init`): Quit, New Window (⌥⌘N since B5) and Open Folder.
    let global = ["cmd-q", "alt-cmd-n", "cmd-o"];
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

/// B5: macOS and the Edit menu keep these for themselves: the app switcher, Spotlight, full
/// screen, Hide, Minimize, Quit, Close Window, Preferences and the clipboard and undo keys. No
/// default binding of the workspace, the terminal, the file tree, the browser or the canvas's
/// workspace keys takes one (the canvas's and the file tree's own Copy and Paste act on what they
/// hold, as the Edit menu's would).
const MACOS_KEYS: &[&str] = &[
    "cmd-tab",
    "shift-cmd-tab",
    "cmd-space",
    "ctrl-cmd-f",
    "cmd-h",
    "alt-cmd-h",
    "cmd-m",
    "cmd-q",
    "cmd-,",
    "cmd-c",
    "cmd-v",
    "cmd-x",
    "cmd-z",
    "shift-cmd-z",
    "cmd-a",
    "cmd-`",
];

#[test]
fn no_default_binding_takes_a_macos_key() {
    for binding in bindings() {
        let keys: Vec<String> = binding
            .keystrokes()
            .iter()
            .map(|k| page_key(k.modifiers(), k.key()))
            .collect();
        let keys = keys.join(" ");
        assert!(!MACOS_KEYS.contains(&keys.as_str()), "{keys} is macOS's");
    }
}

/// B5's keys, each the one action the backlog gives it, in the workspace; ⌘J is free.
#[test]
fn the_area_keys_open_close_and_fill() {
    let bound: Vec<(String, String)> = bindings()
        .iter()
        .map(|b| {
            let keys = crate::keymap::names::unparse(b.keystrokes());
            (keys, b.action().name().to_string())
        })
        .collect();
    let action = |key: &str| {
        let found: Vec<&str> = (bound.iter())
            .filter(|(k, _)| k == key)
            .map(|(_, a)| a.as_str())
            .collect();
        assert_eq!(found.len(), 1, "{key}: {found:?}");
        found[0].to_string()
    };
    for (key, name) in [
        ("cmd-t", "workspace::ToggleTerminal"),
        ("cmd-shift-t", "workspace::MaximizeTerminal"),
        ("alt-cmd-t", "workspace::NewTerminal"),
        ("cmd-u", "workspace::ToggleCanvas"),
        ("cmd-shift-c", "workspace::MaximizeCanvas"),
        ("cmd-b", "workspace::ToggleBrowser"),
        ("cmd-shift-b", "workspace::MaximizeBrowser"),
        ("cmd-e", "workspace::ToggleSidebar"),
        ("cmd-n", "workspace::NewFileInProject"),
        ("cmd-shift-n", "workspace::OpenNewProject"),
        ("alt-tab", "workspace::FocusNextPanel"),
        ("alt-shift-tab", "workspace::FocusPreviousPanel"),
        ("cmd-k o", "workspace::CloseOtherTabs"),
        ("ctrl-cmd-c", "workspace::OpenCanvas"),
    ] {
        assert_eq!(action(key), name, "{key}");
    }
    assert!(!bound.iter().any(|(k, _)| k == "cmd-j"), "⌘J is left free");
    assert!(!bound.iter().any(|(k, _)| k == "ctrl-cmd-n"), "⌃⌘N is gone");
}

/// ⇧⌘↩ fills the window with the canvas or the browser, and a second ⇧⌘↩ puts the layout back;
/// anything else that changes what the window shows ends it too, and the tab bar comes back (ruling
/// P8-C3). The tab bar shows unless a tab fills the window (`render_center`), and each render
/// settles the fill against what is on screen (`sync_filled`).
#[test]
fn a_filled_tab_ends_once_something_else_shows() {
    let tab = 7_u32;
    // While the tab stays the one on screen, alone, renders leave it filling the window.
    let (mut filled, mut unmaximized) = (Some(tab), Some(Layout::default()));
    settle_tab_fill(&mut filled, &mut unmaximized, CENTER_ONLY, Some(tab));
    assert_eq!(filled, Some(tab));
    assert_eq!(unmaximized, Some(Layout::default()));

    // ⌘B shows the sidebar: the next render ends the fill, and the sidebar stays.
    let mut layout = CENTER_ONLY;
    layout.toggle(Area::Sidebar);
    settle_tab_fill(&mut filled, &mut unmaximized, layout, Some(tab));
    assert_eq!(filled, None, "the tab bar is back");
    assert_eq!(unmaximized, None);

    // Another tab is activated: the tab is no longer on screen, which ends the fill too.
    let (mut filled, mut unmaximized) = (Some(tab), Some(Layout::default()));
    settle_tab_fill(&mut filled, &mut unmaximized, CENTER_ONLY, Some(8));
    assert_eq!(filled, None);
    let (mut filled, mut unmaximized) = (Some(tab), Some(Layout::default()));
    settle_tab_fill(&mut filled, &mut unmaximized, CENTER_ONLY, None);
    assert_eq!(filled, None);
    assert_eq!(unmaximized, None);
}

/// The panels' direct keys, ⌃⌘1–⌃⌘3, are free: no other binding of the app's, and none of the
/// canvas's (its panels are ⌥1–⌥3), and Escape restores the layout only while a panel fills the
/// window.
#[test]
fn maximize_keys_are_free() {
    let bound: Vec<(String, Option<String>)> = bindings()
        .iter()
        .map(|b| {
            let keys = crate::keymap::names::unparse(b.keystrokes());
            (keys, b.predicate().map(|p| p.to_string()))
        })
        .collect();
    for key in ["ctrl-cmd-1", "ctrl-cmd-2", "ctrl-cmd-3"] {
        assert_eq!(bound.iter().filter(|(k, _)| k == key).count(), 1, "{key}");
    }
    let escape = bound.iter().find(|(k, _)| k == "escape").expect("escape");
    assert!(escape.1.as_deref().unwrap_or("").contains("maximized"));
    for binding in sawit_board::shortcuts::defaults() {
        for key in binding.keys {
            let taken = matches!(*key, "Ctrl+Cmd+1" | "Ctrl+Cmd+2" | "Ctrl+Cmd+3");
            assert!(!taken, "the canvas's {} takes {key}", binding.id);
        }
    }
}

mod focus;
mod keyboard;
