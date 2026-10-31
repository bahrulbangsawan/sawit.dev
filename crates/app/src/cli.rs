//! The `sawit` command, like `zed`. From a terminal it hands its paths to
//! the running app over a Unix socket (launching the app first when needed)
//! and returns, or blocks with `--wait`. Inside the app, the socket server
//! routes each request to the right window, opening new ones as needed.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use sawit_core::app_control::{self, Command as ControlCommand, reply_error, reply_ok};
use sawit_core::cli::{
    CLI_LINK, Invocation, OpenRequest, REPLY_DONE, REPLY_ERROR, REPLY_OK, Target, USAGE, decode,
    encode, install_link, install_link_as_admin, parse_args,
};
use sawit_core::cli::{LinkState, link_state};
use sawit_core::instances::AppPaths;
use sawit_core::kebun;
use sawit_core::probe::find_in_path;

use crate::rt;
use crate::terminal::TerminalPanel;
use crate::workspace::{Answer, Workspace};

actions!(sawit, [InstallCli, Quit, NewWindow, OpenFolder]);

// ---------------------------------------------------------------------------
// The command line (runs before GPUI starts)
// ---------------------------------------------------------------------------

/// Handles `sawit …`. Returns the request to open when this process should
/// become the app (launched by Finder/Dock/`open`, or `--foreground`);
/// otherwise talks to the app and exits.
pub fn command_line() -> OpenRequest {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let paths = AppPaths::default_macos();
    // `sawit kyzn leads "…"`: a database, unless the name turns out to be
    // nothing but a new file to open.
    if let Some(query) = kebun::query_args(&args, &cwd)
        && let Some(code) = kebun::block_on(kebun::query(query, &paths, &cwd))
    {
        std::process::exit(code);
    }
    let (mut request, stdin, foreground) = match parse_args(args, &cwd) {
        Ok(Invocation::List(what)) => {
            std::process::exit(kebun::block_on(kebun::list(what, &paths, &cwd)));
        }
        Ok(Invocation::Bakar(bakar)) => {
            std::process::exit(kebun::block_on(kebun::bakar(bakar, &paths)));
        }
        Ok(Invocation::DbJob(job)) => {
            std::process::exit(kebun::block_on(kebun::db_job(job, &paths, &cwd)));
        }
        Ok(Invocation::Pasang(args)) => {
            std::process::exit(kebun::block_on(kebun::pasang(args)));
        }
        Ok(Invocation::Pupuk(args)) => {
            std::process::exit(kebun::block_on(kebun::pupuk(args)));
        }
        Ok(Invocation::Help) => {
            print!("{USAGE}");
            std::process::exit(0);
        }
        Ok(Invocation::Version) => {
            println!("sawit {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        Ok(Invocation::Open {
            request,
            stdin,
            foreground,
        }) => (request, stdin, foreground),
        Err(e) => {
            eprintln!("sawit: {e}\nSee `sawit --help`.");
            std::process::exit(2);
        }
    };
    if stdin {
        match stdin_to_file() {
            Ok(path) => request.targets.push(Target {
                path,
                line: None,
                column: None,
            }),
            Err(e) => {
                eprintln!("sawit: unable to read stdin: {e}");
                std::process::exit(1);
            }
        }
    }
    // LaunchServices starts apps as children of launchd.
    if foreground || std::os::unix::process::parent_id() == 1 {
        return request;
    }
    std::process::exit(match send(&request) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("sawit: {e}");
            1
        }
    });
}

/// Copies stdin into a fresh file under the cache folder (`ls | sawit -`).
fn stdin_to_file() -> std::io::Result<PathBuf> {
    let dir = AppPaths::default_macos().cache_dir().join("stdin");
    std::fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = dir.join(format!("stdin-{stamp}.txt"));
    let mut text = Vec::new();
    std::io::stdin().read_to_end(&mut text)?;
    std::fs::write(&path, text)?;
    Ok(path)
}

