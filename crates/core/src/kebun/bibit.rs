//! `sawit skills install|update|remove [--project <dir>] [-y]`: the footer's Install Skills from
//! a terminal. Globally, the Sawit pack's skills into every agent, the block into each agent's own
//! instruction file and Sawit's MCP server where a client lacks it; with `--project`, only the
//! block in that project's instruction file. It says what it will touch and asks first.

use std::path::{Path, PathBuf};

use super::ink::{Ink, confirm, tilde};
use crate::instances::AppPaths;
use crate::sawit_skills::{self, Level, Scope, files, project};

/// What `sawit skills` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BibitAction {
    /// `install` (`pasang`), and `update` (`pupuk`, `upgrade`): the same run, which brings
    /// whatever is there up to this Sawit's version.
    Install,
    /// `remove` (`cabut`, `uninstall`).
    Remove,
}

/// `sawit skills <action> [--project [dir]] [-y]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BibitArgs {
    pub action: BibitAction,
    /// `--project`: this project only (its folder, made absolute).
    pub project: Option<PathBuf>,
    pub yes: bool,
}

impl BibitArgs {
    /// The words after `skills`; `None` when the first isn't an action, so `sawit skills` alone
    /// still opens a folder named skills.
    pub fn parse(args: &[&str], cwd: &Path) -> Option<Result<BibitArgs, String>> {
        let action = match args.first().copied()? {
            "install" | "pasang" | "update" | "upgrade" | "pupuk" => BibitAction::Install,
            "remove" | "uninstall" | "cabut" => BibitAction::Remove,
            _ => return None,
        };
        let mut out = BibitArgs {
            action,
            project: None,
            yes: false,
        };
        let mut rest = args[1..].iter().peekable();
        while let Some(&arg) = rest.next() {
            match arg {
                "-y" | "--yes" | "--ya" => out.yes = true,
                "--project" | "--proyek" => {
                    let dir = match rest.peek() {
                        Some(next) if !next.starts_with('-') => {
                            crate::cli::absolute(cwd, Path::new(rest.next().unwrap()))
                        }
                        _ => cwd.to_path_buf(),
                    };
                    if !dir.is_dir() {
                        return Some(Err(format!("{} bukan folder.", dir.display())));
                    }
                    out.project = Some(dir);
                }
                _ => {
                    return Some(Err(format!(
                        "opsi tak dikenal: {arg} (yang ada: --project [folder], -y)"
                    )));
                }
            }
        }
        Some(Ok(out))
    }
}

/// `sawit skills …`: shows what it touches, asks (unless `-y`), runs, and lists every path it
/// changed. Exit code 0, or 1 when something failed or nothing was asked.
pub async fn bibit(args: BibitArgs, paths: &AppPaths) -> i32 {
    let ink = Ink::stdout();
    let removing = args.action == BibitAction::Remove;
    let server = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .map(|exe| sawit_skills::server_beside(&exe))
        .unwrap_or_else(|_| PathBuf::from("sawit-mcp"));
    let scope = match &args.project {
        Some(dir) => Scope::Project(dir.clone()),
        None => Scope::Global,
    };
    ink.header(
        if removing {
            "🌱 Cabut Bibit Sawit (Sawit Skills)"
        } else {
            "🌱 Pasang Bibit Sawit (Sawit Skills)"
        },
        "skill Sawit, blok di file instruksi agen, dan server MCP Sawit",
    );
    let touches = match &scope {
        Scope::Global => global_touches(&server, removing, paths).await,
        Scope::Project(dir) => project_touches(dir, removing),
    };
    ink.section(if removing {
        "Yang dicabut"
    } else {
        "Yang ditanam"
    });
    for line in &touches {
        println!("  {line}");
    }
    println!();
    if !args.yes {
        let question = if removing {
            "Cabut Sawit Skills dari tempat di atas?"
        } else {
            "Tulis Sawit Skills ke tempat di atas?"
        };
        match confirm(question) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Tidak ada yang ditulis.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }
    let outcome = if removing {
        sawit_skills::remove(&scope, paths).await
    } else {
        sawit_skills::install(&scope, paths, &server).await
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(e) => {
            println!("  {} {e}", ink.red("✗"));
            return 1;
        }
    };
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    for line in outcome.lines(&home) {
        let line = match line.trim_start().chars().next() {
            Some('✓') => line.replacen('✓', &ink.green("✓"), 1),
            Some('✗') => line.replacen('✗', &ink.red("✗"), 1),
            Some('·') => ink.gray(&line),
            _ => line,
        };
        println!("  {line}");
    }
    ink.footer(&outcome.summary(removing));
    i32::from(outcome.failures() > 0)
}

/// What a global run touches: the agents' skills folders, their instruction files and the
/// clients' MCP configs.
async fn global_touches(server: &Path, removing: bool, paths: &AppPaths) -> Vec<String> {
    let server = server.to_path_buf();
    let state = tokio::task::spawn_blocking(move || sawit_skills::state(&server))
        .await
        .ok();
    let Some(state) = state else {
        return Vec::new();
    };
    let mut out = vec![format!(
        "sekarang: {}",
        match state.level {
            Level::NotInstalled => "belum terpasang",
            Level::Update => "ada yang perlu diperbarui",
            Level::Installed => "terpasang, versi Sawit ini",
        }
    )];
    for (label, dir, _) in &state.skills {
        out.push(format!("skills  {label}: {}", tilde(dir)));
    }
    for (file, _) in &state.files {
        out.push(format!("blok    {}: {}", file.label, tilde(&file.path)));
    }
    if removing {
        for (client, config) in sawit_skills::mcp_added(paths) {
            out.push(format!(
                "MCP     {client}: entri sawit di {} dicabut",
                tilde(&config)
            ));
        }
    } else {
        for client in &state.mcp_missing {
            out.push(format!("MCP     {client}: server sawit ditambahkan"));
        }
    }
    out
}

/// What a project run touches: its instruction file(s).
fn project_touches(dir: &Path, removing: bool) -> Vec<String> {
    let found = if removing {
        project::files_with_block(dir)
    } else {
        project::files_for_put(dir)
    };
    if found.is_empty() {
        return vec!["tidak ada blok Sawit di proyek ini".into()];
    }
    found
        .iter()
        .map(|path| {
            let holds = match files::holds(path) {
                files::Holds::Nothing if !path.exists() => " (dibuat)",
                files::Holds::Current => " (sudah versi ini)",
                _ => "",
            };
            format!("blok    {}{holds}", tilde(path))
        })
        .collect()
}
