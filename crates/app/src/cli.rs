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
use sawit_core::cli::{
    CLI_LINK, Invocation, OpenRequest, REPLY_DONE, REPLY_ERROR, REPLY_OK, Target, USAGE, decode,
    encode, install_link, install_link_as_admin, parse_args,
};
use sawit_core::cli::{LinkState, link_state};
use sawit_core::instances::AppPaths;
use sawit_core::probe::find_in_path;

use crate::rt;
use crate::workspace::Workspace;

actions!(sawit, [InstallCli, Quit]);

// ---------------------------------------------------------------------------
// The command line (runs before GPUI starts)
// ---------------------------------------------------------------------------

/// Handles `sawit …`. Returns the request to open when this process should
/// become the app (launched by Finder/Dock/`open`, or `--foreground`);
/// otherwise talks to the app and exits.
pub fn command_line() -> OpenRequest {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let (mut request, stdin, foreground) = match parse_args(args, &cwd) {
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
                eprintln!("sawit: failed to read stdin: {e}");
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
            launch_app().map_err(|e| format!("failed to start Sawit: {e}"))?;
            connect_when_ready(&socket, Duration::from_secs(20))
                .ok_or("Sawit is not responding; try opening the app directly")?
        }
    };
    stream
        .write_all(&encode(request))
        .and_then(|()| stream.shutdown(std::net::Shutdown::Write))
        .map_err(|e| format!("failed to send to Sawit: {e}"))?;
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
        Err("Sawit closed the connection".into())
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
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    cx.set_menus([Menu::new("Sawit").items([
        MenuItem::action("Install ‘sawit’ Command in PATH…", InstallCli),
        MenuItem::separator(),
        MenuItem::action("Quit Sawit", Quit),
    ])]);
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
                workspace.open_project(project, cx);
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

fn frontmost(cx: &App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    let spaces = workspaces(cx);
    let stack = cx.window_stack().unwrap_or_default();
    stack
        .iter()
        .find_map(|h| spaces.iter().find(|(w, _)| w == h).cloned())
        .or_else(|| spaces.last().cloned())
}

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
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<(OpenRequest, UnixStream)>();
    let accept = move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut bytes = Vec::new();
            if (&stream).take(1 << 20).read_to_end(&mut bytes).is_err() {
                continue;
            }
            match decode(&bytes) {
                Some(request) => {
                    if tx.unbounded_send((request, stream)).is_err() {
                        return;
                    }
                }
                None => reply(&mut stream, &format!("{REPLY_ERROR}unrecognized request")),
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
        while let Some((request, stream)) = rx.next().await {
            cx.update(|cx| handle(request, Some(stream), cx));
        }
    })
    .detach();
}

fn reply(stream: &mut UnixStream, line: &str) {
    let _ = writeln!(stream, "{line}");
}

/// Opens a request from the CLI (or this process's own command line, with
/// no stream). Folders behave as in Zed: an already-open project gets
/// focus, otherwise it fills an empty window or opens a new one. Files go
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
                &format!("{REPLY_ERROR}couldn't open a Sawit window"),
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

fn open_folder(
    dir: PathBuf,
    new_window: bool,
    reuse: bool,
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    if !new_window {
        let spaces = workspaces(cx);
        if let Some(open) = spaces
            .iter()
            .find(|(_, w)| w.read(cx).root(cx).as_ref() == Some(&dir))
        {
            return Some(open.clone());
        }
        let target = match reuse {
            true => frontmost(cx),
            false => spaces
                .into_iter()
                .find(|(_, w)| w.read(cx).root(cx).is_none()),
        };
        if let Some((handle, workspace)) = target {
            workspace.update(cx, |w, cx| w.open_project(dir, cx));
            return Some((handle, workspace));
        }
    }
    open_window(Some(dir), cx)
}

/// The window whose project holds `path` (the deepest one), else the
/// frontmost window, else a new one.
fn window_for_file(path: &Path, cx: &mut App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    workspaces(cx)
        .into_iter()
        .filter_map(|(h, w)| {
            let root = w.read(cx).root(cx)?;
            path.starts_with(&root)
                .then(|| (root.components().count(), (h, w)))
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
                        detail.push_str("\n\n/usr/local/bin isn't in your shell's PATH yet; add it in ~/.zshrc.");
                    }
                    (PromptLevel::Info, "‘sawit’ command installed", detail)
                }
                Err(e) => (PromptLevel::Critical, "Couldn't install the ‘sawit’ command", e),
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
