//! The `sawit` command line, like `zed`: argument parsing (`file:line:col`
//! included), the request the CLI sends to the running app over its Unix
//! socket, and installing the `sawit` symlink into `PATH`.

mod link;
mod request;
mod usage;

pub use link::{CLI_LINK, LinkState, install_link, install_link_as_admin, link_state};
pub(crate) use link::{applescript_escape, shell_quote};
pub use request::{REPLY_DONE, REPLY_ERROR, REPLY_OK, decode, encode};
pub use usage::USAGE;

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use crate::kebun::{BakarArgs, CabutArgs, DbJob, ListWhat, NyalakanArgs, PasangArgs, PupukArgs};

#[cfg(test)]
mod tests;

/// A path to open, with an optional 1-based cursor position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// What the CLI asks the running app to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenRequest {
    pub targets: Vec<Target>,
    pub new_window: bool,
    pub reuse: bool,
    pub wait: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    Help,
    Version,
    /// `sawit list …`.
    List(ListWhat),
    /// `sawit bakar …`.
    Bakar(BakarArgs),
    /// `sawit panen|backup|tanam …`.
    DbJob(DbJob),
    /// `sawit pasang …`.
    Pasang(PasangArgs),
    /// `sawit pupuk …`.
    Pupuk(PupukArgs),
    /// `sawit cabut …`.
    Cabut(CabutArgs),
    Open {
        request: OpenRequest,
        /// `-` was given: the CLI turns stdin into a file target.
        stdin: bool,
        /// Run the app in this process instead of handing off to it.
        foreground: bool,
    },
    /// `sawit nyalakan …` (`start`).
    Nyalakan(NyalakanArgs),
    /// `sawit browse [url|port]` (`intip`), and `sawit <url>`.
    Browse(BrowseTarget),
    /// `sawit new …` (`semai`): the New Project page, or its command.
    Semai(Box<crate::semai::SemaiArgs>),
    /// `sawit github token …`: the GitHub sign-in, handed to an MCP client.
    GitHub(crate::github_token::TokenArgs),
}

/// What `sawit browse` opens in Sawit's browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseTarget {
    /// The dev server running in this folder, or in the open project that holds it.
    DevServer,
    /// An address as the browser's address field takes it: a URL, `localhost:5173`, a host; a
    /// port on its own is already `http://localhost:<port>`.
    Address(String),
}

impl BrowseTarget {
    /// `sawit browse`'s arguments: nothing, a port, or one address.
    pub fn parse(args: &[&str]) -> std::result::Result<BrowseTarget, String> {
        match args {
            [] => Ok(BrowseTarget::DevServer),
            [arg] if arg.starts_with('-') => Err(format!(
                "opsi tak dikenal: {arg} (`sawit browse` cuma butuh alamat atau port)"
            )),
            [port] if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
                match port.parse::<u16>() {
                    Ok(port) if port > 0 => {
                        Ok(BrowseTarget::Address(format!("http://localhost:{port}")))
                    }
                    _ => Err(format!("“{port}” bukan nomor port.")),
                }
            }
            [address] if !address.trim().is_empty() => {
                Ok(BrowseTarget::Address(address.trim().to_string()))
            }
            _ => Err("intip satu alamat saja: `sawit browse [url|port]`.".into()),
        }
    }
}

