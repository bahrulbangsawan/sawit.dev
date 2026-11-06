//! `sawit permissions` (`izin`): the Permissions page from a terminal. `list` shows Claude Code's
//! rules in each settings file, Sawit's tools that need confirmation and what the rules say of
//! them, and the other agents' settings; `add`, `remove` and `move` change one rule in one file,
//! showing the diff and asking first (`-y` to skip), with the same plan, backup and save as the
//! page and `edit_permission` ([`crate::permissions`]).

use std::path::{Path, PathBuf};

use super::ink::{Ink, confirm, tilde};
use crate::permissions::{self as perms, Bucket, Change};

/// What `sawit permissions` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IzinAction {
    /// `list` (`lihat`, `ls`), and the word alone.
    List,
    /// `add <allow|ask|deny> <rule>` (`tambah`).
    Add(Bucket, String),
    /// `remove <rule>` (`hapus`, `rm`).
    Remove(String),
    /// `move <rule> <allow|ask|deny>` (`pindah`, `mv`).
    Move(String, Bucket),
}

/// `sawit permissions [action] [--file user|user:<profile>|project|local] [--project <dir>] [-y]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IzinArgs {
    pub action: IzinAction,
    /// `--file`: which settings file (default `user`).
    pub file: String,
    /// `--project`: the project whose `project` and `local` files count (default: this folder).
    pub project: PathBuf,
    pub yes: bool,
}

fn bucket(word: &str) -> Result<Bucket, String> {
    match word {
        "izinkan" | "boleh" => Ok(Bucket::Allow),
        "tanya" => Ok(Bucket::Ask),
        "tolak" | "larang" => Ok(Bucket::Deny),
        w => Bucket::parse(w).ok_or_else(|| {
            format!("“{w}” bukan daftar aturan: allow (izinkan), ask (tanya) atau deny (tolak).")
        }),
    }
}

impl IzinArgs {
    /// The words after `permissions`.
    pub fn parse(args: &[&str], cwd: &Path) -> Result<IzinArgs, String> {
        let mut words: Vec<&str> = Vec::new();
        let mut out = IzinArgs {
            action: IzinAction::List,
            file: "user".into(),
            project: cwd.to_path_buf(),
            yes: false,
        };
        let mut rest = args.iter().peekable();
        while let Some(&arg) = rest.next() {
            match arg {
                "-y" | "--yes" | "--ya" => out.yes = true,
                "--file" | "--berkas" => match rest.next() {
                    Some(f) => out.file = f.to_string(),
                    None => {
                        return Err(
                            "--file butuh nama: user, user:<profil>, project atau local.".into(),
                        );
                    }
                },
                "--project" | "--proyek" => {
                    let dir = match rest.next() {
                        Some(d) => crate::cli::absolute(cwd, Path::new(d)),
                        None => return Err("--project butuh folder.".into()),
                    };
                    if !dir.is_dir() {
                        return Err(format!("{} bukan folder.", dir.display()));
                    }
                    out.project = dir;
                }
                a if a.starts_with('-') && a.len() > 1 => {
                    return Err(format!(
                        "opsi tak dikenal: {a} (yang ada: --file, --project, -y)"
                    ));
                }
                a => words.push(a),
            }
        }
        out.action = match words.as_slice() {
            [] | ["list" | "lihat" | "ls"] => IzinAction::List,
            ["add" | "tambah", list, rule] => IzinAction::Add(bucket(list)?, rule.to_string()),
            ["remove" | "hapus" | "rm", rule] => IzinAction::Remove(rule.to_string()),
            ["move" | "pindah" | "mv", rule, list] => {
                IzinAction::Move(rule.to_string(), bucket(list)?)
            }
            ["add" | "tambah", ..] => {
                return Err("pakai: sawit permissions add <allow|ask|deny> \"<aturan>\"".into());
            }
            ["remove" | "hapus" | "rm", ..] => {
                return Err("pakai: sawit permissions remove \"<aturan>\"".into());
            }
            ["move" | "pindah" | "mv", ..] => {
                return Err("pakai: sawit permissions move \"<aturan>\" <allow|ask|deny>".into());
            }
            [word, ..] => {
                return Err(format!(
                    "“{word}” bukan perintah izin: list, add, remove atau move."
                ));
            }
        };
        Ok(out)
    }
}

