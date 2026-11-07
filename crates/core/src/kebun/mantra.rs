//! `sawit prompts backup|restore` (`mantra cadangkan|pulihkan`): the prompt library on GitHub from
//! a terminal, as the Prompts sidebar's Back Up Now and Restore and `backup_prompts` /
//! `restore_prompts` do it, with Sawit's GitHub sign-in. Prompts changed on both sides are listed
//! and left alone unless named with `--overwrite <id>`. A restore changes the library on this
//! Mac, so it asks first (`-y` to skip); the running app reads it again.

use super::ink::{Ink, confirm};
use crate::app_control::{Store, notify_reload};
use crate::github::{Keychain, TokenStore};
use crate::instances::AppPaths;
use crate::prompt_sync::{self, BackupSettings, SyncOutcome};
use crate::prompts::Library;

/// What `sawit prompts` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MantraAction {
    /// `backup` (`cadangkan`, `push`).
    Backup,
    /// `restore` (`pulihkan`, `pull`).
    Restore,
}

/// `sawit prompts <backup|restore> [--repo <name>] [--overwrite <id>]… [-y]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MantraArgs {
    pub action: MantraAction,
    pub repo: Option<String>,
    pub overwrite: Vec<String>,
    pub yes: bool,
}

impl MantraArgs {
    /// The words after `prompts`; `None` when the first isn't an action.
    pub fn parse(args: &[&str]) -> Option<Result<MantraArgs, String>> {
        let action = match args.first().copied()? {
            "backup" | "cadangkan" | "push" => MantraAction::Backup,
            "restore" | "pulihkan" | "pull" => MantraAction::Restore,
            _ => return None,
        };
        let mut out = MantraArgs {
            action,
            repo: None,
            overwrite: Vec::new(),
            yes: false,
        };
        let mut rest = args[1..].iter();
        while let Some(&arg) = rest.next() {
            match arg {
                "-y" | "--yes" | "--ya" => out.yes = true,
                "--repo" => match rest.next() {
                    Some(r) => out.repo = Some(r.to_string()),
                    None => return Some(Err("--repo butuh nama repositori.".into())),
                },
                "--overwrite" | "--timpa" => match rest.next() {
                    Some(id) => out.overwrite.push(id.to_string()),
                    None => return Some(Err("--overwrite butuh id prompt.".into())),
                },
                a => {
                    return Some(Err(format!(
                        "opsi tak dikenal: {a} (yang ada: --repo <nama>, --overwrite <id>, -y)"
                    )));
                }
            }
        }
        Some(Ok(out))
    }
}

fn report(ink: &Ink, verb: &str, done: &SyncOutcome) {
    if done.created_repo {
        println!(
            "  {} Repositori privat {} dibuat.",
            ink.green("✓"),
            done.repo
        );
    }
    if done.held {
        ink.section(
            "Ditahan: baris ini mirip rahasia (ulangi dengan -y kalau memang mau dicadangkan)",
        );
        for line in &done.secrets {
            println!("  {} {line}", ink.yellow("!"));
        }
        return;
    }
    for id in &done.deleted {
        println!("  {} {id} (dihapus)", ink.green("✓"));
    }
    if done.synced.is_empty() && done.deleted.is_empty() {
        ink.empty(&format!(
            "Tak ada yang perlu di{verb}: {} sudah sama.",
            done.repo
        ));
    }
    for id in &done.synced {
        println!("  {} {id}", ink.green("✓"));
    }
    if !done.conflicts.is_empty() {
        ink.section("Berubah di dua sisi, dibiarkan (pakai --overwrite <id> setelah memilih)");
        for c in &done.conflicts {
            println!("  {} {} (id {})", ink.yellow("!"), c.title, c.id);
        }
    }
}

/// `sawit prompts …`. Exit 0, or 1 when signed out, failed or not confirmed.
pub async fn mantra(args: MantraArgs, paths: &AppPaths) -> i32 {
    let ink = Ink::stdout();
    let token = match Keychain::user().load() {
        Ok(Some(t)) if !t.trim().is_empty() => t,
        Ok(_) => {
            println!(
                "  {} Sawit belum masuk ke GitHub. Masuk dulu dari bilah judul Sawit.",
                ink.red("✗")
            );
            return 1;
        }
        Err(e) => {
            println!(
                "  {} Kunci GitHub Sawit tak terbaca dari Keychain: {e}",
                ink.red("✗")
            );
            return 1;
        }
    };
    let file = paths.root.join("prompts.json");
    let mut lib = match Library::load(&file) {
        Ok(lib) => lib,
        Err(e) => {
            println!("  {} {e}", ink.red("✗"));
            return 1;
        }
    };
    let mut settings = BackupSettings::load(&paths.root.join(prompt_sync::SETTINGS));
    if let Some(repo) = args
        .repo
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        settings.repo = repo.to_string();
    }
    let state = paths.root.join(prompt_sync::SYNC_STATE);
    ink.header(
        "📜 Mantra (pustaka prompt)",
        &format!("repositori {}", settings.repo),
    );
    match args.action {
        MantraAction::Backup => {
            let opts = prompt_sync::Options {
                allow_secrets: args.yes,
                dry_run: false,
            };
            match prompt_sync::back_up(&token, &lib, &settings, &state, &args.overwrite, opts).await
            {
                Ok(done) => {
                    report(&ink, "cadangkan", &done);
                    ink.footer(if done.held { "Ditahan." } else { "Selesai." });
                    i32::from(done.held)
                }
                Err(e) => {
                    println!("  {} {e}", ink.red("✗"));
                    1
                }
            }
        }
        MantraAction::Restore => {
            if !args.yes {
                match confirm("Pulihkan prompt dari GitHub ke pustaka di Mac ini?") {
                    Some(true) => {}
                    Some(false) => {
                        ink.footer("Batal. Pustakanya tidak disentuh.");
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
            match prompt_sync::restore(&token, &mut lib, &settings, &state, &args.overwrite, false)
                .await
            {
                Ok(done) => {
                    if !done.synced.is_empty() || !done.deleted.is_empty() {
                        if let Err(e) = lib.save(&file) {
                            println!("  {} {e}", ink.red("✗"));
                            return 1;
                        }
                        notify_reload(paths, Store::Prompts);
                    }
                    report(&ink, "pulihkan", &done);
                    ink.footer("Selesai.");
                    0
                }
                Err(e) => {
                    println!("  {} {e}", ink.red("✗"));
                    1
                }
            }
        }
    }
}
