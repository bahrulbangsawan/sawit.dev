//! `sawit account …`, and `cp1`…`cp9`: the accounts from any terminal. It reads and writes the
//! same files as the Accounts page (which notices within a few seconds).
//!
//! An `<account>` is its name, its number in `list`, or the start of one name.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::claude;
use super::{Accounts, AgentKind, run, usage};
use crate::instances::AppPaths;

pub const USAGE: &str = "\
sawit account: Claude Code accounts (the Accounts page)

  sawit account list [--json]          every account: number, name, who, plan, 5h / 7d usage
  sawit account open [account] [dir]   start claude as the account here (`cp<n>` = open <n>)
  sawit account use <account>          make it active: a plain `claude` starts as it (shell block)
  sawit account current                the active account's name
  sawit account env [account]          the export that pins this shell: eval \"$(sawit account env work)\"
  sawit account login <account>        Claude Code's own `claude auth login` for it
  sawit account logout <account>       Claude Code's own `claude auth logout` for it
  sawit account status [account]       checks sign-in now (exit 1 if any is signed out)
  sawit account usage [account]        5-hour and 7-day usage as Claude Code last reported it
  sawit account add <name> [--dir D]   adds an account (~/.claude-<name> unless --dir)
  sawit account rename <account> <name>
  sawit account remove <account> [--yes]  takes it out of Sawit; the folder and sign-in stay

Options after `open <account>` that aren't a folder go to claude.
";

struct Ctx {
    root: PathBuf,
    home: PathBuf,
}

fn ctx() -> Ctx {
    Ctx {
        root: AppPaths::default_macos().root,
        home: PathBuf::from(std::env::var("HOME").unwrap_or_default()),
    }
}

fn load(c: &Ctx) -> Result<Accounts, String> {
    Accounts::load(&c.root).map_err(|e| e.to_string())
}

fn pick<'a>(all: &'a Accounts, what: Option<&str>, c: &Ctx) -> Result<&'a super::Account, String> {
    match what {
        Some(w) => all
            .find(AgentKind::ClaudeCode, w)
            .ok_or_else(|| format!("No account “{w}”. `sawit account list` shows them.")),
        None => all.active(AgentKind::ClaudeCode, &c.root).ok_or_else(|| {
            "No accounts yet: add one on the Accounts page or with `sawit account add`.".into()
        }),
    }
}

/// Runs `sawit account <args>`; the exit code.
pub fn main(args: &[OsString]) -> i32 {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    match run_cli(&args, &ctx()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("sawit account: {e}");
            1
        }
    }
}

/// What `cp<n> [args]` runs: `open <n> [args]`.
pub fn shortcut_main(n: usize, rest: &[OsString]) -> i32 {
    let mut args = vec![OsString::from("open"), OsString::from(n.to_string())];
    args.extend(rest.iter().cloned());
    main(&args)
}

fn run_cli(args: &[String], c: &Ctx) -> Result<i32, String> {
    let (cmd, rest) = match args.split_first() {
        Some((cmd, rest)) => (cmd.as_str(), rest),
        None => ("list", &[][..]),
    };
    let arg = |i: usize| {
        rest.get(i)
            .map(String::as_str)
            .filter(|a| !a.starts_with('-'))
    };
    match cmd {
        "list" | "ls" => list(c, rest.iter().any(|a| a == "--json")),
        "current" => {
            let all = load(c)?;
            println!("{}", pick(&all, None, c)?.name);
            Ok(0)
        }
        "use" | "switch" => {
            let all = load(c)?;
            let a = pick(&all, Some(arg(0).ok_or("Name the account.")?), c)?;
            super::write_active(&c.root, a).map_err(|e| e.to_string())?;
            println!(
                "New claude sessions use {} (with the shell block; or `sawit account open`).",
                a.name
            );
            Ok(0)
        }
        "env" => {
            let all = load(c)?;
            let a = pick(&all, arg(0), c)?;
            match a.env_value() {
                Some(d) => println!(
                    "export CLAUDE_CONFIG_DIR={}",
                    crate::terminal::shell_quote(d)
                ),
                // The shell block would send this shell to the active account: skip it.
                None => println!("unset CLAUDE_CONFIG_DIR\nclaude() {{ command claude \"$@\"; }}"),
            }
            Ok(0)
        }
        "open" => open(c, rest),
        "login" | "logout" => {
            let all = load(c)?;
            let a = pick(&all, Some(arg(0).ok_or("Name the account.")?), c)?;
            let mut argv = vec!["auth".to_string(), cmd.to_string()];
            argv.extend(rest.iter().skip(1).cloned());
            exec_claude(c, a, &argv, None)
        }
        "status" => status(c, arg(0)),
        "usage" => usage_cmd(c, arg(0), rest.iter().any(|a| a == "--check")),
        "add" => add(c, rest),
        "rename" => {
            let mut all = load(c)?;
            let id = pick(&all, Some(arg(0).ok_or("Name the account.")?), c)?
                .id
                .clone();
            let name = all
                .rename(&id, arg(1).ok_or("Give the new name.")?)
                .map_err(|e| e.to_string())?;
            all.save(&c.root).map_err(|e| e.to_string())?;
            println!("Renamed to {name}.");
            Ok(0)
        }
        "remove" | "rm" => remove(c, rest),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(0)
        }
        other => Err(format!("unknown command “{other}”.\n\n{USAGE}")),
    }
}

