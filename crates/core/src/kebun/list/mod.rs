//! `sawit list [what]`: instances, databases, ports, connections and tools, and the lists
//! shown only when asked.

mod config;
mod explorer;
mod mcp;
mod skills;

use std::path::Path;

use crate::control;
use crate::instances::{AppPaths, Registry};
use crate::pgctl::PgCtlStatus;
use crate::probe;

use super::args::ListWhat;
use super::bakar::{burnable, ports_of};
use super::find::{dotenv_places, local_kebuns, saved_places};
use super::ink::{Ink, clip, tilde};
use super::keys::list_keys;
use config::list_config;
use explorer::list_explorer;
use mcp::list_mcp;
use skills::list_skills;

/// `sawit list [what]`.
pub async fn list(what: ListWhat, paths: &AppPaths, cwd: &Path) -> i32 {
    let ink = Ink::stdout();
    let registry = Registry::load(&paths.instances_file()).unwrap_or_default();
    if what.shows(ListWhat::Instances) {
        ink.section("🌴 Kebun (instances)");
        if registry.instances.is_empty() {
            ink.empty("belum ada kebun. Hutannya masih utuh, untuk sementara.");
        }
        let mut padam = false;
        for inst in &registry.instances {
            let status = match control::status(inst).await {
                Ok(PgCtlStatus::Running { pid }) => ink.green(&format!("menyala (PID {pid})")),
                Ok(PgCtlStatus::Stopped) => {
                    padam = true;
                    ink.gray("padam")
                }
                Ok(other) => ink.yellow(&format!("{other:?}")),
                Err(e) => ink.red(&format!("error: {e}")),
            };
            println!(
                "  {:<16} :{:<6} {status}  {}",
                inst.name,
                inst.port,
                ink.gray(&format!(
                    "postgres://postgres@localhost:{}/postgres",
                    inst.port
                ))
            );
        }
        if padam {
            println!(
                "  {}",
                ink.gray("Yang padam dinyalakan lagi dengan `sawit nyalakan <nama>`.")
            );
        }
    }
    if what.shows(ListWhat::Databases) {
        ink.section("🌾 Blok tanam (databases)");
        let mut kebuns = local_kebuns(&registry).await;
        // Where each one goes, never its password: none is read.
        kebuns.extend(dotenv_places(cwd));
        kebuns.extend(saved_places(paths));
        if kebuns.is_empty() {
            ink.empty("tidak ada Postgres yang menyala. Udara bersih, ISPU hijau.");
        }
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for k in &kebuns {
            let place = format!("{}:{}", k.profile.host, k.profile.port);
            let key = match k.via.contains(&place) {
                true => k.via.clone(),
                false => format!("{} · {place}", k.via),
            };
            match groups.iter_mut().find(|(g, _)| *g == key) {
                Some((_, dbs)) => dbs.push(k.profile.database.clone()),
                None => groups.push((key, vec![k.profile.database.clone()])),
            }
        }
        for (place, dbs) in groups {
            println!("  {}  {}", dbs.join(", "), ink.gray(&place));
        }
    }
    if what.shows(ListWhat::Ports) {
        ink.section("🔥 Lahan terbuka (ports)");
        match probe::listening_jobs().await {
            Ok(jobs) if jobs.is_empty() => ink.empty("tidak ada port terbuka. Nihil titik api."),
            Ok(jobs) => {
                let instance_ports: Vec<u16> = registry.instances.iter().map(|i| i.port).collect();
                for job in &jobs {
                    let tag = if burnable(job, &instance_ports) {
                        ink.red("titik api")
                    } else if job.is_docker() {
                        ink.gray("docker")
                    } else if job.is_daemon() {
                        ink.gray("server")
                    } else {
                        ink.gray("aplikasi")
                    };
                    println!(
                        "  {:<14} {}  {}  {}",
                        ports_of(job),
                        clip(&job.label, 60),
                        ink.gray(&format!("PID {}", job.root)),
                        tag
                    );
                    if let Some(cwd) = &job.cwd {
                        println!(
                            "  {:<14} {}",
                            "",
                            ink.gray(&format!("└ {}", tilde(Path::new(cwd))))
                        );
                    }
                }
                println!(
                    "  {}",
                    ink.gray("“titik api” = yang dibakar oleh `sawit bakar ports`.")
                );
            }
            Err(e) => println!("  {}", ink.red(&format!("lsof gagal: {e}"))),
        }
    }
    if what.shows(ListWhat::Connections) {
        ink.section("📜 Konsesi (connections)");
        let profiles = saved_places(paths);
        if profiles.is_empty() {
            ink.empty("belum ada izin konsesi. Belum ada yang “diatur”.");
        }
        for k in profiles {
            println!("  {}  {}", k.profile.name, ink.gray(&k.profile.url()));
        }
    }
    if what.shows(ListWhat::Tools) {
        let (detection, latest) = futures::join!(
            crate::dev_tools::detect(),
            crate::dev_tools::latest_versions()
        );
        let found = detection.found;
        if let Some(problem) = &detection.problem {
            ink.warn(&format!(
                "Shell login tidak menjawab ({problem}): hanya alat di tempat biasanya yang terdeteksi."
            ));
        }
        for category in crate::dev_tools::Category::ALL {
            ink.section(&format!("🧰 {}", category.title()));
            for t in crate::dev_tools::TOOLS
                .iter()
                .filter(|t| t.category == category)
            {
                let state = match found.get(t.id) {
                    Some(f) => {
                        let installed = ink.green(&format!(
                            "terpasang{}",
                            f.version
                                .as_ref()
                                .map(|v| format!(" {v}"))
                                .unwrap_or_default()
                        ));
                        match (crate::dev_tools::newer(t.id, &found, &latest), f.outside) {
                            (Some(v), _) => {
                                format!("{installed} {}", ink.yellow(&format!("→ {v}")))
                            }
                            // Sawit neither upgrades nor removes it.
                            (None, Some(crate::dev_tools::Outside::Homebrew)) => {
                                format!("{installed} {}", ink.gray("(di luar Homebrew)"))
                            }
                            (None, Some(crate::dev_tools::Outside::InstallScript)) => {
                                format!("{installed} {}", ink.gray("(di luar skrip instalnya)"))
                            }
                            (None, None) => installed,
                        }
                    }
                    None => ink.gray("belum"),
                };
                let place = found
                    .get(t.id)
                    .map(|f| format!("  {}", ink.gray(&tilde(&f.path))))
                    .unwrap_or_default();
                println!("  {:<14} {:<22} {state}{place}", t.id, clip(t.name, 22));
            }
        }
        println!(
            "  {}",
            ink.gray("Pasang dengan `sawit pasang <id…>`; prasyaratnya ikut ditanam duluan.")
        );
        println!(
            "  {}",
            ink.gray("Yang bertanda → ada versi baru: `sawit pupuk` memupuk semuanya.")
        );
        println!(
            "  {}",
            ink.gray("Cabut dengan `sawit cabut <id…>`: hanya salinan dari installer-nya, Homebrew atau npm.")
        );
    }
    if what.shows(ListWhat::Mcp) {
        list_mcp(&ink);
    }
    if what.shows(ListWhat::Skills) {
        list_skills(&ink, paths).await;
    }
    if what.shows(ListWhat::Config) {
        list_config(&ink, cwd);
    }
    if what.shows(ListWhat::Explorer) {
        list_explorer(&ink, cwd).await;
    }
    if what.shows(ListWhat::Keys) {
        let code = list_keys(&ink, paths);
        if code != 0 {
            return code;
        }
    }
    println!();
    0
}