fn files_for(project: &Path) -> Vec<perms::RulesFile> {
    perms::claude_files(
        &perms::claude_profiles(),
        &[project.to_path_buf()],
        Path::new(perms::MANAGED),
    )
}

/// `sawit permissions …`. Exit 0, or 1 when the change was refused, failed or not confirmed.
pub fn izin(args: IzinArgs) -> i32 {
    let ink = Ink::stdout();
    let files = files_for(&args.project);
    let change = match &args.action {
        IzinAction::List => return list(&ink, &files, &args.project),
        IzinAction::Add(to, rule) => Change::Add {
            rule: rule.clone(),
            to: *to,
        },
        IzinAction::Remove(rule) => Change::Remove { rule: rule.clone() },
        IzinAction::Move(rule, to) => Change::Move {
            rule: rule.clone(),
            to: *to,
        },
    };
    if args.file.eq_ignore_ascii_case("managed") {
        println!(
            "  {} Pengaturan managed milik organisasimu: Sawit tidak mengubahnya.",
            ink.red("✗")
        );
        return 1;
    }
    let Some(file) = perms::find_file(&files, &args.file, Some(&args.project)) else {
        let known: Vec<String> = files.iter().map(|f| f.scope.id()).collect();
        println!(
            "  {} Tidak ada berkas “{}”. Yang ada: {}.",
            ink.red("✗"),
            args.file,
            known.join(", ")
        );
        return 1;
    };
    if let Some(problem) = &file.problem {
        println!(
            "  {} {} tak bisa diubah: {problem}",
            ink.red("✗"),
            tilde(&file.path)
        );
        return 1;
    }
    let plan = match perms::plan(&file.path, &change) {
        Ok(plan) => plan,
        Err(e) => {
            println!("  {} {e}", ink.red("✗"));
            return 1;
        }
    };
    ink.header(
        "🛂 Izin Claude Code",
        &format!("{} · {}", change.describe(), tilde(&file.path)),
    );
    for line in plan.diff.lines().skip(2) {
        let shown = match line.chars().next() {
            Some('+') => ink.green(line),
            Some('-') => ink.red(line),
            _ => ink.gray(line),
        };
        println!("  {shown}");
    }
    println!();
    if !args.yes {
        match confirm("Simpan perubahan ini? (cadangannya disimpan di sebelahnya)") {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Berkasnya tidak disentuh.");
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
    if let Err(e) = perms::save(&plan) {
        println!("  {} {e}", ink.red("✗"));
        return 1;
    }
    ink.footer(&if plan.before.is_empty() {
        format!("Tersimpan: {} dibuat.", tilde(&file.path))
    } else {
        format!(
            "Tersimpan. Versi lamanya di {}.",
            tilde(&perms::backup_path(&file.path))
        )
    });
    0
}

fn list(ink: &Ink, files: &[perms::RulesFile], project: &Path) -> i32 {
    ink.header("🛂 Izin agen", &format!("proyek {}", tilde(project)));
    ink.section("Claude Code (Deny, lalu Ask, lalu Allow)");
    for f in files {
        let mut title = format!("{} · {}", f.scope.label(), tilde(&f.path));
        if !f.exists {
            title.push_str(" (belum ada)");
        }
        println!("  {} {}", ink.yellow(&f.scope.id()), ink.gray(&title));
        if let Some(problem) = &f.problem {
            println!("      {} {problem}", ink.red("✗"));
            continue;
        }
        if f.rules.is_empty() {
            println!("      {}", ink.gray("—"));
        }
        for b in Bucket::ALL {
            for r in f.rules.get(b) {
                println!("      {:<6} {r}", b.key());
            }
        }
    }
    println!();
    ink.section("Alat Sawit yang butuh confirm=true");
    for t in perms::sawit_tool_states(files) {
        let state = match &t.rule {
            Some((b, rule, file)) => format!("{} ({rule}, {file})", b.key()),
            None => "ask (bawaan Claude Code)".into(),
        };
        println!("  {:<24} {}", t.tool, ink.gray(&state));
    }
    let others = perms::other_agents(&[project.to_path_buf()]);
    if !others.is_empty() {
        println!();
        ink.section("Agen lain (hanya dibaca)");
        for o in others {
            println!("  {} {}", o.agent, ink.gray(&tilde(&o.path)));
            for l in o.lines {
                println!("      {l}");
            }
        }
    }
    ink.footer(
        "Ubah: sawit permissions add <allow|ask|deny> \"<aturan>\" [--file user|project|local]",
    );
    0
}
