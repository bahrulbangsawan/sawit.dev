//! `sawit list config`: the Config Files panel's shell startup files and coding agents' config
//! files.

use std::path::{Path, PathBuf};

use crate::kebun::ink::{Ink, tilde};

/// `sawit list config`: the Config Files panel, for the current folder's
/// project. Paths only: what a file holds (API keys, say) isn't shown.
pub(super) fn list_config(ink: &Ink, cwd: &Path) {
    use crate::{agent_config, shell_config};
    let login = shell_config::login_shell();
    let shells = shell_config::detect();
    ink.section(&match &login {
        Some(login) => format!("🐚 Startup shell (login: {login})"),
        None => "🐚 Startup shell".to_string(),
    });
    if shells.is_empty() {
        ink.empty("tidak ada berkas startup shell. Lahan bersih, tanpa dokumen AMDAL.");
    }
    for c in &shells {
        let read = shell_config::reads(login.as_deref(), c.shell);
        let mut notes = vec![c.note.to_string()];
        if !read {
            notes.push(format!(
                "tidak dibaca {}",
                login.as_deref().unwrap_or("shell login")
            ));
        }
        if !c.writable {
            notes.push("simpan butuh sandi admin".into());
        }
        let notes = notes.join(" · ");
        if read {
            println!("  {:<34} {}", tilde(&c.path), ink.gray(&notes));
        } else {
            // Dimmed, like the panel's row: the login shell never reads it.
            println!("  {}", ink.gray(&format!("{:<34} {notes}", tilde(&c.path))));
        }
    }
    // The current folder is the project, unless it is the home folder or `/`.
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let projects: Vec<PathBuf> = (cwd.parent().is_some() && Some(cwd) != home.as_deref())
        .then(|| cwd.to_path_buf())
        .into_iter()
        .collect();
    let agents = agent_config::detect(&projects);
    ink.section("🤖 Agen AI (prioritas tertinggi duluan)");
    if agents.is_empty() {
        ink.empty("tidak ada agen AI terpasang. Kebun ini masih dikerjakan tangan manusia.");
    }
    for (client, profile, files) in agent_config::groups(&agents) {
        println!("  {}", agent_config::group_title(client, profile));
        for f in files {
            // A project's file by the project's name: `app/CLAUDE.md`.
            let name = match &f.project {
                Some(project) => format!(
                    "{}/{}",
                    project
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    f.path.strip_prefix(project).unwrap_or(&f.path).display()
                ),
                None => tilde(&f.path),
            };
            let mut notes = format!("{} · {}", f.layer.label(), f.note);
            if !f.writable {
                notes.push_str(" · simpan butuh sandi admin");
            }
            println!("    {:<40} {}", name, ink.gray(&notes));
        }
    }
    println!(
        "  {}",
        ink.gray("Buka dengan `sawit <path>`. Isinya tidak ditampilkan: bisa ada kunci API.")
    );
}
