//! Opening folders and files: the CLI's requests, ⌘O and the Open dialogs, each in the window that
//! has the folder already, else the front one, else a new one.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use gpui_kit::*;
use sawit_core::cli::{OpenRequest, REPLY_ERROR, REPLY_OK, Target};

use super::server::reply;
use super::{Waiter, Windows, frontmost, open_window, workspaces};
use crate::files::FilesPanel;
use crate::workspace::Workspace;

/// Opens a request from the CLI (or this process's own command line, with
/// no stream). An already-open project gets focus, otherwise a folder is
/// added to the frontmost window as another project (`-n`: a new window). Files go
/// to the window whose project holds them, else to the frontmost window.
pub fn handle(request: OpenRequest, mut stream: Option<UnixStream>, cx: &mut App) {
    let (dirs, files): (Vec<Target>, Vec<Target>) =
        request.targets.into_iter().partition(|t| t.path.is_dir());
    let mut last = None;
    for dir in dirs {
        last = open_folder(dir.path, request.new_window, request.reuse, cx).or(last);
    }
    // Files given with a folder open in that folder's window; with `-n`
    // they share one new window.
    let files_window = match (&last, request.new_window && !files.is_empty()) {
        (None, true) => open_window(None, cx),
        _ => last.clone(),
    };
    for file in &files {
        let Some((handle, workspace)) = files_window
            .clone()
            .or_else(|| window_for_file(&file.path, cx))
        else {
            continue;
        };
        let position = file.line.map(|line| (line, file.column.unwrap_or(1)));
        let path = file.path.clone();
        let _ = handle.update(cx, |_, window, cx| {
            workspace.update(cx, |w, cx| w.open_file_at(path, position, window, cx))
        });
        last = Some((handle, workspace));
    }
    // Just `sawit`: bring the app forward.
    if last.is_none() {
        last = match request.new_window {
            true => open_window(None, cx),
            false => frontmost(cx).or_else(|| open_window(None, cx)),
        };
    }
    let Some((handle, _)) = last else {
        if let Some(stream) = &mut stream {
            reply(
                stream,
                &format!("{REPLY_ERROR}unable to open a Sawit window"),
            );
        }
        return;
    };
    let _ = handle.update(cx, |_, window, _| window.activate_window());
    cx.activate(true);
    let Some(mut stream) = stream else { return };
    reply(&mut stream, REPLY_OK);
    if request.wait && !files.is_empty() {
        cx.default_global::<Windows>().waiters.push(Waiter {
            files: files.into_iter().map(|t| t.path).collect(),
            stream,
        });
    }
}

/// Asks for a folder, then opens it with [`open_folder_in`].
pub fn prompt_open_folder(prefer: Option<AnyWindowHandle>, cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open Folder".into()),
    });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = rx.await else {
            return;
        };
        if let Some(dir) = paths.into_iter().next() {
            cx.update(|cx| open_folder_in(dir, prefer, cx));
        }
    })
    .detach();
}

/// The window that has `dir` open as one of its projects ([`window_with_root`]).
fn window_with_project(dir: &Path, cx: &App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    let spaces = workspaces(cx);
    let roots: Vec<(usize, Vec<PathBuf>)> = spaces
        .iter()
        .enumerate()
        .map(|(ix, (_, w))| (ix, w.read(cx).roots(cx)))
        .collect();
    window_with_root(&roots, dir).map(|ix| spaces[ix].clone())
}

/// The window, of `windows` and each one's project folders, that has `dir` open as a project.
/// A window holds a project at its folder as it is on disk ([`Workspace::open_project`]), so
/// `dir` is compared so too ([`sawit_core::files::resolve`]): the Open dialog keeps a link picked
/// there as the link, and Recent Projects reopens a folder as it was remembered.
pub(super) fn window_with_root<H: Clone>(windows: &[(H, Vec<PathBuf>)], dir: &Path) -> Option<H> {
    let dir = sawit_core::files::resolve(dir);
    windows
        .iter()
        .find(|(_, roots)| roots.contains(&dir))
        .map(|(window, _)| window.clone())
}

/// Opens `dir` as a project of the window (showing it if already open there).
fn add_project(
    (handle, workspace): (AnyWindowHandle, Entity<Workspace>),
    dir: PathBuf,
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    handle
        .update(cx, |_, window, cx| {
            workspace.update(cx, |w, cx| w.open_project(dir, window, cx))
        })
        .ok()?;
    Some((handle, workspace))
}

/// Opens a folder picked in the app: shows it in the window that already
/// has it, else adds it to `prefer` (or the frontmost window) as another
/// project, else opens a window. Must not run while a workspace is being
/// updated (it reads them all).
pub fn open_folder_in(dir: PathBuf, prefer: Option<AnyWindowHandle>, cx: &mut App) {
    let target = window_with_project(&dir, cx)
        .or_else(|| {
            workspaces(cx)
                .into_iter()
                .find(|(h, _)| Some(*h) == prefer)
                .or_else(|| frontmost(cx))
        })
        .and_then(|found| add_project(found, dir.clone(), cx))
        .or_else(|| open_window(Some(dir), cx));
    if let Some((handle, _)) = target {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        cx.activate(true);
    }
}

/// Opens a folder picked with a project's Files tab, `asking` (its Open Project Folder button):
/// shows it in the window that has it already, as ⌘O does, else puts it in the place of that
/// project's folder in `workspace`, as `sawit -r` does ([`Workspace::replace_project_of`]), which
/// shows a folder the window has in another project and ungroups the tabs of the folder it
/// replaces. Must not run while a workspace is being updated (it reads them all).
pub fn open_folder_from_files(
    dir: PathBuf,
    window: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
    asking: WeakEntity<FilesPanel>,
    cx: &mut App,
) {
    let elsewhere = window_with_project(&dir, cx).filter(|(handle, _)| *handle != window);
    let target = match (elsewhere, workspace.upgrade()) {
        (Some(found), _) => add_project(found, dir, cx),
        (None, Some(workspace)) => window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |w, cx| w.replace_project_of(&asking, dir, window, cx))
            })
            .ok()
            .map(|()| (window, workspace)),
        (None, None) => None,
    };
    if let Some((handle, _)) = target {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        cx.activate(true);
    }
}

fn open_folder(
    dir: PathBuf,
    new_window: bool,
    reuse: bool,
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    // `-r` swaps the frontmost window's active project for this folder.
    if reuse && let Some((handle, workspace)) = frontmost(cx) {
        let replaced = handle.update(cx, |_, window, cx| {
            workspace.update(cx, |w, cx| w.replace_project(dir.clone(), window, cx))
        });
        if replaced.is_ok() {
            return Some((handle, workspace));
        }
    }
    // Otherwise every folder joins one window as another project; `-n`
    // asks for a window of its own.
    if !new_window
        && let Some(found) = window_with_project(&dir, cx).or_else(|| frontmost(cx))
        && let Some(opened) = add_project(found, dir.clone(), cx)
    {
        return Some(opened);
    }
    open_window(Some(dir), cx)
}

/// The window whose project holds `path` (the deepest one), else the
/// frontmost window, else a new one.
pub(super) fn window_for_file(
    path: &Path,
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    workspaces(cx)
        .into_iter()
        .filter_map(|(h, w)| {
            let root = crate::tabs::project_of(&w.read(cx).roots(cx), path)?;
            Some((root.components().count(), (h, w)))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, found)| found)
        .or_else(|| frontmost(cx))
        .or_else(|| open_window(None, cx))
}
