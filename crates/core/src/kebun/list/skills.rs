//! `sawit list skills`: the Skills page's packs and the agents that have them.

use crate::instances::AppPaths;
use crate::kebun::ink::{Ink, clip, tilde};

/// `sawit list skills`: the Skills page's packs, how each installs (the
/// skills CLI with the package manager picked last, a Claude Code plugin, a
/// clone and copy) and the agents that have it.
pub(super) async fn list_skills(ink: &Ink, paths: &AppPaths) {
    use crate::package_manager::{self, PackageManager, Use};
    use crate::skills::{self, Status};
    let available = package_manager::detect().await;
    let pm = package_manager::choose(
        &available,
        None,
        package_manager::remembered(paths),
        Use::Run,
    )
    .unwrap_or(PackageManager::Npm);
    let report = skills::report();
    ink.section("🌱 Bibit (skills)");
    if report.targets.is_empty() {
        ink.empty("belum ada agen dengan folder skills di Mac ini.");
    } else {
        let agents: Vec<String> = report
            .targets
            .iter()
            .map(|t| format!("{} ({})", t.label, tilde(&t.dir)))
            .collect();
        println!("  {} {}", ink.gray("agen:"), agents.join("; "));
    }
    for pack in skills::PACKS {
        println!(
            "  {:<28} {:<26} {}",
            pack.id,
            clip(pack.name, 26),
            ink.gray(&format!("{} · {}", pack.by, pack.repo))
        );
        let pad = " ".repeat(28);
        for method in pack.methods {
            println!("  {pad} {}", ink.gray(&skills::sample(pack, *method, pm)));
        }
        let have: Vec<String> = (0..report.targets.len())
            .filter_map(|t| {
                let label = &report.targets[t].label;
                match report.status(pack, t) {
                    Status::Installed { plugin: true } => Some(format!("{label} (plugin)")),
                    Status::Installed { plugin: false } => Some(label.clone()),
                    Status::Partial { have, of } => Some(format!("{label} ({have}/{of})")),
                    Status::Missing => None,
                }
            })
            .collect();
        if have.is_empty() {
            println!("  {pad} {}", ink.gray("belum di agen mana pun"));
        } else {
            println!("  {pad} {} {}", ink.green("✓"), have.join(", "));
        }
    }
    println!(
        "  {}",
        ink.gray(&format!(
            "Pasang dari halaman Skills di app (skills CLI lewat {}); agen pakai `install_skill`.",
            pm.runner()
        ))
    );
}