/// Sends `request` to the running app (launching it if needed) and waits
/// for its answer: `ok`, or `done` too with `--wait`.
fn send(request: &OpenRequest) -> Result<(), String> {
    let socket = AppPaths::default_macos().cli_socket();
    let mut stream = match UnixStream::connect(&socket) {
        Ok(stream) => stream,
        Err(_) => {
            launch_app().map_err(|e| format!("unable to start Sawit: {e}"))?;
            connect_when_ready(&socket, Duration::from_secs(20))
                .ok_or("Sawit isn't responding. Open the app from Applications, then try again.")?
        }
    };
    stream
        .write_all(&encode(request))
        .and_then(|()| stream.shutdown(std::net::Shutdown::Write))
        .map_err(|e| format!("unable to reach Sawit: {e}"))?;
    let mut handled = false;
    for line in BufReader::new(stream).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if let Some(e) = line.strip_prefix(REPLY_ERROR) {
            return Err(e.to_string());
        }
        if line == REPLY_DONE || (line == REPLY_OK && !request.wait) {
            return Ok(());
        }
        handled |= line == REPLY_OK;
    }
    // Quitting the app while we wait also ends the wait, as in Zed.
    if handled {
        Ok(())
    } else {
        Err("Sawit closed the connection. Try again.".into())
    }
}

/// Starts the app detached from this terminal: through LaunchServices for
/// the .app bundle (Dock icon, activation), else as a background process.
fn launch_app() -> std::io::Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if let Some(bundle) = bundle_of(&exe) {
        let status = Command::new("/usr/bin/open")
            .arg("-a")
            .arg(bundle)
            .status()?;
        return match status.success() {
            true => Ok(()),
            false => Err(std::io::Error::other(format!("`open` failed ({status})"))),
        };
    }
    Command::new(exe)
        .arg("--foreground")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group: closing the terminal doesn't take it down.
        .process_group(0)
        .spawn()
        .map(drop)
}

/// `…/Sawit.app` when `exe` is `…/Sawit.app/Contents/MacOS/sawit`.
pub fn bundle_of(exe: &Path) -> Option<&Path> {
    let bundle = exe.parent()?.parent()?.parent()?;
    let inside = exe.parent()?.ends_with("Contents/MacOS");
    (inside && bundle.extension().is_some_and(|e| e == "app")).then_some(bundle)
}

fn connect_when_ready(socket: &Path, timeout: Duration) -> Option<UnixStream> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(stream) = UnixStream::connect(socket) {
            return Some(stream);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

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

/// Whether `sawit` in the terminal reaches this app; the title bar's
/// "Install CLI" button shows only while it doesn't.
pub struct CliStatus {
    pub installed: bool,
}

impl CliStatus {
    fn detect() -> bool {
        let Ok(exe) = std::env::current_exe().and_then(|e| e.canonicalize()) else {
            return false;
        };
        link_state(Path::new(CLI_LINK), &exe) == LinkState::Installed
            || find_in_path("sawit").and_then(|p| p.canonicalize().ok()) == Some(exe)
    }
}

struct GlobalCliStatus(Entity<CliStatus>);

impl Global for GlobalCliStatus {}

pub fn cli_status(cx: &App) -> Entity<CliStatus> {
    cx.global::<GlobalCliStatus>().0.clone()
}

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
    cx.set_menus(menus());
}

/// The app's menus. The Edit menu's items are the system's (`cut:`, `copy:`, `paste:`,
/// `selectAll:`), sent to the view with AppKit's keyboard: a web page's clipboard keys reach it
/// that way, which no binding of Sawit's takes there. Elsewhere Sawit's own bindings of the same
/// keys run first, and a click on an item runs gpui-kit's text editing action in the field with
/// the keyboard.
fn menus() -> Vec<Menu> {
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
            MenuItem::action("Open Folder…", OpenFolder),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Cut", text::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", text::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", text::Paste, OsAction::Paste),
            MenuItem::os_action("Select All", text::SelectAll, OsAction::SelectAll),
        ]),
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

