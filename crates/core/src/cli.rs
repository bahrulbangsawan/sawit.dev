//! The `pgdesk` command line, like `zed`: argument parsing (`file:line:col`
//! included), the request the CLI sends to the running app over its Unix
//! socket, and installing the `pgdesk` symlink into `PATH`.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use crate::{CoreError, Result, cmd};

/// Where "Install CLI" puts the command.
pub const CLI_LINK: &str = "/usr/local/bin/pgdesk";

pub const USAGE: &str = "\
pgdesk — open folders and files in PG Desk

Usage:
  pgdesk [options] [path[:line[:column]] ...]

Examples:
  pgdesk .                  open this folder as a project
  pgdesk src/main.rs:42     open a file at line 42
  pgdesk -w COMMIT_EDITMSG  wait until the file's tab is closed (git core.editor)
  ls | pgdesk -             open stdin as a new file

Options:
  -n, --new         always open in a new window
  -r, --reuse       use the frontmost window, replacing its project
  -w, --wait        wait until every opened file is closed
  -                 read stdin into a temporary file and open it
      --foreground  run the app in this terminal (debugging)
  -v, --version     print the version
  -h, --help        print this help
";

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
    Open {
        request: OpenRequest,
        /// `-` was given: the CLI turns stdin into a file target.
        stdin: bool,
        /// Run the app in this process instead of handing off to it.
        foreground: bool,
    },
}

