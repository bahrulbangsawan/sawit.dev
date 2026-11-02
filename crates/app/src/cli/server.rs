//! The socket that the `sawit` command and `sawit-mcp` talk to: each request is read off the UI
//! thread, then handled on it, and a `sawit-mcp` command goes to the window it is for.

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::app_control::{self, Command as ControlCommand, reply_error, reply_ok};
use sawit_core::cli::{OpenRequest, REPLY_ERROR, Target, decode};
use sawit_core::instances::AppPaths;

use super::open::handle;
use super::{frontmost, open_window, workspaces};
use crate::rt;
use crate::workspace::{Answer, Workspace};

/// A request from the socket: the CLI's, or `sawit-mcp`'s.
enum Incoming {
    Open(OpenRequest),
    /// Boxed: a request, with the agent that sent it, is much larger than an open request.
    Control(Box<app_control::Request>),
}

/// How long a browser op may take before `sawit-mcp` hears it timed out.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);

/// The answers [`answer`] handed to the blocking pool that may still be writing: quitting waits
/// for them (within GPUI's shutdown timeout), so a large answer an agent is reading when the user
/// quits or restarts to update still reaches it whole, as when the UI thread wrote it.
#[derive(Default)]
struct Writing(Vec<oneshot::Receiver<()>>);

impl Global for Writing {}

/// Listens on the CLI socket; requests are handled on the UI thread.
pub fn serve(cx: &mut App) {
    let path = AppPaths::default_macos().cli_socket();
    if UnixStream::connect(&path).is_ok() {
        eprintln!(
            "sawit: another Sawit is already serving {}; the CLI goes there",
            path.display()
        );
        return;
    }
    // Left behind by a crash.
    let _ = std::fs::remove_file(&path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("sawit: CLI socket {} failed: {e}", path.display());
            return;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    cx.on_app_quit(|cx| {
        let writing = std::mem::take(&mut cx.default_global::<Writing>().0);
        async move {
            futures::future::join_all(writing).await;
        }
    })
    .detach();
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<(Incoming, UnixStream)>();
    let accept = move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut bytes = Vec::new();
            // Canvas scenes and images can be large.
            if (&stream).take(64 << 20).read_to_end(&mut bytes).is_err() {
                continue;
            }
            let incoming = match app_control::decode(&bytes) {
                Some(Ok(request)) => Incoming::Control(Box::new(request)),
                Some(Err(e)) => {
                    reply(&mut stream, &reply_error(&e));
                    continue;
                }
                None => match decode(&bytes) {
                    Some(request) => Incoming::Open(request),
                    None => {
                        reply(&mut stream, &format!("{REPLY_ERROR}unrecognized request"));
                        continue;
                    }
                },
            };
            if tx.unbounded_send((incoming, stream)).is_err() {
                return;
            }
        }
    };
    if let Err(e) = std::thread::Builder::new()
        .name("sawit-cli".into())
        .spawn(accept)
    {
        eprintln!("sawit: CLI thread failed: {e}");
        return;
    }
    cx.spawn(async move |cx| {
        while let Some((incoming, stream)) = rx.next().await {
            cx.update(|cx| match incoming {
                Incoming::Open(request) => handle(request, Some(stream), cx),
                Incoming::Control(request) => handle_control(*request, stream, cx),
            });
        }
    })
    .detach();
}

pub(super) fn reply(stream: &mut UnixStream, line: &str) {
    let _ = writeln!(stream, "{line}");
}

/// Runs a `sawit-mcp` command and answers on `stream`: right away, or for
/// browser ops once the page replies (or [`CONTROL_TIMEOUT`] passes).
fn handle_control(request: app_control::Request, stream: UnixStream, cx: &mut App) {
    match control(request, cx) {
        Ok(Answer::Now(value)) => answer(stream, Ok(value), cx),
        Err(e) => answer(stream, Err(e), cx),
        Ok(Answer::Later(rx)) => {
            let timer = cx.background_executor().timer(CONTROL_TIMEOUT);
            cx.spawn(async move |cx| {
                let result = match futures::future::select(rx, timer).await {
                    futures::future::Either::Left((Ok(result), _)) => result,
                    futures::future::Either::Left((Err(_), _)) => {
                        Err("The browser or canvas closed before answering.".to_string())
                    }
                    futures::future::Either::Right(_) => Err(
                        "The browser page didn't answer in time. Is the Sawit window visible?"
                            .to_string(),
                    ),
                };
                cx.update(|cx| answer(stream, result, cx));
            })
            .detach();
        }
    }
}

