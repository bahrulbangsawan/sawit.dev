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
    cx.on_action(|_: &Quit, cx| cx.quit());
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
    cx.set_menus([
        Menu::new("Sawit").items([
            MenuItem::action("Install ‘sawit’ Command in PATH…", InstallCli),
            MenuItem::separator(),
            MenuItem::action("Quit Sawit", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Window", NewWindow),
            MenuItem::action("Open Folder…", OpenFolder),
        ]),
    ]);
    cx.on_window_closed(|cx, _| check_waiters(cx)).detach();
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

/// Dock icon clicked with every window closed.
pub fn reopen(cx: &mut App) {
    if workspaces(cx).is_empty() {
        open_window(None, cx);
    }
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

/// How long a canvas op may take before `sawit-mcp` hears it timed out.
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
            // Canvas scenes from `sawit-mcp` can be large.
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
/// canvas ops once the page replies (or [`CONTROL_TIMEOUT`] passes).
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
                        reply_error("The canvas closed before answering.")
                    }
                    futures::future::Either::Right(_) => reply_error(
                        "The canvas didn't answer in time. Is the Sawit window visible?",
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
            let request = OpenRequest {
                targets: vec![Target {
                    path: path.clone(),
                    line,
                    column,
                }],
                new_window,
                ..OpenRequest::default()
            };
            handle(request, None, cx);
            return Ok(Answer::Now(serde_json::json!({ "opened": path })));
        }
        _ => {}
    }
    let (handle, workspace) = match &request.project {
        Some(project) => workspaces(cx)
            .into_iter()
            .filter_map(|(h, w)| {
                let root = crate::tabs::project_of(&w.read(cx).roots(cx), project)?;
                Some((root.components().count(), (h, w)))
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, found)| found)
            .ok_or_else(|| {
                format!(
                    "No Sawit window has {} open. Open it with app_open first.",
                    project.display()
                )
            })?,
        None => frontmost(cx)
            .or_else(|| open_window(None, cx))
            .ok_or("Unable to open a Sawit window.")?,
    };
    // Commands act on the project asked for: make it the active one.
    if let Some(project) = &request.project {
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

/// The window that has `dir` open as one of its projects.
fn window_with_project(dir: &Path, cx: &App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    workspaces(cx)
        .into_iter()
        .find(|(_, w)| w.read(cx).roots(cx).iter().any(|r| r == dir))
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
        let waiting = w.files.iter().any(|f| open.contains(f));
        if !waiting {
            reply(&mut w.stream, REPLY_DONE);
        }
        waiting
    });
}

/// Links `/usr/local/bin/sawit` to this app (menu and title bar), asking
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
    use super::bundle_of;
    use std::path::Path;

    #[test]
    fn bundle_detection() {
        assert_eq!(
            bundle_of(Path::new("/Applications/Sawit.app/Contents/MacOS/sawit")),
            Some(Path::new("/Applications/Sawit.app"))
        );
        assert_eq!(bundle_of(Path::new("/repo/target/release/sawit")), None);
    }
}
