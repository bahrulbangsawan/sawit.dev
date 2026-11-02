//! The `sawit` command, like `zed`. From a terminal it hands its paths to
//! the running app over a Unix socket (launching the app first when needed)
//! and returns, or blocks with `--wait`. Inside the app, the socket server
//! routes each request to the right window, opening new ones as needed.

use std::collections::HashSet;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use sawit_core::cli::REPLY_DONE;

use crate::terminal::TerminalPanel;
use crate::workspace::Workspace;

mod command_line;
mod install;
mod open;
mod server;
#[cfg(test)]
mod tests;

pub use command_line::{bundle_of, command_line};
use install::GlobalCliStatus;
pub use install::{CliStatus, cli_status, install_cli};
use open::window_for_file;
pub use open::{handle, open_folder_from_files, open_folder_in, prompt_open_folder};
use server::reply;
pub use server::serve;

actions!(sawit, [InstallCli, Quit, NewWindow, OpenFolder]);

// ---------------------------------------------------------------------------
// Inside the app
// ---------------------------------------------------------------------------

struct WindowEntry {
    handle: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
}

/// A `--wait` client, answered once none of `files` is open anywhere.
struct Waiter {
    files: Vec<PathBuf>,
    stream: UnixStream,
}

#[derive(Default)]
struct Windows {
    entries: Vec<WindowEntry>,
    waiters: Vec<Waiter>,
}

impl Global for Windows {}

pub fn init(cx: &mut App) {
    let status = cx.new(|_| CliStatus {
        installed: CliStatus::detect(),
    });
    cx.set_global(GlobalCliStatus(status));
    cx.on_action(|_: &InstallCli, cx| install_cli(cx));
    // The question goes up outside the window update a key press runs in.
    cx.on_action(|_: &Quit, cx| cx.defer(quit));
    cx.on_action(|_: &NewWindow, cx| {
        if let Some((handle, _)) = open_window(None, cx) {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
        }
    });
    cx.on_action(|_: &OpenFolder, cx| {
        let prefer = frontmost(cx).map(|(h, _)| h);
        prompt_open_folder(prefer, cx);
    });
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-shift-n", NewWindow, None),
        KeyBinding::new("cmd-o", OpenFolder, None),
    ]);
    set_menus(cx);
    cx.on_window_closed(|cx, _| check_waiters(cx)).detach();
}

/// Sets the app's menus. Their shortcuts are the keymap's as it is now, so the keymap sets them
/// again whenever it changes.
pub fn set_menus(cx: &mut App) {
    cx.set_menus(menus(cx));
}

/// The app's menus. The Edit menu's items are the system's (`cut:`, `copy:`, `paste:`,
/// `selectAll:`), sent to the view with AppKit's keyboard: a web page's clipboard keys reach it
/// that way, which no binding of Sawit's takes there. Elsewhere Sawit's own bindings of the same
/// keys run first, and a click on an item runs gpui-kit's text editing action in the field with
/// the keyboard. The View menu's item says what it does now: hide the agents' cursors, or show
/// them.
fn menus(cx: &App) -> Vec<Menu> {
    use gpui_kit::component::input as text;
    vec![
        Menu::new("Sawit").items([
            MenuItem::action("Keymap Editor", crate::keymap::OpenKeymapEditor),
            MenuItem::action("Open Keymap File", crate::keymap::OpenKeymapFile),
            MenuItem::separator(),
            MenuItem::action("Install ‘sawit’ Command in PATH…", InstallCli),
            MenuItem::separator(),
            MenuItem::action("Quit Sawit", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Window", NewWindow),
            MenuItem::action("New Project…", crate::workspace::OpenNewProject),
            MenuItem::action("Open Folder…", OpenFolder),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Cut", text::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", text::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", text::Paste, OsAction::Paste),
            MenuItem::os_action("Select All", text::SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([MenuItem::action(
            crate::agents::menu_label(cx),
            crate::agents::ToggleAgentCursors,
        )]),
    ]
}

/// Opens a Sawit window, optionally with a project folder.
pub fn open_window(
    project: Option<PathBuf>,
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1280.), px(800.)), cx)),
        window_min_size: Some(size(px(720.), px(480.))),
        ..TitleBar::window_options()
    };
    let (handle, workspace) = gpui_kit::open_window(options, cx, |window, cx| {
        window.set_window_title("Sawit");
        cx.new(|cx| {
            let mut workspace = Workspace::new(window, cx);
            if let Some(project) = project {
                workspace.open_project(project, window, cx);
            }
            workspace
        })
    })
    .inspect_err(|e| eprintln!("sawit: failed to open a window: {e}"))
    .ok()?;
    // Closing a tab notifies the workspace: a `--wait` may be over.
    cx.observe(&workspace, |_, cx| check_waiters(cx)).detach();
    let windows = cx.default_global::<Windows>();
    windows.entries.retain(|e| e.workspace.upgrade().is_some());
    windows.entries.push(WindowEntry {
        handle,
        workspace: workspace.downgrade(),
    });
    Some((handle, workspace))
}

