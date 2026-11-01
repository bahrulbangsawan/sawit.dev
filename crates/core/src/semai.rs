//! `sawit new` (`semai`, to sow seedlings): the New Project page from the terminal. With no
//! arguments it opens the page in the app; with a name or stack flags (create-better-t-stack's
//! own) it prints the command for the stack the compatibility rules resolve, and with
//! `--create` (`--buat`) it runs it here. The same rules and commands as the page and
//! `sawit-mcp`'s `stack_*` tools ([`crate::stack_builder`], [`crate::new_project`]).

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::app_control::{self, Command, Page, Request};
use crate::instances::AppPaths;
use crate::new_project;
use crate::stack_builder::{self, Choices};

/// How long the app gets to answer `sawit new`.
const APP_TIMEOUT: Duration = Duration::from_secs(35);

/// `sawit new`'s arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemaiArgs {
    /// The stack: a name, a template and create-better-t-stack's stack flags.
    pub choices: Choices,
    /// Where to create it (`--dir`, `--lokasi`); default the folder `sawit new` runs in.
    pub dir: Option<PathBuf>,
    /// Run the command here (`--create`, `--buat`) instead of printing it.
    pub create: bool,
}

impl SemaiArgs {
    /// Whether nothing was given: `sawit new` alone opens the page.
    pub fn is_empty(&self) -> bool {
        self == &SemaiArgs::default()
    }

    /// `sawit new`'s words after `new`: an optional name, then flags. A flag that takes several
    /// values (`--frontend`, `--addons`, `--examples`) takes the words up to the next flag.
    pub fn parse(words: &[&str]) -> Result<SemaiArgs, String> {
        let mut args = SemaiArgs::default();
        let mut ix = 0;
        let value = |ix: usize, flag: &str| -> Result<String, String> {
            words
                .get(ix + 1)
                .filter(|w| !w.starts_with("--"))
                .map(|w| w.to_string())
                .ok_or_else(|| format!("{flag} butuh nilai (needs a value)"))
        };
        let values = |ix: usize| -> Vec<String> {
            words[ix + 1..]
                .iter()
                .take_while(|w| !w.starts_with("--"))
                .map(|w| w.to_string())
                .collect()
        };
        while ix < words.len() {
            let word = words[ix];
            let c = &mut args.choices;
            let one = |slot: &mut Option<String>| -> Result<usize, String> {
                *slot = Some(value(ix, word)?);
                Ok(2)
            };
            let step = match word {
                "--template" => one(&mut c.template)?,
                "--backend" => one(&mut c.backend)?,
                "--runtime" => one(&mut c.runtime)?,
                "--api" => one(&mut c.api)?,
                "--auth" => one(&mut c.auth)?,
                "--payments" => one(&mut c.payments)?,
                "--database" => one(&mut c.database)?,
                "--orm" => one(&mut c.orm)?,
                "--db-setup" => one(&mut c.db_setup)?,
                "--web-deploy" => one(&mut c.web_deploy)?,
                "--server-deploy" => one(&mut c.server_deploy)?,
                "--package-manager" => one(&mut c.package_manager)?,
                "--frontend" | "--addons" | "--examples" => {
                    let list = values(ix);
                    if list.is_empty() {
                        return Err(format!("{word} butuh nilai (needs values)"));
                    }
                    let n = list.len();
                    match word {
                        "--frontend" => c.frontend = Some(list),
                        "--addons" => c.addons = Some(list),
                        _ => c.examples = Some(list),
                    }
                    n + 1
                }
                "--git" => {
                    c.git = Some(true);
                    1
                }
                "--no-git" => {
                    c.git = Some(false);
                    1
                }
                "--install" => {
                    c.install = Some(true);
                    1
                }
                "--no-install" => {
                    c.install = Some(false);
                    1
                }
                "--dir" | "--lokasi" => {
                    args.dir = Some(PathBuf::from(value(ix, word)?));
                    2
                }
                "--create" | "--buat" => {
                    args.create = true;
                    1
                }
                flag if flag.starts_with('-') => {
                    return Err(format!(
                        "opsi tak dikenal (unknown option): {flag}. Lihat `sawit --help`."
                    ));
                }
                name if c.name.is_none() => {
                    c.name = Some(name.to_string());
                    1
                }
                extra => {
                    return Err(format!(
                        "nama proyek sudah {} (one name only): {extra}",
                        c.name.as_deref().unwrap_or_default()
                    ));
                }
            };
            ix += step;
        }
        // Checked now, so a typo fails before anything runs.
        args.choices.stack()?;
        Ok(args)
    }
}

/// Colours for a terminal (none when piped, or with `NO_COLOR`).
struct Ink(bool);

