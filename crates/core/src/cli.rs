//! The `sawit` command line, like `zed`: argument parsing (`file:line:col`
//! included), the request the CLI sends to the running app over its Unix
//! socket, and installing the `sawit` symlink into `PATH`.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use crate::kebun::{BakarArgs, CabutArgs, DbJob, ListWhat, NyalakanArgs, PasangArgs, PupukArgs};
use crate::{CoreError, Result, cmd};

/// Where "Install CLI" puts the command.
pub const CLI_LINK: &str = "/usr/local/bin/sawit";

pub const USAGE: &str = "\
sawit — open folders and files in Sawit, query databases, clear ports

Usage:
  sawit [options] [path[:line[:column]] ...]
  sawit <db> [table] [\"sql\"] [-y]
  sawit list [instances|db|ports|connections|tools|mcp|skills|config|keys]
  sawit bakar ports [port ...] [-y]
  sawit bakar instances [name ...] [-y]
  sawit nyalakan [name ...] [--ulang] [-y]
  sawit browse [url|port]
  sawit panen <db> <table|\"sql\"> <file.csv|file.xlsx> [-y]
  sawit backup <db> [schema|schema.table] <file.sql|file.dump> [-y]
  sawit tanam <db> <table> <file.csv>
  sawit pasang <tool ...> [-y]
  sawit pupuk [tool ...] [-y]
  sawit cabut <tool ...> [-y]

Examples:
  sawit .                  open this folder as a project (joins the open
                           window next to its other projects)
  sawit -n ~/code/shop     open a folder in a new window of its own
  sawit -r ~/code/blog     open it in place of the front window's project
  sawit src/main.rs:42     open a file at line 42
  sawit -w COMMIT_EDITMSG  wait until the file's tab is closed (git core.editor)
  ls | sawit -             open stdin as a new file
  sawit -- -notes.md       a name that starts with - (after --, all are paths)

Browser (the project's tab in Sawit, which starts if it isn't running):
  sawit browse             this project's running dev server
  sawit browse 5173        http://localhost:5173
  sawit browse example.com any address, as the address field takes it
  sawit https://example.com
                           a URL on its own opens there too

Kebun (databases), no app needed:
  sawit kyzn                               list tables
  sawit kyzn leads                         first 100 rows
  sawit kyzn leads \"phone is null\"         filter (becomes WHERE)
  sawit kyzn leads \"select * from leads where phone is null\"
  sawit kyzn \"delete from leads where id = 3\"
                                           SQL is tried read-only first; SQL
                                           that writes, or several statements,
                                           is shown, then asks (-y: run it
                                           without asking)
  <db> is a Sawit instance, a saved connection, a database named in
  ./.env DATABASE_URL (or a folder above), or a database on a running
  local Postgres. A unique prefix works: kyzn finds kyzn_crm.

  sawit list               instances, databases, ports and connections
  sawit list db            just one: instances, db, ports or connections
  sawit bakar ports        stop dev servers on ports (asks first;
                           SIGTERM, then SIGKILL for the stubborn)
  sawit bakar ports 3000   stop whatever listens on :3000
  sawit bakar instances    stop running Sawit instances (data is kept)
  sawit nyalakan           start every stopped Sawit instance; a port
                           another process holds is explained, not taken
  sawit nyalakan kyzn --ulang
                           restart kyzn (asks first while it runs; -y)

  sawit panen kyzn leads leads.xlsx        export a table (CSV or Excel)
  sawit panen kyzn \"select * from leads where phone is null\" x.csv
                                           export a query (read-only)
  sawit backup kyzn kyzn.dump              pg_dump the database (.dump is
                                           pg_restore's format, else SQL)
  sawit backup kyzn crm.leads leads.sql    one schema or table
  sawit tanam kyzn leads leads.csv         load a CSV file into a table
  An existing file is replaced only after asking (or with -y).

Tools (the app's Tools page):
  sawit list tools         package managers, runtimes, AI agents (alat)
  sawit pasang bun uv      run their official install scripts, prerequisites
                           first (asks first; -y to skip)
  sawit pupuk              upgrade every installed tool with a newer
                           release (shown as → in `list tools`)
  sawit pupuk bun          upgrade just bun (asks first; -y to skip)
  sawit cabut bun          uninstall bun: only the copy its own installer,
                           Homebrew or npm put there (asks first; -y to skip)

More lists, shown only when asked:
  sawit list mcp           the MCP page: servers and the AI clients that
                           have them (colokan)
  sawit list skills        the Skills page: skill packs, how each installs
                           and the agents that have them (skill, bibit)
  sawit list config       shell startup files and coding agents' config
                           files, for this folder's project (configs, konfig)

Keys (the app's Keymap Editor; Sawit must be running):
  sawit list keys          every shortcut: its keys, what it runs, where,
                           and whether it's a default, yours or turned off
  sawit list keys | grep -i split
                           find one (aliases: key, keymap, shortcuts, pintasan)

Aliases (every word has an English and an Indonesian name):
  list = ls, daftar · bakar = stop · nyalakan = start · browse = intip
  panen = export, ekspor · backup = dump, cadangkan · tanam = import, impor
  pasang = install · pupuk = upgrade · cabut = uninstall
  list: all = semua · instances = instance, kebun · db = dbs, database,
    databases, blok · ports = port, lahan · connections = connection,
    conn, koneksi, konsesi · tools = tool, alat, perkakas · mcp = colokan ·
    skills = skill, bibit ·
    config = configs, konfig · keys = key, keymap, shortcuts, pintasan
  bakar: ports = port, lahan · instances = instance, kebun
  -y = --yes, --ya (don't ask first) · --ulang = --restart
  `sawit ./panen` opens a folder named panen; so for every word here.

Options:
  -n, --new         always open in a new window
  -r, --reuse       use the frontmost window, replacing its active project
  -w, --wait        wait until every opened file is closed
  -                 read stdin into a temporary file and open it
  --                the rest are paths, even when they start with -
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
        assert_eq!(
            parse(&["list", "colokan"]),
            Ok(Invocation::List(ListWhat::Mcp))
        );
        assert_eq!(
            parse(&["ls", "konfig"]),
            Ok(Invocation::List(ListWhat::Config))
        );
    }

    /// `sawit list keys` lists the key bindings under each of its names, `sawit --help` names
    /// them all with an example, and `sawit ./keys` still opens a folder named keys.
    #[test]
    fn list_keys_has_its_names_and_its_help() {
        for word in ["keys", "key", "keymap", "shortcuts", "pintasan"] {
            assert_eq!(
                parse(&["list", word]),
                Ok(Invocation::List(ListWhat::Keys)),
                "{word}"
            );
        }
        assert_eq!(
            parse(&["ls", "pintasan"]),
            Ok(Invocation::List(ListWhat::Keys))
        );
        assert!(matches!(parse(&["./keys"]), Ok(Invocation::Open { .. })));
        assert!(matches!(parse(&["keys"]), Ok(Invocation::Open { .. })));
        for said in [
            "sawit list [instances|db|ports|connections|tools|mcp|skills|config|keys]",
            "sawit list keys ",
            "Sawit must be running",
            "key, keymap, shortcuts, pintasan",
        ] {
            assert!(USAGE.contains(said), "the help doesn't say {said:?}");
        }
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
        let Ok(Invocation::Cabut(c)) = parse(&["uninstall", id, "-y"]) else {
            panic!()
        };
        assert_eq!(c.tools, [id]);
        assert!(c.yes);
        let Ok(Invocation::Cabut(c)) = parse(&["cabut", id]) else {
            panic!()
        };
        assert!(!c.yes);
        assert!(parse(&["cabut"]).is_err());
        assert!(parse(&["cabut", "nope-tool"]).is_err());
        assert!(parse(&["cabut", id, "--force"]).is_err());
        assert!(matches!(parse(&["./cabut"]), Ok(Invocation::Open { .. })));
        assert!(matches!(
            parse(&["./uninstall"]),
            Ok(Invocation::Open { .. })
        ));
    }

    /// `sawit --help` names `cabut`, its alias and an example.
    #[test]
    fn help_names_cabut() {
        for said in [
            "sawit cabut <tool ...> [-y]",
            "cabut = uninstall",
            "sawit cabut bun",
        ] {
            assert!(USAGE.contains(said), "no {said:?} in the help");
        }
    }

    /// `sawit --help` names the lists shown only when asked, with their
    /// aliases.
    #[test]
    fn help_names_the_mcp_and_config_lists() {
        for said in [
            "tools|mcp|skills|config|keys]",
            "sawit list mcp",
            "(colokan)",
            "sawit list config",
            "(configs, konfig)",
        ] {
            assert!(USAGE.contains(said), "no {said:?} in the help");
        }
    }

    /// `sawit nyalakan` (`start`) starts instances; `./start` is still a folder.
    #[test]
    fn nyalakan_starts_instances() {
        assert_eq!(
            parse(&["start"]),
            Ok(Invocation::Nyalakan(NyalakanArgs::default()))
        );
        let Ok(Invocation::Nyalakan(args)) = parse(&["nyalakan", "kyzn", "--ulang", "-y"]) else {
            panic!()
        };
        assert_eq!(args.names, ["kyzn"]);
        assert!(args.again && args.yes);
        assert!(parse(&["start", "--nope"]).is_err());
        assert!(matches!(parse(&["./start"]), Ok(Invocation::Open { .. })));
        assert!(matches!(
            parse(&["./nyalakan"]),
            Ok(Invocation::Open { .. })
        ));
    }

    /// `sawit browse` (`intip`) opens this project's dev server, a port or an address in Sawit's
    /// browser, and so does a URL on its own. With anything else a URL is refused, where it was
    /// opened as an empty file `http:/…` at the port's line.
    #[test]
    fn browse_opens_pages() {
        let address = |a: &str| Ok(Invocation::Browse(BrowseTarget::Address(a.into())));
        assert_eq!(
            parse(&["browse"]),
            Ok(Invocation::Browse(BrowseTarget::DevServer))
        );
        assert_eq!(parse(&["intip", "5173"]), address("http://localhost:5173"));
        assert_eq!(
            parse(&["browse", "example.com/a"]),
            address("example.com/a")
        );
        assert_eq!(
            parse(&["http://localhost:5173/x"]),
            address("http://localhost:5173/x")
        );
        assert_eq!(parse(&["HTTPS://x.dev"]), address("HTTPS://x.dev"));
        for bad in [
            &["browse", "0"][..],
            &["browse", "70000"],
            &["browse", ""],
            &["browse", "a.dev", "b.dev"],
            &["intip", "-y"],
            &["-n", "http://x.dev"],
            &["a.rs", "https://x.dev"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        // Past `--` it is a path, and `./browse` is a folder.
        assert!(matches!(
            parse(&["--", "http://x.dev"]),
            Ok(Invocation::Open { .. })
        ));
        assert!(matches!(parse(&["./browse"]), Ok(Invocation::Open { .. })));
        assert!(matches!(parse(&["./intip"]), Ok(Invocation::Open { .. })));
    }

    /// `list` and `bakar` have their names in both languages: `daftar` and `stop`. Neither is a
    /// database, and `./stop` is still a folder.
    #[test]
    fn list_and_bakar_in_both_languages() {
        assert_eq!(
            parse(&["daftar", "ports"]),
            Ok(Invocation::List(ListWhat::Ports))
        );
        let Ok(Invocation::Bakar(bakar)) = parse(&["stop", "lahan", "3000", "-y"]) else {
            panic!()
        };
        assert_eq!(bakar.targets, ["3000"]);
        assert!(bakar.yes);
        assert!(parse(&["stop"]).is_err());
        assert!(matches!(parse(&["./daftar"]), Ok(Invocation::Open { .. })));
        assert!(matches!(parse(&["./stop"]), Ok(Invocation::Open { .. })));
    }

    /// `sawit --help` names every word `sawit` takes as a command, every target of `list` and
    /// `bakar` and every flag, each alias among them: it left out `ls`, most targets, `--yes`
    /// and `--`.
    #[test]
    fn the_help_names_every_word_and_alias() {
        let named: std::collections::HashSet<&str> = USAGE
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .collect();
        for word in crate::kebun::SUBCOMMANDS {
            assert!(named.contains(word), "--help doesn't name {word}");
        }
        for target in [
            "all",
            "semua",
            "instances",
            "instance",
            "kebun",
            "db",
            "dbs",
            "database",
            "databases",
            "blok",
            "ports",
            "port",
            "lahan",
            "connections",
            "connection",
            "conn",
            "koneksi",
            "konsesi",
            "tools",
            "tool",
            "alat",
            "perkakas",
            "mcp",
            "colokan",
            "skills",
            "skill",
            "bibit",
            "config",
            "configs",
            "konfig",
            "keys",
            "key",
            "keymap",
            "shortcuts",
            "pintasan",
        ] {
            assert!(ListWhat::parse(Some(target)).is_ok(), "{target}");
            assert!(named.contains(target), "--help doesn't name list {target}");
        }
        for target in ["ports", "port", "lahan", "instances", "instance", "kebun"] {
            assert!(BakarArgs::parse(&[target]).is_ok(), "{target}");
            assert!(named.contains(target), "--help doesn't name bakar {target}");
        }
        for flag in [
            "-y",
            "--yes",
            "--ya",
            "--ulang",
            "--restart",
            "--",
            "-",
            "-n",
            "--new",
            "-r",
            "--reuse",
            "-w",
            "--wait",
            "--foreground",
            "-v",
            "--version",
            "-h",
            "--help",
        ] {
            assert!(named.contains(flag), "--help doesn't name {flag}");
        }
        // The lines a terminal shows whole.
        for line in USAGE.lines() {
            assert!(line.chars().count() <= 81, "too wide: {line}");
        }
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
