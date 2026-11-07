//! `sawit shell` (`cangkang`, the palm kernel's shell): the shell files' sync with GitHub from a
//! terminal, as the Config Files panel and `shell_files_sync` do it. `status` says how each file
//! stands, `upload` sends the files named to the private `sawit-shell-config` repository, and
//! `download` puts GitHub's version of the files named here, each backed up beside itself first.
//! Both ask first (`-y` to skip), refuse files changed on both sides unless `--overwrite`, and an
//! upload stops on lines that look like secrets unless `--allow-secrets`. Only the user can sign
//! in to GitHub, in the app; this uses that sign-in.

use super::ink::{Ink, confirm};
use crate::github::{Keychain, TokenStore};
use crate::instances::AppPaths;
use crate::shell_config::{self, Direction, FileSync};

/// What `sawit shell` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CangkangAction {
    /// `status` (`cek`), and the word alone.
    Status,
    /// `upload` (`unggah`, `push`).
    Upload,
    /// `download` (`unduh`, `pull`).
    Download,
}

/// `sawit shell [status|upload|download] [files…] [--overwrite] [--allow-secrets] [-y]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CangkangArgs {
    pub action: CangkangAction,
    /// The files, by their path under the home folder (`.zshrc`).
    pub files: Vec<String>,
    pub overwrite: bool,
    pub allow_secrets: bool,
    pub yes: bool,
}

impl CangkangArgs {
    /// The words after `shell`; `None` when the first isn't one of its actions, so `sawit shell`
    /// alone still opens a folder named shell… unless it's empty, which is the status.
    pub fn parse(args: &[&str]) -> Option<Result<CangkangArgs, String>> {
        let action = match args.first().copied() {
            None | Some("status" | "cek") => CangkangAction::Status,
            Some("upload" | "unggah" | "push") => CangkangAction::Upload,
            Some("download" | "unduh" | "pull") => CangkangAction::Download,
            Some(_) => return None,
        };
        let mut out = CangkangArgs {
            action,
            files: Vec::new(),
            overwrite: false,
            allow_secrets: false,
            yes: false,
        };
        for &arg in args.iter().skip(1) {
            match arg {
                "-y" | "--yes" | "--ya" => out.yes = true,
                "--overwrite" | "--timpa" => out.overwrite = true,
                "--allow-secrets" => out.allow_secrets = true,
                a if a.starts_with('-') => {
                    return Some(Err(format!(
                        "opsi tak dikenal: {a} (yang ada: --overwrite, --allow-secrets, -y)"
                    )));
                }
                a => out.files.push(a.trim_start_matches("~/").to_string()),
            }
        }
        if action == CangkangAction::Status && !out.files.is_empty() {
            return Some(Err("`sawit shell status` tidak butuh nama berkas.".into()));
        }
        if action != CangkangAction::Status && out.files.is_empty() {
            return Some(Err(
                "Sebut berkasnya: sawit shell upload .zshrc (lihat dulu: sawit shell status)."
                    .into(),
            ));
        }
        Some(Ok(out))
    }
}

/// `sawit shell …`. Exit 0, or 1 when signed out, refused, failed or not confirmed.
pub async fn cangkang(args: CangkangArgs, paths: &AppPaths) -> i32 {
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
    let state = paths.root.join(shell_config::SYNC_STATE);
    let status = match shell_config::sync_status(&token, &state).await {
        Ok(s) => s,
        Err(e) => {
            println!("  {} {e}", ink.red("✗"));
            return 1;
        }
    };
    ink.header(
        "🐚 Cangkang (berkas shell)",
        &format!("github.com/{}/{}", status.login, shell_config::BACKUP_REPO),
    );
    if args.action == CangkangAction::Status {
        if status.backup.is_none() {
            ink.empty("Repositorinya belum ada: buat dengan Save Shell Files to GitHub di panel Config Files.");
        }
        for f in &status.files {
            println!("  {:<22} {}", f.rel, ink.gray(f.state.label()));
        }
        ink.footer("Kirim: sawit shell upload <berkas> · Ambil: sawit shell download <berkas>");
        return 0;
    }
    let Some(backup) = &status.backup else {
        println!(
            "  {} github.com/{}/{} belum ada: buat dulu dari panel Config Files di Sawit.",
            ink.red("✗"),
            status.login,
            shell_config::BACKUP_REPO
        );
        return 1;
    };
    let way = match args.action {
        CangkangAction::Upload => Direction::Upload,
        _ => Direction::Download,
    };
    let files = match shell_config::pick_files(&status, &args.files, args.overwrite, way) {
        Ok(files) => files,
        Err(e) => {
            println!(
                "  {} {}",
                ink.red("✗"),
                e.replace("(overwrite)", "(--overwrite)")
            );
            return 1;
        }
    };
    if way == Direction::Upload {
        let secrets = shell_config::secret_lines(&files);
        if !secrets.is_empty() && !args.allow_secrets {
            println!(
                "  {} Baris ini mirip rahasia, jadi tidak ada yang dikirim:",
                ink.red("✗")
            );
            for s in secrets {
                println!("      {s}");
            }
            ink.footer("Kalau memang mau dikirim ke repositori privatmu: --allow-secrets");
            return 1;
        }
    }
    for f in &files {
        println!("  {:<22} {}", f.rel, ink.gray(f.state.label()));
    }
    println!();
    if !args.yes {
        let question = match way {
            Direction::Upload => "Kirim berkas di atas ke GitHub?",
            Direction::Download => "Timpa berkas di atas dengan versi GitHub? (dicadangkan dulu)",
        };
        match confirm(question) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Tidak ada yang berpindah.");
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
    match way {
        Direction::Upload => {
            let rels: Vec<String> = files.iter().map(|f| f.rel.clone()).collect();
            let host = shell_config::computer_name();
            match shell_config::upload(&token, backup, &rels, &state, &host).await {
                Ok(done) if done.changed => {
                    ink.footer(&format!("Terkirim ke {}.", backup.repo.html_url));
                    0
                }
                Ok(_) => {
                    ink.footer("GitHub sudah punya versi yang sama: tak ada yang dikirim.");
                    0
                }
                Err(e) => {
                    println!("  {} {e}", ink.red("✗"));
                    1
                }
            }
        }
        Direction::Download => {
            let owned: Vec<FileSync> = files.into_iter().cloned().collect();
            let deletes = deleted_on_github(&owned);
            let contents = match shell_config::fetch_remote(&token, backup, &owned).await {
                Ok(c) => c,
                Err(e) => {
                    println!("  {} {e}", ink.red("✗"));
                    return 1;
                }
            };
            let known = shell_config::known_files_here();
            match shell_config::apply_download_with(
                &known,
                &contents,
                &deletes,
                &state,
                &backup.tree.commit,
                crate::github::now(),
            ) {
                Ok(done) => {
                    for r in &done.replaced {
                        match &r.backup {
                            Some(b) => println!(
                                "  {} {} (cadangan: {})",
                                ink.green("✓"),
                                super::ink::tilde(&r.path),
                                super::ink::tilde(b)
                            ),
                            None => println!(
                                "  {} {} (baru)",
                                ink.green("✓"),
                                super::ink::tilde(&r.path)
                            ),
                        }
                    }
                    ink.footer("Shell yang terbuka membaca berkas baru setelah `source` atau dibuka ulang.");
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

/// The files among `files` deleted on GitHub since the last sync, which a download deletes here.
fn deleted_on_github(files: &[FileSync]) -> Vec<String> {
    files
        .iter()
        .filter(|f| f.state == crate::github::FileState::DeletedOnGitHub)
        .map(|f| f.rel.clone())
        .collect()
}