/// Whether `arg` is a web page's address (`http://…`, `https://…`), which `sawit` opens in its
/// browser rather than as a file.
fn web_address(arg: &str) -> bool {
    let lower = arg.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Parses `args` (without the program name); relative paths resolve
/// against `cwd`.
pub fn parse_args(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
) -> std::result::Result<Invocation, String> {
    let args: Vec<OsString> = args.into_iter().collect();
    // `sawit github token …`; `sawit github` alone is still a folder or file. What `--exec` runs
    // may be any path, so its words needn't be UTF-8.
    if args.first().is_some_and(|a| a == "github") && args.get(1).is_some_and(|a| a == "token") {
        return crate::github_token::TokenArgs::parse(&args[1..]).map(Invocation::GitHub);
    }
    let words: Vec<&str> = args.iter().filter_map(|a| a.to_str()).collect();
    match words.first().copied() {
        Some("list" | "ls" | "daftar") if words.len() == args.len() && words.len() <= 2 => {
            return ListWhat::parse(words.get(1).copied()).map(Invocation::List);
        }
        Some("bakar" | "stop") if words.len() == args.len() => {
            return BakarArgs::parse(&words[1..]).map(Invocation::Bakar);
        }
        Some("install" | "pasang") if words.len() == args.len() => {
            return PasangArgs::parse(&words[1..]).map(Invocation::Pasang);
        }
        Some("upgrade" | "pupuk") if words.len() == args.len() => {
            return PupukArgs::parse(&words[1..]).map(Invocation::Pupuk);
        }
        Some("uninstall" | "cabut") if words.len() == args.len() => {
            return CabutArgs::parse(&words[1..]).map(Invocation::Cabut);
        }
        Some("start" | "nyalakan") if words.len() == args.len() => {
            return NyalakanArgs::parse(&words[1..]).map(Invocation::Nyalakan);
        }
        Some("new" | "semai") if words.len() == args.len() => {
            return crate::semai::SemaiArgs::parse(&words[1..])
                .map(|args| Invocation::Semai(Box::new(args)));
        }
        Some("browse" | "intip") if words.len() == args.len() => {
            return BrowseTarget::parse(&words[1..]).map(Invocation::Browse);
        }
        Some(url) if args.len() == 1 && web_address(url) => {
            return Ok(Invocation::Browse(BrowseTarget::Address(url.trim().into())));
        }
        Some(word) if words.len() == args.len() && DbJob::kind(word).is_some() => {
            let kind = DbJob::kind(word).expect("checked");
            return DbJob::parse(kind, &words[1..], cwd).map(Invocation::DbJob);
        }
        _ => {}
    }
    let mut request = OpenRequest::default();
    let (mut stdin, mut foreground, mut options_done) = (false, false, false);
    for arg in args {
        let bytes = arg.as_bytes();
        if options_done || !bytes.starts_with(b"-") {
            // A URL opens in the browser, alone; past `--` it is a path.
            if let Some(url) = arg.to_str().filter(|a| !options_done && web_address(a)) {
                return Err(format!(
                    "{url} is a web address: open it on its own, `sawit {url}` or `sawit browse {url}`"
                ));
            }
            request
                .targets
                .push(parse_target(&arg, cwd, |p| p.exists()));
            continue;
        }
        match bytes {
            b"-" => stdin = true,
            b"--" => options_done = true,
            b"--help" => return Ok(Invocation::Help),
            b"--version" => return Ok(Invocation::Version),
            b"--new" => request.new_window = true,
            b"--reuse" => request.reuse = true,
            b"--wait" => request.wait = true,
            b"--foreground" => foreground = true,
            // Finder adds a process serial number on old macOS versions.
            _ if bytes.starts_with(b"-psn_") => {}
            _ if bytes.starts_with(b"--") => {
                return Err(format!("unknown option: {}", arg.to_string_lossy()));
            }
            // Short flags, also combined (`-nw`).
            _ => {
                for &flag in &bytes[1..] {
                    match flag {
                        b'h' => return Ok(Invocation::Help),
                        b'v' => return Ok(Invocation::Version),
                        b'n' => request.new_window = true,
                        b'r' => request.reuse = true,
                        b'w' => request.wait = true,
                        _ => return Err(format!("unknown option: -{}", flag as char)),
                    }
                }
            }
        }
    }
    if request.new_window && request.reuse {
        return Err("use either --new or --reuse, not both".into());
    }
    if request.wait && !stdin && request.targets.iter().all(|t| t.path.is_dir()) {
        return Err("--wait needs at least one file".into());
    }
    Ok(Invocation::Open {
        request,
        stdin,
        foreground,
    })
}

/// `path`, `path:line` or `path:line:col` (a trailing `:` from grep output
/// is ignored). A path that exists as written is never split.
pub fn parse_target(raw: &OsStr, cwd: &Path, exists: impl Fn(&Path) -> bool) -> Target {
    let plain = |path: &OsStr| absolute(cwd, Path::new(path));
    let whole = plain(raw);
    if exists(&whole) {
        return Target {
            path: whole,
            line: None,
            column: None,
        };
    }
    let Some(text) = raw.to_str() else {
        return Target {
            path: whole,
            line: None,
            column: None,
        };
    };
    let text = text.strip_suffix(':').unwrap_or(text);
    let mut numbers = Vec::new();
    let mut base = text;
    while numbers.len() < 2
        && let Some((head, tail)) = base.rsplit_once(':')
        && let Ok(n) = tail.parse::<u32>()
        && !head.is_empty()
    {
        numbers.insert(0, n);
        base = head;
    }
    Target {
        path: plain(OsStr::new(base)),
        line: numbers.first().copied(),
        column: numbers.get(1).copied(),
    }
}

/// `path` made absolute against `cwd` with `.`/`..` removed, then resolved
/// through symlinks when it exists (so it matches canonical project roots).
pub fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in cwd.join(path).components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    if let Ok(real) = out.canonicalize() {
        return real;
    }
    // A new file: resolve its folder instead.
    match (
        out.parent().and_then(|p| p.canonicalize().ok()),
        out.file_name(),
    ) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => out,
    }
}