fn list(c: &Ctx, json: bool) -> Result<i32, String> {
    let all = load(c)?;
    let active = all
        .active(AgentKind::ClaudeCode, &c.root)
        .map(|a| a.id.clone());
    let now = crate::github::now() as f64;
    let rows: Vec<serde_json::Value> = all
        .of(AgentKind::ClaudeCode)
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let details = std::fs::read_to_string(claude::global_config(a, &c.home))
                .ok()
                .and_then(|t| claude::parse_details(&t));
            let u = usage::load_snapshot(&c.root, &a.id);
            let cur = |w: Option<usage::Window>| {
                w.filter(|w| w.resets_at > now)
                    .map(|w| w.used_percentage.round())
            };
            serde_json::json!({
                "number": i + 1,
                "name": a.name,
                "active": Some(&a.id) == active.as_ref(),
                "dir": a.dir,
                "email": details.as_ref().and_then(|d| d.email_address.clone()),
                "plan": claude::plan(None, details.as_ref(), None),
                "five_hour": u.as_ref().and_then(|u| cur(u.five_hour)),
                "seven_day": u.as_ref().and_then(|u| cur(u.seven_day)),
            })
        })
        .collect();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        );
        return Ok(0);
    }
    for r in rows {
        let pct = |k: &str| {
            r[k].as_f64()
                .map(|v| format!("{v}%"))
                .unwrap_or_else(|| "–".into())
        };
        println!(
            "{} {:>2}  {:<16} {:<28} 5h {:>4}  7d {:>4}",
            if r["active"] == true { "*" } else { " " },
            r["number"],
            r["name"].as_str().unwrap_or_default(),
            r["email"]
                .as_str()
                .unwrap_or("(not signed in or not known yet)"),
            pct("five_hour"),
            pct("seven_day"),
        );
    }
    Ok(0)
}

/// `open [account] [folder] [claude options…]`: replaces this process with `claude`.
fn open(c: &Ctx, rest: &[String]) -> Result<i32, String> {
    let all = load(c)?;
    let mut rest = rest.to_vec();
    let account = match rest.first().filter(|a| !a.starts_with('-')) {
        Some(w) if all.find(AgentKind::ClaudeCode, w).is_some() => Some(rest.remove(0)),
        _ => None,
    };
    let a = pick(&all, account.as_deref(), c)?;
    let dir = match rest.first().filter(|a| !a.starts_with('-')) {
        Some(d) if Path::new(d).is_dir() => Some(PathBuf::from(rest.remove(0))),
        Some(d) => match all
            .settings
            .projects_folder
            .as_ref()
            .and_then(|p| project_in(p, d))
        {
            Some(p) => {
                rest.remove(0);
                Some(p)
            }
            None => None,
        },
        None => None,
    };
    if rest.first().is_some_and(|a| a == "--") {
        rest.remove(0);
    }
    exec_claude(c, a, &rest, dir.as_deref())
}