/// Answers a `sawit-mcp` command on `stream`, off the UI thread: the line is made and written on
/// the runtime's blocking pool. `sawit-mcp` reads it through a socket that holds 8 KiB at a time,
/// so a large answer (a screenshot, a canvas's scene) held the UI thread until it had read it all.
pub(super) fn answer(
    mut stream: UnixStream,
    result: Result<serde_json::Value, String>,
    cx: &mut App,
) {
    let written = rt::spawn_blocking(cx, move || {
        let line = match result {
            Ok(value) => reply_ok(&value),
            Err(e) => reply_error(&e),
        };
        reply(&mut stream, &line);
    });
    let writing = &mut cx.default_global::<Writing>().0;
    writing.retain_mut(|rx| matches!(rx.try_recv(), Ok(None)));
    writing.push(written);
}

fn control(request: app_control::Request, cx: &mut App) -> Result<Answer, String> {
    match request.command {
        ControlCommand::State => {
            let front = frontmost(cx).map(|(h, _)| h);
            let windows: Vec<serde_json::Value> = workspaces(cx)
                .into_iter()
                .enumerate()
                .map(|(ix, (handle, workspace))| {
                    let mut state = workspace.read(cx).describe(cx);
                    state["window"] = ix.into();
                    state["frontmost"] = (Some(handle) == front).into();
                    state
                })
                .collect();
            // The GitHub sign-in is the app's, not a window's: whether there is one, and whose.
            // Never the token. Agents can't sign in: the user does, in the app.
            let github = crate::git::account::global(cx).read(cx).describe();
            // The agents at work on any window's canvas or browser, each surface once.
            let agents: Vec<serde_json::Value> = (windows.iter())
                .flat_map(|w| w["agents"].as_array().cloned().unwrap_or_default())
                .collect();
            return Ok(Answer::Now(serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "github": github,
                "windows": windows,
                "agents": agents,
            })));
        }
        ControlCommand::Reload { store } => {
            for (_, workspace) in workspaces(cx) {
                workspace.update(cx, |w, cx| w.reload(store, cx));
            }
            return Ok(Answer::Now(serde_json::json!({ "reloaded": store })));
        }
        // The keymap is the app's, not a window's: no window needs to be open or found.
        ControlCommand::Keymap { op } => {
            return crate::keymap::control::run(op, cx).map(Answer::Now);
        }
        ControlCommand::Open {
            path,
            line,
            column,
            new_window,
        } => {
            if !path.is_absolute() {
                return Err(format!("{} isn't an absolute path.", path.display()));
            }
            // Through the links on its way, as the CLI resolves its own paths and the projects'
            // folders are: `/tmp/p` is `/private/tmp/p` on macOS. Else a file of a project is
            // routed to no project's window, and a project's canvas file opens a second canvas of
            // it (CANVAS-UI-4). The answer names what was opened, which later calls name too.
            let opened = sawit_core::files::resolve(&path);
            let request = OpenRequest {
                targets: vec![Target {
                    path: opened.clone(),
                    line,
                    column,
                }],
                new_window,
                ..OpenRequest::default()
            };
            handle(request, None, cx);
            return Ok(Answer::Now(serde_json::json!({ "opened": opened })));
        }
        _ => {}
    }
    let spaces = workspaces(cx);
    let roots: Vec<(usize, Vec<PathBuf>)> = spaces
        .iter()
        .enumerate()
        .map(|(ix, (_, w))| (ix, w.read(cx).roots(cx)))
        .collect();
    let holding =
        |path: &Path| window_naming(&roots, path).map(|(ix, named)| (spaces[ix].clone(), named));
    // An op on a canvas file goes to the window that shows that file's canvas, else to the one
    // whose project holds the file, as `sawit <file>` opens it there: a second canvas of the file
    // in another window would save over the first's edits.
    let file_window = match &request.command {
        ControlCommand::Canvas {
            file: Some(file), ..
        } => {
            let shows = |(_, w): &(AnyWindowHandle, Entity<Workspace>)| {
                w.read(cx).shows_canvas_file(file, cx)
            };
            let showing = spaces.iter().position(shows);
            canvas_file_window(showing, &roots, file).map(|ix| spaces[ix].clone())
        }
        _ => None,
    };
    let target = (file_window, &request.project, &request.agent_folder);
    let ((handle, workspace), project, own) = match target {
        (Some(window), ..) => (window, None, None),
        (None, Some(project), _) => {
            let (window, named) = holding(project).ok_or_else(|| {
                format!(
                    "No Sawit window has {} open. Open it with app_open first.",
                    project.display()
                )
            })?;
            (window, Some(named), None)
        }
        // No project named: the window the agent's own folder is open in,
        // else the frontmost one. The folder goes along, as that window
        // names it: the agent's browser and canvas are its project's.
        (None, None, folder) => {
            let found = folder.as_deref().and_then(holding);
            let own = found.as_ref().map(|(_, named)| named.clone());
            let window = found
                .map(|(window, _)| window)
                .or_else(|| frontmost(cx))
                .or_else(|| open_window(None, cx))
                .ok_or("Unable to open a Sawit window.")?;
            (window, None, own)
        }
    };
    // Commands act on the project asked for: make it the active one. Not a canvas op's: agents'
    // canvas ops leave the user's project, tab and view as they are, and reach the canvas of the
    // project asked for as of the folder the agent works in.
    let canvas = matches!(request.command, ControlCommand::Canvas { .. });
    let (activate, own) = named_project(project, own, canvas);
    if let Some(project) = &activate {
        workspace.update(cx, |w, cx| w.activate_project_of(project, cx));
    }
    let focus = request.command == ControlCommand::Focus;
    let command = request.command;
    let agent = request.agent;
    let answer = handle
        .update(cx, |_, window, cx| {
            if focus {
                window.activate_window();
            }
            workspace.update(cx, |w, cx| {
                w.control(command, own.as_deref(), agent.as_ref(), window, cx)
            })
        })
        .map_err(|_| "The Sawit window closed.".to_string())??;
    if focus {
        cx.activate(true);
    }
    Ok(answer)
}