/// Parses `args` (without the program name); relative paths resolve
/// against `cwd`.
pub fn parse_args(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
) -> std::result::Result<Invocation, String> {
    let mut request = OpenRequest::default();
    let (mut stdin, mut foreground, mut options_done) = (false, false, false);
    for arg in args {
        let bytes = arg.as_bytes();
        if options_done || !bytes.starts_with(b"-") {
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
        return Err("--new and --reuse can't be used together".into());
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

const MAGIC: &[u8] = b"pgdesk-open/1";

/// The request as NUL-separated fields: magic, flags, then path, line and
/// column per target. The CLI writes it and closes its sending half.
pub fn encode(request: &OpenRequest) -> Vec<u8> {
    let mut flags = String::new();
    for (on, c) in [
        (request.new_window, 'n'),
        (request.reuse, 'r'),
        (request.wait, 'w'),
    ] {
        if on {
            flags.push(c);
        }
    }
    let mut fields: Vec<Vec<u8>> = vec![MAGIC.to_vec(), flags.into_bytes()];
    let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default().into_bytes();
    for t in &request.targets {
        fields.push(t.path.as_os_str().as_bytes().to_vec());
        fields.push(num(t.line));
        fields.push(num(t.column));
    }
    fields.join(&0u8)
}

pub fn decode(bytes: &[u8]) -> Option<OpenRequest> {
    let mut fields = bytes.split(|&b| b == 0);
    if fields.next()? != MAGIC {
        return None;
    }
    let flags = fields.next()?;
    let rest: Vec<&[u8]> = fields.collect();
    if !rest.len().is_multiple_of(3) {
        return None;
    }
    let num = |f: &[u8]| -> Option<Option<u32>> {
        if f.is_empty() {
            return Some(None);
        }
        std::str::from_utf8(f).ok()?.parse().ok().map(Some)
    };
    let targets = rest
        .chunks(3)
        .map(|t| {
            Some(Target {
                path: PathBuf::from(OsString::from_vec(t[0].to_vec())),
                line: num(t[1])?,
                column: num(t[2])?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(OpenRequest {
        targets,
        new_window: flags.contains(&b'n'),
        reuse: flags.contains(&b'r'),
        wait: flags.contains(&b'w'),
    })
}

/// Replies, one per line: `ok` once handled, then `done` for `--wait` when
/// every waited file is closed, or `error: …`.
pub const REPLY_OK: &str = "ok";
pub const REPLY_DONE: &str = "done";
pub const REPLY_ERROR: &str = "error: ";

/// What sits at the CLI link right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    Missing,
    /// Points at this app.
    Installed,
    /// A symlink to something else (e.g. an older copy of the app).
    Other(PathBuf),
    /// A regular file we must not overwrite.
    Blocked,
}

pub fn link_state(link: &Path, exe: &Path) -> LinkState {
    match std::fs::read_link(link) {
        Ok(target) if same_file(&target, exe) => LinkState::Installed,
        Ok(target) => LinkState::Other(target),
        Err(_) if link.symlink_metadata().is_ok() => LinkState::Blocked,
        Err(_) => LinkState::Missing,
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Points `link` at `exe`, replacing an older symlink but never a regular
/// file. Fails with `PermissionDenied` when the folder isn't writable.
pub fn install_link(link: &Path, exe: &Path) -> std::io::Result<()> {
    if link_state(link, exe) == LinkState::Blocked {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} already exists and is not a symlink", link.display()),
        ));
    }
    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match std::fs::remove_file(link) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    std::os::unix::fs::symlink(exe, link)
}

/// `install_link` through macOS's administrator password prompt, for the
/// root-owned `/usr/local/bin`.
pub async fn install_link_as_admin(link: &Path, exe: &Path) -> Result<()> {
    let dir = link.parent().unwrap_or(Path::new("/"));
    let script = format!(
        "mkdir -p {} && ln -sfn {} {}",
        shell_quote(dir.as_os_str()),
        shell_quote(exe.as_os_str()),
        shell_quote(link.as_os_str()),
    );
    let apple = format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(&script)
    );
    let out = cmd::run(
        "/usr/bin/osascript",
        ["-e", &apple],
        Duration::from_secs(120),
    )
    .await?;
    if out.code == Some(0) {
        return Ok(());
    }
    Err(CoreError::Command {
        program: "osascript".into(),
        code: out.code,
        stderr: out.stderr.trim().to_string(),
    })
}

pub(crate) fn shell_quote(s: &OsStr) -> String {
    format!("'{}'", s.to_string_lossy().replace('\'', r"'\''"))
}

fn applescript_escape(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(raw: &str, existing: &[&str]) -> Target {
        parse_target(OsStr::new(raw), Path::new("/p"), |p| {
            existing.iter().any(|e| p == Path::new(e))
        })
    }

    fn at(path: &str, line: Option<u32>, column: Option<u32>) -> Target {
        Target {
            path: path.into(),
            line,
            column,
        }
    }

    #[test]
    fn targets_with_positions() {
        assert_eq!(target("a.rs", &[]), at("/p/a.rs", None, None));
        assert_eq!(target("a.rs:12", &[]), at("/p/a.rs", Some(12), None));
        assert_eq!(target("a.rs:12:5", &[]), at("/p/a.rs", Some(12), Some(5)));
        assert_eq!(target("a.rs:12:5:", &[]), at("/p/a.rs", Some(12), Some(5)));
        assert_eq!(target("/x/b.sql:3", &[]), at("/x/b.sql", Some(3), None));
        // Only two numbers are taken; the rest is part of the name.
        assert_eq!(target("a:1:2:3", &[]), at("/p/a:1", Some(2), Some(3)));
        assert_eq!(target(":12", &[]), at("/p/:12", None, None));
        assert_eq!(target("a.rs:x", &[]), at("/p/a.rs:x", None, None));
    }

    #[test]
    fn existing_names_with_colons_are_not_split() {
        assert_eq!(
            target("notes:2024", &["/p/notes:2024"]),
            at("/p/notes:2024", None, None)
        );
    }

    #[test]
    fn dot_segments_are_removed() {
        assert_eq!(
            absolute(Path::new("/nope/a/b"), Path::new("../c/./d.rs")),
            PathBuf::from("/nope/a/c/d.rs")
        );
        assert_eq!(
            absolute(Path::new("/nope"), Path::new("/abs/x")),
            PathBuf::from("/abs/x")
        );
    }

    fn parse(args: &[&str]) -> std::result::Result<Invocation, String> {
        parse_args(args.iter().map(OsString::from), Path::new("/nope"))
    }

    #[test]
    fn flags() {
        assert_eq!(parse(&["-h"]), Ok(Invocation::Help));
        assert_eq!(parse(&["--version"]), Ok(Invocation::Version));
        let Ok(Invocation::Open {
            request,
            stdin,
            foreground,
        }) = parse(&["-nw", "a.rs:3", "-", "--foreground", "--", "-b"])
        else {
            panic!()
        };
        assert!(request.new_window && request.wait && !request.reuse);
        assert!(stdin && foreground);
        assert_eq!(
            request.targets,
            vec![at("/nope/a.rs", Some(3), None), at("/nope/-b", None, None)]
        );
        assert!(parse(&["-psn_0_1234"]).is_ok());
    }

    #[test]
    fn bad_flags() {
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["-x"]).is_err());
        assert!(parse(&["-n", "-r"]).is_err());
        assert!(parse(&["-w"]).is_err());
        assert!(parse(&["-w", "/"]).is_err());
        assert!(parse(&["-w", "-"]).is_ok());
    }

    #[test]
    fn request_round_trip() {
        let request = OpenRequest {
            targets: vec![
                at("/a b/c.rs", Some(4), Some(2)),
                at("/d", None, None),
                at("/e:f", Some(1), None),
            ],
            new_window: true,
            reuse: false,
            wait: true,
        };
        assert_eq!(decode(&encode(&request)), Some(request));
        assert_eq!(
            decode(&encode(&OpenRequest::default())),
            Some(OpenRequest::default())
        );
        assert_eq!(decode(b"something else"), None);
        assert_eq!(decode(b"pgdesk-open/1\0\0/a\0x\0"), None);
    }

    #[test]
    fn link_install_and_state() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("pgdesk-app");
        std::fs::write(&exe, "").unwrap();
        let link = dir.path().join("bin/pgdesk");
        assert_eq!(link_state(&link, &exe), LinkState::Missing);
        install_link(&link, &exe).unwrap();
        assert_eq!(link_state(&link, &exe), LinkState::Installed);
        // Re-pointing an older symlink works.
        let other = dir.path().join("old");
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&other, &link).unwrap();
        assert_eq!(link_state(&link, &exe), LinkState::Other(other));
        install_link(&link, &exe).unwrap();
        assert_eq!(link_state(&link, &exe), LinkState::Installed);
        // A real file is left alone.
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, "mine").unwrap();
        assert_eq!(link_state(&link, &exe), LinkState::Blocked);
        assert!(install_link(&link, &exe).is_err());
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "mine");
    }

    #[test]
    fn admin_script_quoting() {
        assert_eq!(shell_quote(OsStr::new("/A B/it's")), r"'/A B/it'\''s'");
        assert_eq!(applescript_escape(r#"a "b" \c"#), r#"a \"b\" \\c"#);
    }
}