/// Opens each file whose unsaved changes the app kept when it last quit without asking (the Dock's
/// Quit, logging out, the install script's quit), in the window whose project holds it, with the
/// changes back in it, unsaved ([`crate::editor::kept`]).
pub fn restore_kept(cx: &mut App) {
    let folder = crate::editor::kept::folder(cx);
    for (from, kept) in crate::editor::kept::all(&folder) {
        let Some((handle, workspace)) = window_for_file(&kept.path, cx) else {
            continue;
        };
        let _ = handle.update(cx, |_, window, cx| {
            workspace.update(cx, |w, cx| w.restore_kept(kept, from, window, cx))
        });
    }
}

/// Dock icon clicked with every window closed.
pub fn reopen(cx: &mut App) {
    if workspaces(cx).is_empty() {
        open_window(None, cx);
    }
}

/// Quits Sawit. Quitting ends every terminal, so it asks first while one
/// runs something.
fn quit(cx: &mut App) {
    let windows = terminal_panels(cx);
    crate::terminal::end_all(windows, crate::terminal::Ending::Quit, cx, |cx| cx.quit());
}

/// Every window's terminal dock, front to back.
pub fn terminal_panels(cx: &App) -> Vec<(AnyWindowHandle, Entity<TerminalPanel>)> {
    let stack = cx.window_stack().unwrap_or_default();
    let mut spaces = workspaces(cx);
    spaces.sort_by_key(|(h, _)| stack.iter().position(|s| s == h).unwrap_or(usize::MAX));
    spaces
        .into_iter()
        .map(|(h, w)| (h, w.read(cx).terminal().clone()))
        .collect()
}

/// Live windows, in the order they were opened.
fn workspaces(cx: &App) -> Vec<(AnyWindowHandle, Entity<Workspace>)> {
    let open = cx.windows();
    let Some(windows) = cx.try_global::<Windows>() else {
        return Vec::new();
    };
    windows
        .entries
        .iter()
        .filter(|e| open.contains(&e.handle))
        .filter_map(|e| Some((e.handle, e.workspace.upgrade()?)))
        .collect()
}

/// The workspace shown in `window`, if it is a Sawit window.
pub fn workspace_for(window: AnyWindowHandle, cx: &App) -> Option<Entity<Workspace>> {
    workspaces(cx)
        .into_iter()
        .find_map(|(h, w)| (h == window).then_some(w))
}

/// The front window's active project folder.
pub fn front_project(cx: &App) -> Option<PathBuf> {
    frontmost(cx).and_then(|(_, w)| w.read(cx).root(cx))
}

fn frontmost(cx: &App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    let spaces = workspaces(cx);
    let stack = cx.window_stack().unwrap_or_default();
    stack
        .iter()
        .find_map(|h| spaces.iter().find(|(w, _)| w == h).cloned())
        .or_else(|| spaces.last().cloned())
}

/// Answers every `--wait` whose files are all closed.
fn check_waiters(cx: &mut App) {
    if cx
        .try_global::<Windows>()
        .is_none_or(|w| w.waiters.is_empty())
    {
        return;
    }
    let open: HashSet<PathBuf> = workspaces(cx)
        .into_iter()
        .flat_map(|(_, w)| w.read(cx).open_files(cx))
        .collect();
    cx.default_global::<Windows>().waiters.retain_mut(|w| {
        let waiting = still_open(&w.files, &open);
        if !waiting {
            reply(&mut w.stream, REPLY_DONE);
        }
        waiting
    });
}

/// Whether a `--wait` on `files` waits on: one of them is in `open`, the files the windows' tabs
/// show, itself or, for an `.excalidraw` file, as the `.sawit` file beside it that its canvas
/// is bound to and its tab shows (CANVAS-UI-7).
fn still_open(files: &[PathBuf], open: &HashSet<PathBuf>) -> bool {
    files
        .iter()
        .any(|file| open.contains(file) || open.contains(&sawit_board::store::bound_file(file)))
}