/// A request from the socket: the CLI's, or `sawit-mcp`'s.
enum Incoming {
    Open(OpenRequest),
    Control(app_control::Request),
}

/// How long a browser op may take before `sawit-mcp` hears it timed out.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);

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
                Some(Ok(request)) => Incoming::Control(request),
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
                Incoming::Control(request) => handle_control(request, stream, cx),
            });
        }
    })
    .detach();
}

fn reply(stream: &mut UnixStream, line: &str) {
    let _ = writeln!(stream, "{line}");
}

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

/// Runs a `sawit-mcp` command and answers on `stream`: right away, or for
/// browser ops once the page replies (or [`CONTROL_TIMEOUT`] passes).
fn handle_control(request: app_control::Request, mut stream: UnixStream, cx: &mut App) {
    match control(request, cx) {
        Ok(Answer::Now(value)) => reply(&mut stream, &reply_ok(&value)),
        Err(e) => reply(&mut stream, &reply_error(&e)),
        Ok(Answer::Later(rx)) => {
            let timer = cx.background_executor().timer(CONTROL_TIMEOUT);
            cx.spawn(async move |_| {
                let line = match futures::future::select(rx, timer).await {
                    futures::future::Either::Left((Ok(Ok(value)), _)) => reply_ok(&value),
                    futures::future::Either::Left((Ok(Err(e)), _)) => reply_error(&e),
                    futures::future::Either::Left((Err(_), _)) => {
                        reply_error("The browser closed before answering.")
                    }
                    futures::future::Either::Right(_) => reply_error(
                        "The browser page didn't answer in time. Is the Sawit window visible?",
                    ),
                };
                reply(&mut stream, &line);
            })
            .detach();
        }
    }
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
            return Ok(Answer::Now(serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "windows": windows,
            })));
        }
        ControlCommand::Reload { store } => {
            for (_, workspace) in workspaces(cx) {
                workspace.update(cx, |w, cx| w.reload(store, cx));
            }
            return Ok(Answer::Now(serde_json::json!({ "reloaded": store })));
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
    let ((handle, workspace), project) = match (&request.project, &request.agent_folder) {
        (Some(project), _) => {
            let (window, named) = holding(project).ok_or_else(|| {
                format!(
                    "No Sawit window has {} open. Open it with app_open first.",
                    project.display()
                )
            })?;
            (window, Some(named))
        }
        // No project named: the window the agent's own folder is open in,
        // else the frontmost one.
        (None, folder) => {
            let window = folder
                .as_deref()
                .and_then(holding)
                .map(|(window, _)| window)
                .or_else(|| frontmost(cx))
                .or_else(|| open_window(None, cx))
                .ok_or("Unable to open a Sawit window.")?;
            (window, None)
        }
    };
    // Commands act on the project asked for: make it the active one.
    if let Some(project) = &project {
        workspace.update(cx, |w, cx| w.activate_project_of(project, cx));
    }
    let focus = request.command == ControlCommand::Focus;
    let command = request.command;
    let answer = handle
        .update(cx, |_, window, cx| {
            if focus {
                window.activate_window();
            }
            workspace.update(cx, |w, cx| w.control(command, window, cx))
        })
        .map_err(|_| "The Sawit window closed.".to_string())??;
    if focus {
        cx.activate(true);
    }
    Ok(answer)
}

