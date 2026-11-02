//! The command line, which runs before GPUI starts: `sawit …` hands its paths to the running app
//! over the socket, starting the app first when it isn't running, or runs a subcommand and exits.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sawit_core::cli::{
    Invocation, OpenRequest, REPLY_DONE, REPLY_ERROR, REPLY_OK, Target, USAGE, encode, parse_args,
};
use sawit_core::instances::AppPaths;
use sawit_core::kebun;

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
        Ok(Invocation::Cabut(args)) => {
            std::process::exit(kebun::block_on(kebun::cabut(args)));
        }
        Ok(Invocation::Nyalakan(args)) => {
            std::process::exit(kebun::block_on(kebun::nyalakan(args, &paths)));
        }
        Ok(Invocation::Bibit(args)) => {
            std::process::exit(kebun::block_on(kebun::bibit(args, &paths)));
        }
        // The page goes to the running app, started first as `send` starts it.
        Ok(Invocation::Browse(target)) => {
            let start = || start_app(&paths.cli_socket()).map(drop);
            std::process::exit(kebun::browse(target, &paths, &cwd, start));
        }
        // The page goes to the running app, started first as `send` starts it.
        Ok(Invocation::Semai(args)) => {
            let start = || start_app(&paths.cli_socket()).map(drop);
            std::process::exit(sawit_core::semai::semai(*args, &paths, &cwd, start));
        }
        // A client connecting to GitHub's MCP server asks for the sign-in: nothing else runs.
        Ok(Invocation::GitHub(args)) => {
            std::process::exit(sawit_core::github_token::run(args));
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
        Err(_) => start_app(&socket)?,
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

/// Starts the app ([`launch_app`]) and waits for it to listen on `socket`: the connection, or why
/// there is none.
fn start_app(socket: &Path) -> Result<UnixStream, String> {
    launch_app().map_err(|e| format!("unable to start Sawit: {e}"))?;
    connect_when_ready(socket, Duration::from_secs(20)).ok_or_else(|| {
        "Sawit isn't responding. Open the app from Applications, then try again.".to_string()
    })
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