/// The folder in `projects` whose name starts with `start`.
fn project_in(projects: &Path, start: &str) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(projects)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with(start))
        .map(|e| e.path())
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Replaces this process with `claude args` as the account, the rest of the environment kept.
fn exec_claude(
    c: &Ctx,
    a: &super::Account,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<i32, String> {
    use std::os::unix::process::CommandExt as _;
    let bin =
        run::find_claude(&c.home).ok_or("Claude Code isn't installed (no `claude` on PATH).")?;
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    match a.env_value() {
        Some(d) => cmd.env(a.kind.profile_var(), d),
        None => cmd.env_remove(a.kind.profile_var()),
    };
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    Err(format!("couldn't start claude: {}", cmd.exec()))
}

fn status(c: &Ctx, which: Option<&str>) -> Result<i32, String> {
    let all = load(c)?;
    let bin =
        run::find_claude(&c.home).ok_or("Claude Code isn't installed (no `claude` on PATH).")?;
    let list: Vec<&super::Account> = match which {
        Some(w) => vec![pick(&all, Some(w), c)?],
        None => all.of(AgentKind::ClaudeCode),
    };
    let mut out = 0;
    for a in list {
        match run::auth_status(&bin, a, &c.home) {
            Ok(s) if s.signed_in => println!(
                "{:<16} signed in: {}",
                a.name,
                s.identity().unwrap_or_default()
            ),
            Ok(_) => {
                println!("{:<16} signed out", a.name);
                out = 1;
            }
            Err(e) => {
                println!("{:<16} {e}", a.name);
                out = 1;
            }
        }
    }
    Ok(out)
}

fn usage_cmd(c: &Ctx, which: Option<&str>, check: bool) -> Result<i32, String> {
    let all = load(c)?;
    let now = crate::github::now() as f64;
    let list: Vec<&super::Account> = match which {
        Some(w) => vec![pick(&all, Some(w), c)?],
        None => all.of(AgentKind::ClaudeCode),
    };
    for a in list {
        let snap = if check {
            let bin = run::find_claude(&c.home).ok_or("Claude Code isn't installed.")?;
            run::check_usage(&bin, a, &c.home, &c.root).map_err(|e| e.to_string())?
        } else {
            match usage::load_snapshot(&c.root, &a.id) {
                Some(s) => s,
                None => {
                    println!("{:<16} no data yet", a.name);
                    continue;
                }
            }
        };
        let show = |l: &str, w: Option<usage::Window>| match w {
            Some(w) if w.resets_at > now => format!(
                "{l} {}% (resets in {}m)",
                w.used_percentage.round(),
                ((w.resets_at - now) / 60.0) as i64
            ),
            Some(_) => format!("{l} reset, no newer data"),
            None => format!("{l} –"),
        };
        println!(
            "{:<16} {}  {}",
            a.name,
            show("5h", snap.five_hour),
            show("7d", snap.seven_day)
        );
    }
    Ok(0)
}

fn add(c: &Ctx, rest: &[String]) -> Result<i32, String> {
    let name = rest
        .first()
        .filter(|a| !a.starts_with('-'))
        .ok_or("Give the account a name.")?;
    let dir = rest
        .iter()
        .position(|a| a == "--dir")
        .and_then(|i| rest.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| c.home.join(format!(".claude-{}", super::kebab(name))));
    if !dir.exists() {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|e| e.to_string())?;
    }
    let mut all = load(c)?;
    let account = super::Account {
        id: super::new_id(),
        kind: AgentKind::ClaudeCode,
        name: name.clone(),
        is_default: dir == AgentKind::ClaudeCode.default_dir(&c.home),
        dir,
        color: "#4F8EF7".into(),
        order: 0,
        tracking: false,
        original_status_line: None,
        created_at: crate::github::now(),
    };
    let added = all.add(account).map_err(|e| e.to_string())?.clone();
    all.save(&c.root).map_err(|e| e.to_string())?;
    println!(
        "Added {} ({}). Sign it in: sawit account login {}",
        added.name,
        added.dir.display(),
        added.name
    );
    Ok(0)
}

fn remove(c: &Ctx, rest: &[String]) -> Result<i32, String> {
    let mut all = load(c)?;
    let a = pick(&all, Some(rest.first().ok_or("Name the account.")?), c)?.clone();
    if !rest.iter().any(|x| x == "--yes") {
        eprint!(
            "Remove {} from Sawit? Its folder and sign-in stay. [y/N] ",
            a.name
        );
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("Nothing removed.");
            return Ok(1);
        }
    }
    let mut gone = a.clone();
    if gone.tracking {
        let _ = super::tracking::disable(&c.root, &mut gone, crate::github::now());
    }
    all.remove(&a.id);
    all.save(&c.root).map_err(|e| e.to_string())?;
    println!(
        "Removed {}. {} and its sign-in are still there.",
        a.name,
        a.dir.display()
    );
    Ok(0)
}

#[cfg(test)]
pub(super) fn run_for_test(args: &[&str], root: &Path, home: &Path) -> Result<i32, String> {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    run_cli(
        &args,
        &Ctx {
            root: root.into(),
            home: home.into(),
        },
    )
}
