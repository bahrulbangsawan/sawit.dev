//! `sawit list explorer`: Cloudflare's Local Explorers, and the Workers of this folder's
//! project.

use std::path::Path;

use crate::kebun::ink::{Ink, clip, tilde};
use crate::probe;

/// `sawit list explorer`: the Local Explorers Workers dev servers serve (`wrangler dev`, `vite`
/// with the Cloudflare plugin, `cf dev`), as `sawit-mcp`'s `local_explorer` finds them, and the
/// Workers of the project in `cwd` with what their dev tool needs for one.
pub(super) async fn list_explorer(ink: &Ink, cwd: &Path) {
    use crate::cloudflare::{self, explorer};
    ink.section("🧭 Local Explorer (Cloudflare)");
    let explorers = match probe::listening_jobs().await {
        Ok(jobs) => explorer::find(&jobs).await,
        Err(e) => {
            println!("  {}", ink.red(&format!("lsof gagal: {e}")));
            Vec::new()
        }
    };
    if explorers.is_empty() {
        ink.empty(
            "tidak ada yang menyala. Jalankan dev server Workers-nya (wrangler dev, vite, cf dev).",
        );
    }
    for e in &explorers {
        println!(
            "  {:<7} {}  {}  {}",
            format!(":{}", e.port),
            ink.green(&e.url()),
            clip(&e.label, 40),
            ink.gray(&format!("PID {}", e.pid))
        );
        if let Some(dir) = &e.cwd {
            println!(
                "  {:<7} {}",
                "",
                ink.gray(&format!("└ {}", tilde(Path::new(dir))))
            );
        }
    }
    let workers = cloudflare::detect_project(cwd);
    if workers.is_empty() {
        return;
    }
    ink.section("☁️ Workers di folder ini");
    for w in &workers {
        let tool = match &w.version {
            Some(v) => format!("{} {v}", w.tool.name()),
            None => w.tool.name().to_string(),
        };
        let rel = w
            .root
            .strip_prefix(cwd)
            .ok()
            .filter(|r| !r.as_os_str().is_empty())
            .map(|r| format!("{}/", r.display()))
            .unwrap_or_default();
        let bindings = w
            .bindings
            .iter()
            .map(|b| format!("{} {}", b.kind.label(), b.binding))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "  {:<22} {}  {}",
            clip(w.name.as_deref().unwrap_or("(tanpa nama)"), 22),
            ink.gray(&format!("{rel}{} · {tool}", w.config_name())),
            bindings
        );
        let running = explorers.iter().any(|e| e.runs_in(&w.root));
        if let Some(note) = cloudflare::requirement_note(w, cwd) {
            println!("  {:<22} {}", "", ink.yellow(&format!("└ {note}")));
        }
        if !running && let Some(script) = cloudflare::dev_scripts(w).first() {
            println!(
                "  {:<22} {}",
                "",
                ink.gray(&format!("└ nyalakan: {} ({})", script.name, script.body))
            );
        }
    }
}