impl Ink {
    fn stderr() -> Ink {
        Ink(std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none())
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.0 {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

/// Runs `sawit new`: opens the page (starting the app with `start_app` when it isn't running),
/// prints the command for the stack given, or creates it with `--create`. The exit code.
pub fn semai(
    args: SemaiArgs,
    paths: &AppPaths,
    cwd: &Path,
    start_app: impl FnOnce() -> Result<(), String>,
) -> i32 {
    let ink = Ink::stderr();
    if args.is_empty() {
        return open_page(paths, cwd, start_app, &ink);
    }
    let stack = match args.choices.stack() {
        Ok(stack) => stack,
        Err(e) => {
            eprintln!("{} {e}", ink.paint("31", "🔥"));
            return 2;
        }
    };
    let resolved = stack_builder::resolve(&stack);
    for change in &resolved.changes {
        eprintln!("{} {}", ink.paint("33", "🍂 disesuaikan:"), change.message);
    }
    for note in &resolved.notes {
        eprintln!("{} {note}", ink.paint("90", "📝"));
    }
    if !args.create {
        println!("{}", stack_builder::command(&resolved.stack));
        eprintln!(
            "{}",
            ink.paint(
                "90",
                &format!(
                    "🌱 create-better-t-stack {} · tambah --create untuk menyemai di sini",
                    stack_builder::CLI_VERSION
                )
            )
        );
        return 0;
    }
    let location = args
        .dir
        .map(|d| if d.is_absolute() { d } else { cwd.join(d) })
        .unwrap_or_else(|| cwd.to_path_buf());
    let dir = match new_project::target(&location, &resolved.stack) {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("{} {e}", ink.paint("31", "🔥"));
            return 1;
        }
    };
    let command = stack_builder::run_command(&resolved.stack);
    eprintln!(
        "{} {}\n$ {command}",
        ink.paint("32", "🌱 Menyemai"),
        dir.display()
    );
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&command)
        .current_dir(&location)
        .status();
    match status {
        Ok(_) if new_project::created(&dir) => {
            eprintln!(
                "{} {} siap tumbuh. Buka di Sawit: sawit {}",
                ink.paint("32", "🌴"),
                dir.display(),
                crate::package_manager::quote(&dir.to_string_lossy())
            );
            0
        }
        Ok(status) => {
            eprintln!(
                "{} benih tidak tumbuh: create-better-t-stack berhenti ({status}) tanpa package.json di {}",
                ink.paint("31", "🔥"),
                dir.display()
            );
            1
        }
        Err(e) => {
            eprintln!("{} /bin/sh tidak mau jalan: {e}", ink.paint("31", "🔥"));
            1
        }
    }
}

/// `sawit new` alone: the New Project page, in the app, in front.
fn open_page(
    paths: &AppPaths,
    cwd: &Path,
    start_app: impl FnOnce() -> Result<(), String>,
    ink: &Ink,
) -> i32 {
    if !app_control::is_running(paths)
        && let Err(e) = start_app()
    {
        eprintln!("{} Sawit tidak mau menyala: {e}", ink.paint("31", "🔥"));
        return 1;
    }
    let folder = (cwd.parent().is_some()
        && std::env::var_os("HOME").map(PathBuf::from).as_deref() != Some(cwd))
    .then(|| cwd.to_path_buf());
    let ask = |command| {
        // `sawit new` is the person at the terminal, not an agent: no agent cursor.
        let request = Request {
            project: None,
            agent_folder: folder.clone(),
            agent: None,
            command,
        };
        app_control::send(paths, &request, APP_TIMEOUT)
    };
    let page = Page::NewProject;
    if let Err(e) = ask(Command::OpenPage { page }) {
        eprintln!(
            "{} halaman New Project tidak terbuka: {e}",
            ink.paint("31", "🔥")
        );
        return 1;
    }
    let _ = ask(Command::Focus);
    eprintln!(
        "{}",
        ink.paint(
            "90",
            "🌱 New Project terbuka di Sawit: pilih stack, lalu Create Project"
        )
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(words: &[&str]) -> Result<SemaiArgs, String> {
        SemaiArgs::parse(words)
    }

    /// Nothing given opens the page; a name and upstream's flags, several values each where
    /// the CLI takes several, and Sawit's own flags in both languages.
    #[test]
    fn the_flags_are_create_better_t_stacks() {
        assert!(parse(&[]).unwrap().is_empty());
        let args = parse(&[
            "shop",
            "--frontend",
            "next",
            "native-uniwind",
            "--backend",
            "self",
            "--addons",
            "biome",
            "turborepo",
            "--no-git",
            "--package-manager",
            "pnpm",
            "--lokasi",
            "/tmp",
            "--buat",
        ])
        .unwrap();
        assert_eq!(args.choices.name.as_deref(), Some("shop"));
        assert_eq!(
            args.choices.frontend,
            Some(vec!["next".to_string(), "native-uniwind".to_string()])
        );
        assert_eq!(args.choices.addons.as_ref().map(Vec::len), Some(2));
        assert_eq!(args.choices.git, Some(false));
        assert_eq!(args.dir, Some(PathBuf::from("/tmp")));
        assert!(args.create);
        let stack = args.choices.stack().unwrap();
        assert_eq!(stack.backend, stack_builder::Backend::SelfNext);
        let t3 = parse(&["--template", "t3", "--create"]).unwrap();
        assert_eq!(t3.choices.template.as_deref(), Some("t3"));
        assert!(t3.create);
        assert!(!t3.is_empty());
    }

    /// Typos fail before anything runs, with what to do.
    #[test]
    fn mistakes_fail_with_a_reason() {
        assert!(parse(&["--orm"]).unwrap_err().contains("--orm"));
        assert!(
            parse(&["--orm", "sequelize"])
                .unwrap_err()
                .contains("drizzle")
        );
        assert!(parse(&["--frontend"]).is_err());
        assert!(parse(&["--frontend", "next", "svelte"]).is_err());
        assert!(parse(&["--nope"]).unwrap_err().contains("--nope"));
        assert!(parse(&["a", "b"]).unwrap_err().contains("one name"));
        assert!(parse(&["--template", "lamp"]).is_err());
    }
}