/// For a command that names the project `named` (as its window names it), sent by an agent working
/// in `own`: the project to make the active one, and the folder the command is for. A canvas op's
/// named project is its folder, and the user's active project stays; any other command's named
/// project becomes the active one.
pub(super) fn named_project(
    named: Option<PathBuf>,
    own: Option<PathBuf>,
    canvas: bool,
) -> (Option<PathBuf>, Option<PathBuf>) {
    match (named, canvas) {
        (Some(named), true) => (None, Some(named)),
        (named, _) => (named, own),
    }
}

/// The window an agent's op on the canvas file `file` goes to, of `windows` and each one's
/// project folders: `showing`, the one with a canvas tab that shows the file, else the one whose
/// project holds the file ([`window_naming`]), as `sawit <file>` opens it there. `None` leaves
/// the choice to the project the op names, or the agent's folder.
pub(super) fn canvas_file_window<H: Clone>(
    showing: Option<H>,
    windows: &[(H, Vec<PathBuf>)],
    file: &Path,
) -> Option<H> {
    showing.or_else(|| window_naming(windows, file).map(|(window, _)| window))
}

/// The window whose project holds the folder an agent named, `path`, and `path` as that window's
/// projects name it. The folder and the projects' folders are compared as they are on disk:
/// through the links on their way, in the letter case they have there and without `..`
/// ([`sawit_core::files::resolve`]). So `/tmp/p`, `/private/tmp/P` and `/private/tmp/q/../p` are
/// the project at `/private/tmp/p`, as they are for `app_open` and the CLI (MCP-2, MCP-3), and
/// `/private/tmp/p/../q` is not. The deepest project wins, the last when two tie, as in
/// [`window_holding`]. A relative path is compared as written: the app's folder isn't the agent's.
pub(super) fn window_naming<H: Clone>(
    windows: &[(H, Vec<PathBuf>)],
    path: &Path,
) -> Option<(H, PathBuf)> {
    if !path.is_absolute() {
        return window_holding(windows, path).map(|window| (window, path.to_path_buf()));
    }
    let resolved = sawit_core::files::resolve(path);
    let mut found: Option<(usize, &H, PathBuf)> = None;
    for (window, roots) in windows {
        for root in roots {
            let real = sawit_core::files::resolve(root);
            let Ok(rest) = resolved.strip_prefix(&real) else {
                continue;
            };
            let depth = real.components().count();
            if found.as_ref().is_none_or(|(deepest, ..)| depth >= *deepest) {
                let named = match rest.as_os_str().is_empty() {
                    true => root.clone(),
                    false => root.join(rest),
                };
                found = Some((depth, window, named));
            }
        }
    }
    found.map(|(_, window, named)| (window.clone(), named))
}

/// The window whose project holds `path`, from each window's project
/// folders: the one with the deepest such project (the last when two tie).
pub(super) fn window_holding<H: Clone>(windows: &[(H, Vec<PathBuf>)], path: &Path) -> Option<H> {
    windows
        .iter()
        .filter_map(|(window, roots)| {
            let root = crate::tabs::project_of(roots, path)?;
            Some((root.components().count(), window))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, window)| window.clone())
}