/// The window whose project holds the folder an agent named, `path`, and `path` as that window's
/// projects name it. The folder and the projects' folders are compared as they are on disk:
/// through the links on their way, in the letter case they have there and without `..`
/// ([`sawit_core::files::resolve`]). So `/tmp/p`, `/private/tmp/P` and `/private/tmp/q/../p` are
/// the project at `/private/tmp/p`, as they are for `app_open` and the CLI (MCP-2, MCP-3), and
/// `/private/tmp/p/../q` is not. The deepest project wins, the last when two tie, as in
/// [`window_holding`]. A relative path is compared as written: the app's folder isn't the agent's.
fn window_naming<H: Clone>(windows: &[(H, Vec<PathBuf>)], path: &Path) -> Option<(H, PathBuf)> {
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
fn window_holding<H: Clone>(windows: &[(H, Vec<PathBuf>)], path: &Path) -> Option<H> {
    windows
        .iter()
        .filter_map(|(window, roots)| {
            let root = crate::tabs::project_of(roots, path)?;
            Some((root.components().count(), window))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, window)| window.clone())
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
fn window_with_root<H: Clone>(windows: &[(H, Vec<PathBuf>)], dir: &Path) -> Option<H> {
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

/// Opens a folder picked with a project's Files tab (its Open Project Folder button): shows it
/// in the window that has it already, as ⌘O does, else puts it in the place of that project's
/// folder in `workspace`, as `sawit -r` does ([`Workspace::replace_project`]), which shows a
/// folder the window has in another project and ungroups the tabs of the folder it replaces.
/// Must not run while a workspace is being updated (it reads them all).
pub fn open_folder_from_files(
    dir: PathBuf,
    window: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
    cx: &mut App,
) {
    let elsewhere = window_with_project(&dir, cx).filter(|(handle, _)| *handle != window);
    let target = match (elsewhere, workspace.upgrade()) {
        (Some(found), _) => add_project(found, dir, cx),
        (None, Some(workspace)) => window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |w, cx| w.replace_project(dir, window, cx))
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
fn window_for_file(path: &Path, cx: &mut App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
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
/// show, itself or, for an `.excalidraw` file, as the `.figcraft` file beside it that its canvas
/// is bound to and its tab shows (CANVAS-UI-7).
fn still_open(files: &[PathBuf], open: &HashSet<PathBuf>) -> bool {
    files
        .iter()
        .any(|file| open.contains(file) || open.contains(&sawit_board::store::bound_file(file)))
}

/// Links `/usr/local/bin/sawit` to this app (menu and status bar), asking
/// for the admin password when the folder is root-owned.
pub fn install_cli(cx: &mut App) {
    let Ok(exe) = std::env::current_exe().and_then(|e| e.canonicalize()) else {
        return;
    };
    let link = PathBuf::from(CLI_LINK);
    let rx = rt::spawn(cx, async move {
        match install_link(&link, &exe) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                install_link_as_admin(&link, &exe)
                    .await
                    .map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    });
    cx.spawn(async move |cx| {
        let Ok(result) = rx.await else { return };
        cx.update(|cx| {
            cli_status(cx).update(cx, |s, cx| {
                s.installed = CliStatus::detect();
                cx.notify();
            });
            let (level, title, detail) = match result {
                Ok(()) => {
                    let in_path = std::env::var_os("PATH").is_some_and(|p| {
                        std::env::split_paths(&p).any(|d| d == Path::new("/usr/local/bin"))
                    });
                    let mut detail = format!(
                        "{CLI_LINK} is installed. From a terminal:\n  sawit .             open this folder\n  sawit file.rs:42    open a file at line 42\n  sawit --help        all options"
                    );
                    if !in_path {
                        detail.push_str("\n\n/usr/local/bin isn't in your shell's PATH. Add this line to ~/.zshrc:\n  export PATH=\"/usr/local/bin:$PATH\"");
                    }
                    (PromptLevel::Info, "‘sawit’ command installed", detail)
                }
                Err(e) => (PromptLevel::Critical, "Unable to install the ‘sawit’ command", e),
            };
            if let Some((handle, _)) = frontmost(cx) {
                let _ = handle.update(cx, |_, window, cx| {
                    window.prompt(level, title, Some(&detail), &["OK"], cx)
                });
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::{bundle_of, still_open, window_holding, window_naming, window_with_root};
    use gpui_kit::{KeyContext, MenuItem, OperatingSystem, OsAction, TestAppContext};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    /// CANVAS-UI-7: `sawit --wait` on a canvas file waits while its canvas is open: an
    /// `.excalidraw` file's canvas is bound to the `.figcraft` file beside it, which its tab
    /// shows, and a project's canvas file is shown by the project's canvas tab. It answered "done"
    /// at the first change of the window.
    #[test]
    fn a_wait_on_a_canvas_file_lasts_while_its_canvas_is_open() {
        let open: HashSet<PathBuf> = [
            PathBuf::from("/p/sketch.figcraft"),
            PathBuf::from("/p/.nyawit/canvas.figcraft"),
            PathBuf::from("/p/notes.md"),
        ]
        .into();
        let waits = |file: &str| still_open(&[PathBuf::from(file)], &open);
        assert!(waits("/p/sketch.excalidraw"));
        assert!(waits("/p/sketch.figcraft"));
        assert!(waits("/p/.nyawit/canvas.figcraft"));
        assert!(waits("/p/notes.md"));
        assert!(!waits("/p/other.excalidraw"));
        assert!(!waits("/p/notes.txt"));
        // Waiting on several, one of them open is enough.
        let files = [
            PathBuf::from("/p/gone.md"),
            PathBuf::from("/p/sketch.excalidraw"),
        ];
        assert!(still_open(&files, &open));
    }

    /// Control commands go to the window whose project holds the folder asked about (a project
    /// named, or the agent's own folder): the deepest project wins.
    #[test]
    fn a_folder_goes_to_the_window_with_its_deepest_project() {
        let windows = vec![
            ("front", vec![PathBuf::from("/code/shop")]),
            (
                "back",
                vec![PathBuf::from("/code"), PathBuf::from("/code/api")],
            ),
        ];
        let window = |path: &str| window_holding(&windows, Path::new(path));
        assert_eq!(window("/code/api/src"), Some("back"));
        assert_eq!(window("/code/shop/web"), Some("front"));
        assert_eq!(window("/code/other"), Some("back"));
        assert_eq!(window("/tmp/elsewhere"), None);
    }

    /// SITE-1: ⌘Q, the app's own Quit (as the menu's Quit Sawit sends it), asks to save a file
    /// with unsaved changes first. It quit at once, and the changes were lost.
    #[gpui_kit::test]
    fn the_quit_key_asks_to_save_unsaved_changes(cx: &mut gpui_kit::TestAppContext) {
        use crate::editor::testing::{open_editor, scratch_dir, type_into};
        cx.update(|cx| {
            crate::editor::testing::init(cx);
            super::init(cx);
        });
        let path = scratch_dir("quit-key").join("notes.txt");
        std::fs::write(&path, "on disk\n").unwrap();
        let (window, editor) = open_editor(cx, &path);
        type_into(cx, window, &editor, "typed ");

        cx.simulate_keystrokes(window, "cmd-q");
        cx.run_until_parked();
        let (question, _) = cx.pending_prompt().expect("⌘Q asked nothing");
        assert_eq!(question, "Save changes to notes.txt before quitting?");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
    }

    /// The app's key bindings, bound as `main` binds them, then taken as the keymap's defaults
    /// (with a keymap file of the test's own, which it never writes).
    fn bind_like_the_app(cx: &mut gpui_kit::App, keymap_file: &Path) {
        gpui_kit::init(cx);
        crate::editor::init(cx);
        crate::files::init(cx);
        crate::shell_config::init(cx);
        crate::db::init(cx);
        crate::git::init(cx);
        crate::workspace::init(cx);
        crate::terminal::init(cx);
        super::init(cx);
        crate::keymap::init_at(keymap_file.to_path_buf(), cx);
    }

    /// The Edit menu's Cut, Copy, Paste and Select All carry ⌘X, ⌘C, ⌘V and ⌘A and go to the
    /// view with AppKit's keyboard (`cut:`, `copy:`, `paste:`, `selectAll:`): a web page's
    /// clipboard keys reach it that way, once no binding of Sawit's takes them.
    #[gpui_kit::test]
    fn the_edit_menu_gives_a_web_page_its_clipboard_keys(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("sawit-edit-menu-{}", std::process::id()));
        cx.update(|cx| bind_like_the_app(cx, &dir.join("keymap.json")));
        cx.update(|cx| {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            let menus = super::menus();
            let edit = menus.iter().find(|m| m.name.as_ref() == "Edit");
            let found: Vec<(String, Option<String>, String)> = edit
                .map(|edit| {
                    edit.items
                        .iter()
                        .filter_map(|item| match item {
                            MenuItem::Action {
                                name,
                                action,
                                os_action,
                                ..
                            } => Some((
                                name.to_string(),
                                os_action.map(|a| {
                                    match a {
                                        OsAction::Cut => "Cut",
                                        OsAction::Copy => "Copy",
                                        OsAction::Paste => "Paste",
                                        OsAction::SelectAll => "SelectAll",
                                        OsAction::Undo => "Undo",
                                        OsAction::Redo => "Redo",
                                    }
                                    .to_string()
                                }),
                                keymap
                                    .bindings_for_action(action.as_ref())
                                    .next()
                                    .map(|b| crate::keymap::names::unparse(b.keystrokes()))
                                    .unwrap_or_default(),
                            )),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let expected = [
                ("Cut", "Cut", "cmd-x"),
                ("Copy", "Copy", "cmd-c"),
                ("Paste", "Paste", "cmd-v"),
                ("Select All", "SelectAll", "cmd-a"),
            ]
            .map(|(n, a, k)| (n.to_string(), Some(a.to_string()), k.to_string()));
            assert_eq!(found, expected);
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The keymap editor shows each binding's action and context, and writes them into
    /// keymap.json, where a later rename would break the user's binding: none of them still
    /// calls the Config Files panel by its old name.
    #[gpui_kit::test]
    fn the_keymap_calls_config_files_by_its_name(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("sawit-config-names-{}", std::process::id()));
        cx.update(|cx| bind_like_the_app(cx, &dir.join("keymap.json")));
        cx.update(|cx| {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            let old: Vec<String> = keymap
                .bindings()
                .flat_map(|b| {
                    [
                        b.action().name().to_string(),
                        b.predicate().map(|p| p.to_string()).unwrap_or_default(),
                    ]
                })
                .filter(|name| name.contains("ShellConfig") || name.contains("shell_config"))
                .collect();
            assert!(old.is_empty(), "{old:?}");
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A menu item carries the key of the first binding of its action that GPUI's macOS menu
    /// builder finds in a Workspace > Pane > Editor focus, else of the first binding at all
    /// (`find_or_first`), and AppKit runs the item for that key wherever the item's action is
    /// handled: in Sawit, everywhere the workspace is. So each item's action has no binding, or
    /// one that holds everywhere in a window. A key bound only in a view (the keymap editor's
    /// ⌘E) would otherwise run the item from every other view, a web page's included.
    #[gpui_kit::test]
    fn menu_keys_hold_everywhere_in_a_window(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("sawit-menu-keys-{}", std::process::id()));
        cx.update(|cx| bind_like_the_app(cx, &dir.join("keymap.json")));
        let workspace = {
            let mut workspace = KeyContext::new_with_defaults(OperatingSystem::Mac);
            workspace.add("Workspace");
            let mut pane = KeyContext::new_with_defaults(OperatingSystem::Mac);
            pane.add("Pane");
            let mut editor = KeyContext::new_with_defaults(OperatingSystem::Mac);
            editor.add("Editor");
            pane.extend(&editor);
            workspace.extend(&pane);
            vec![workspace]
        };
        cx.update(|cx| {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            for menu in super::menus() {
                for item in &menu.items {
                    // A system item (the Edit menu's) goes to the view with AppKit's keyboard,
                    // not to a GPUI action, and only when no binding of Sawit's takes its key.
                    let MenuItem::Action {
                        name,
                        action,
                        os_action: None,
                        ..
                    } = item
                    else {
                        continue;
                    };
                    let bindings: Vec<_> = keymap.bindings_for_action(action.as_ref()).collect();
                    let everywhere = bindings
                        .iter()
                        .any(|b| b.predicate().is_none_or(|p| p.eval(&workspace)));
                    assert!(
                        bindings.is_empty() || everywhere,
                        "{name}: {:?}",
                        bindings
                            .iter()
                            .map(|b| (
                                crate::keymap::names::unparse(b.keystrokes()),
                                b.predicate().map(|p| p.to_string())
                            ))
                            .collect::<Vec<_>>()
                    );
                }
            }
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MCP-2, MCP-3: an agent names an open project by another name of its folder: through a
    /// link (`/tmp/p` for the project at `/private/tmp/p` on macOS), in another letter case (a
    /// Mac's volumes ignore it), or with `..` in it. That is the open project, as it is for
    /// `app_open` and the CLI, and the command goes to its window, named as the window names it.
    /// Matched as written, it was "No Sawit window has … open. Open it with app_open first.",
    /// while `app_open` of the same name answered that it had opened it.
    #[test]
    fn a_project_named_another_way_is_the_open_one() {
        let dir = std::env::temp_dir().join(format!("sawit-cli-{}-named", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("shop");
        std::fs::create_dir_all(project.join("web")).unwrap();
        let project = project.canonicalize().unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&project, &link).unwrap();
        let windows = vec![("window", vec![project.clone()])];
        let named = |path: &Path| window_naming(&windows, path);

        assert_eq!(named(&project), Some(("window", project.clone())));
        for other in [
            link.clone(),
            dir.join("SHOP"),
            project.join("web").join(".."),
        ] {
            assert_eq!(
                named(&other),
                Some(("window", project.clone())),
                "{}",
                other.display()
            );
        }
        // A folder in it, through the link, names it as the window does.
        assert_eq!(
            named(&link.join("web")),
            Some(("window", project.join("web")))
        );
        // `..` out of the project is another folder, though the path starts with the project's.
        std::fs::create_dir_all(dir.join("api")).unwrap();
        assert_eq!(named(&project.join("..").join("api")), None);
        // Neither a folder of no open project nor a relative name is taken for one.
        assert_eq!(named(&dir.join("other")), None);
        assert_eq!(named(Path::new("shop")), None);
        // A window that names its project through the link has it named so.
        let windows = vec![("window", vec![link.clone()])];
        assert_eq!(
            window_naming(&windows, &project.join("web")),
            Some(("window", link.join("web")))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A folder opened through a link (the Open dialog keeps a link picked there as the link,
    /// and Recent Projects reopens folders as they were remembered) is the project open at the
    /// folder it leads to, which is how a window holds its projects: through the links, in the
    /// letter case on disk (review-mcp M1). Compared as written, the folder was added to the
    /// window a second time.
    #[test]
    fn a_folder_opened_through_a_link_is_the_project_open_there() {
        let dir = std::env::temp_dir().join(format!("sawit-cli-{}-root", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("shop");
        std::fs::create_dir_all(&project).unwrap();
        let project = project.canonicalize().unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&project, &link).unwrap();
        std::fs::create_dir_all(dir.join("api")).unwrap();
        let windows = vec![
            ("first", vec![dir.join("api").canonicalize().unwrap()]),
            ("second", vec![project.clone()]),
        ];
        assert_eq!(window_with_root(&windows, &project), Some("second"));
        assert_eq!(window_with_root(&windows, &link), Some("second"));
        // In another letter case, on a volume that ignores case (a Mac's, as a rule).
        if dir.join("SHOP").exists() {
            assert_eq!(
                window_with_root(&windows, &dir.join("SHOP")),
                Some("second")
            );
        }
        // A folder inside an open project isn't that project.
        std::fs::create_dir_all(project.join("web")).unwrap();
        assert_eq!(window_with_root(&windows, &link.join("web")), None);
        assert_eq!(window_with_root(&windows, &dir.join("other")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundle_detection() {
        assert_eq!(
            bundle_of(Path::new("/Applications/Sawit.app/Contents/MacOS/sawit")),
            Some(Path::new("/Applications/Sawit.app"))
        );
        assert_eq!(bundle_of(Path::new("/repo/target/release/sawit")), None);
    }
}
