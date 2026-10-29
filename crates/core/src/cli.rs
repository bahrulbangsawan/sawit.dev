//! The `sawit` command line, like `zed`: argument parsing (`file:line:col`
//! included), the request the CLI sends to the running app over its Unix
//! socket, and installing the `sawit` symlink into `PATH`.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use crate::kebun::{BakarArgs, DbJob, ListWhat, PasangArgs, PupukArgs};
use crate::{CoreError, Result, cmd};

/// Where "Install CLI" puts the command.
pub const CLI_LINK: &str = "/usr/local/bin/sawit";

pub const USAGE: &str = "\
sawit — open folders and files in Sawit, query databases, clear ports

Usage:
  sawit [options] [path[:line[:column]] ...]
  sawit <db> [table] [\"sql\"]
  sawit list [instances|db|ports|connections|tools]
  sawit bakar ports [port ...] [-y]
  sawit bakar instances [name ...] [-y]
  sawit panen <db> <table|\"sql\"> <file.csv|file.xlsx> [-y]
  sawit backup <db> [schema|schema.table] <file.sql|file.dump> [-y]
  sawit tanam <db> <table> <file.csv>
  sawit pasang <tool ...> [-y]
  sawit pupuk [tool ...] [-y]

Examples:
  sawit .                  open this folder as a project (joins the open
                           window next to its other projects)
  sawit src/main.rs:42     open a file at line 42
  sawit -w COMMIT_EDITMSG  wait until the file's tab is closed (git core.editor)
  ls | sawit -             open stdin as a new file

Kebun (databases), no app needed:
  sawit kyzn                               list tables
  sawit kyzn leads                         first 100 rows
  sawit kyzn leads \"phone is null\"         filter (becomes WHERE)
  sawit kyzn leads \"select * from leads where phone is null\"
  <db> is a Sawit instance, a saved connection, a database named in
  ./.env DATABASE_URL (or a folder above), or a database on a running
  local Postgres. A unique prefix works: kyzn finds kyzn_crm.

  sawit list               instances, databases, ports and connections
  sawit bakar ports        stop dev servers on ports (asks first;
                           SIGTERM, then SIGKILL for the stubborn)
  sawit bakar ports 3000   stop whatever listens on :3000
  sawit bakar instances    stop running Sawit instances (data is kept)

  sawit panen kyzn leads leads.xlsx        export a table (CSV or Excel)
  sawit panen kyzn \"select * from leads where phone is null\" x.csv
                                           export a query (read-only)
  sawit backup kyzn kyzn.dump              pg_dump the database (.dump is
                                           pg_restore's format, else SQL)
  sawit backup kyzn crm.leads leads.sql    one schema or table
  sawit tanam kyzn leads leads.csv         load a CSV file into a table
  An existing file is replaced only after asking (or with -y).
  Aliases: panen = export, ekspor · backup = dump, cadangkan ·
  tanam = import, impor · pasang = install · pupuk = upgrade.
  `sawit ./panen` opens a
  folder named panen.

Tools (the app's Tools page):
  sawit list tools         package managers, runtimes, AI agents (alat)
  sawit pasang bun uv      run their official install scripts, prerequisites
                           first (asks first; -y to skip)
  sawit pupuk              upgrade every installed tool with a newer
                           release (shown as → in `list tools`)
  sawit pupuk bun          upgrade just bun (asks first; -y to skip)

Options:
  -n, --new         always open in a new window
  -r, --reuse       use the frontmost window, replacing its active project
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
    let args: Vec<OsString> = args.into_iter().collect();
    let words: Vec<&str> = args.iter().filter_map(|a| a.to_str()).collect();
    match words.first().copied() {
        Some("list" | "ls") if words.len() == args.len() && words.len() <= 2 => {
            return ListWhat::parse(words.get(1).copied()).map(Invocation::List);
        }
        Some("bakar") if words.len() == args.len() => {
            return BakarArgs::parse(&words[1..]).map(Invocation::Bakar);
        }
        Some("install" | "pasang") if words.len() == args.len() => {
            return PasangArgs::parse(&words[1..]).map(Invocation::Pasang);
        }
        Some("upgrade" | "pupuk") if words.len() == args.len() => {
            return PupukArgs::parse(&words[1..]).map(Invocation::Pupuk);
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

const MAGIC: &[u8] = b"sawit-open/1";

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
            format!(
                "{} already exists and isn't a symlink. Move or delete it, then try again.",
                link.display()
            ),
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

pub(crate) fn applescript_escape(s: &str) -> String {
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
    fn subcommands() {
        assert_eq!(parse(&["list"]), Ok(Invocation::List(ListWhat::All)));
        assert_eq!(
            parse(&["ls", "ports"]),
            Ok(Invocation::List(ListWhat::Ports))
        );
        assert!(parse(&["list", "hutan"]).is_err());
        let Ok(Invocation::Bakar(bakar)) = parse(&["bakar", "ports", "-y"]) else {
            panic!()
        };
        assert!(bakar.yes && bakar.targets.is_empty());
        assert!(parse(&["bakar"]).is_err());
        // `sawit ./list` still opens a folder named list.
        assert!(matches!(parse(&["./list"]), Ok(Invocation::Open { .. })));
        assert_eq!(
            parse(&["ls", "alat"]),
            Ok(Invocation::List(ListWhat::Tools))
        );
    }

    #[test]
    fn job_subcommands() {
        let Ok(Invocation::DbJob(job)) = parse(&["export", "kyzn", "leads", "leads.csv"]) else {
            panic!()
        };
        assert_eq!(job.kind, crate::kebun::DbJobKind::Export);
        assert_eq!(job.path, PathBuf::from("/nope/leads.csv"));
        assert!(!job.yes);
        let Ok(Invocation::DbJob(job)) = parse(&["dump", "kyzn", "k.dump", "-y"]) else {
            panic!()
        };
        assert_eq!(job.kind, crate::kebun::DbJobKind::Backup);
        assert!(job.yes);
        assert!(matches!(
            parse(&["impor", "kyzn", "leads", "a.csv"]),
            Ok(Invocation::DbJob(_))
        ));
        assert!(parse(&["panen", "kyzn"]).is_err());
        assert!(parse(&["tanam"]).is_err());
        let id = crate::dev_tools::TOOLS[0].id;
        let Ok(Invocation::Pasang(p)) = parse(&["install", id]) else {
            panic!()
        };
        assert_eq!(p.tools, [id]);
        assert!(parse(&["pasang"]).is_err());
        assert!(parse(&["pasang", "nope-tool"]).is_err());
        // The escape still opens folders with these names.
        assert!(matches!(parse(&["./panen"]), Ok(Invocation::Open { .. })));
        assert!(matches!(parse(&["./pasang"]), Ok(Invocation::Open { .. })));
        let Ok(Invocation::Pupuk(p)) = parse(&["upgrade", id, "-y"]) else {
            panic!()
        };
        assert_eq!(p.tools, [id]);
        assert!(p.yes);
        assert!(matches!(parse(&["pupuk"]), Ok(Invocation::Pupuk(_))));
        assert!(parse(&["pupuk", "nope-tool"]).is_err());
        assert!(matches!(parse(&["./pupuk"]), Ok(Invocation::Open { .. })));
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
        assert_eq!(decode(b"sawit-open/1\0\0/a\0x\0"), None);
    }

    #[test]
    fn link_install_and_state() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("sawit-app");
        std::fs::write(&exe, "").unwrap();
        let link = dir.path().join("bin/sawit");
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
