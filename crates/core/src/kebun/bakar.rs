//! `sawit bakar ports|instances` ("burn"): stops dev servers on ports, or Sawit's instances.

use std::path::Path;
use std::time::Duration;

use crate::control;
use crate::instances::{AppPaths, Registry};
use crate::pgctl::PgCtlStatus;
use crate::ports::PortJob;
use crate::probe;

use super::args::{BakarArgs, BakarWhat};
use super::ink::{Ink, clip, confirm, tilde};

/// Command lines `bakar ports` never touches, even when they listen like a
/// dev server (same list as the `kill-dev` script).
pub const PROTECTED: &[&str] = &[
    "claude",
    "mcp-server",
    "language-server",
    "tsserver",
    "copilot",
    "code helper",
    "orca",
    "electron",
];

/// What `bakar ports` takes when no port is named: dev servers, never
/// databases, Docker, Sawit's instances or protected tools.
pub fn burnable(job: &PortJob, instance_ports: &[u16]) -> bool {
    job.dev
        && !job.is_docker()
        && !job.is_daemon()
        && !protected(job)
        && !job.ports.iter().any(|(p, _)| instance_ports.contains(p))
}

pub fn protected(job: &PortJob) -> bool {
    let cmd = job.cmdline.to_lowercase();
    PROTECTED.iter().any(|p| cmd.contains(p))
}

pub(super) fn ports_of(job: &PortJob) -> String {
    job.ports
        .iter()
        .map(|(p, _)| format!(":{p}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `sawit bakar …`.
pub async fn bakar(args: BakarArgs, paths: &AppPaths) -> i32 {
    match args.what {
        BakarWhat::Ports => bakar_ports(args, paths).await,
        BakarWhat::Instances => bakar_instances(args, paths).await,
    }
}

async fn bakar_ports(args: BakarArgs, paths: &AppPaths) -> i32 {
    let ink = Ink::stdout();
    ink.header(
        "🔥 Bakar Lahan (ports)",
        if args.targets.is_empty() {
            "hanya dev server. Database, Docker & proses penting dilindungi (kali ini)"
        } else {
            "port pilihan sendiri. Izinnya, seperti biasa, menyusul"
        },
    );
    let jobs = match probe::listening_jobs().await {
        Ok(jobs) => jobs,
        Err(e) => {
            println!("  {} lsof gagal: {e}", ink.red("✗"));
            return 1;
        }
    };
    let registry = Registry::load(&paths.instances_file()).unwrap_or_default();
    let instance_ports: Vec<u16> = registry.instances.iter().map(|i| i.port).collect();
    let mut victims: Vec<&PortJob> = Vec::new();
    if args.targets.is_empty() {
        victims.extend(jobs.iter().filter(|j| burnable(j, &instance_ports)));
    }
    for port in args.targets.iter().filter_map(|t| t.parse::<u16>().ok()) {
        let Some(job) = jobs
            .iter()
            .find(|j| j.ports.iter().any(|(p, _)| *p == port))
        else {
            ink.warn(&format!(":{port} tidak ada api di sana."));
            continue;
        };
        if job.is_docker() {
            ink.warn(&format!(
                ":{port} milik Docker. Padamkan dari Docker; membakar lahan tetangga itu urusan korporasi."
            ));
        } else if protected(job) {
            ink.warn(&format!(":{port} ({}) dilindungi.", job.label));
        } else if instance_ports.contains(&port) {
            ink.warn(&format!(
                ":{port} itu instance Sawit. Pakai `sawit bakar instances`."
            ));
        } else if !victims.iter().any(|v| v.root == job.root) {
            victims.push(job);
        }
    }
    if victims.is_empty() {
        ink.empty("tidak ada titik api. Mungkin sudah dibakar duluan oleh oknum.");
        ink.footer("Tidak ada yang dibakar.");
        return 0;
    }
    ink.section(&format!("Titik api ({})", victims.len()));
    for (ix, job) in victims.iter().enumerate() {
        let kids = job.pids.len() - 1;
        let family = if kids > 0 {
            format!(" (+{kids} anak)")
        } else {
            String::new()
        };
        println!(
            "  {} {:<14} {}  {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            ports_of(job),
            clip(&job.label, 60),
            ink.gray(&format!("PID {}{family}", job.root))
        );
        if let Some(cwd) = &job.cwd {
            println!(
                "       {}",
                ink.gray(&format!("└ {}", tilde(Path::new(cwd))))
            );
        }
    }
    println!();
    if !args.yes {
        match confirm(&format!(
            "Bakar {} lahan di atas? Asapnya tanggung sendiri.",
            victims.len()
        )) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Lahan selamat, untuk musim ini.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau membakar.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }

    let pids: Vec<u32> = victims
        .iter()
        .flat_map(|j| j.pids.iter().copied())
        .collect();
    let mut refused = Vec::new();
    for &pid in &pids {
        if let Err(e) = probe::terminate(pid, false) {
            refused.push(e);
        }
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let survivors: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|&p| probe::is_alive(p))
        .collect();
    if !survivors.is_empty() {
        ink.warn(&format!(
            "{} proses tidak padam disiram SIGTERM. Kirim water bombing (SIGKILL).",
            survivors.len()
        ));
        for &pid in &survivors {
            let _ = probe::terminate(pid, true);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let burned = victims
        .iter()
        .filter(|j| j.pids.iter().all(|&p| !probe::is_alive(p)))
        .count();
    println!();
    ink.kv("Hangus", &format!("{burned} / {} lahan", victims.len()));
    if burned < victims.len() {
        if let Some(e) = refused.first() {
            ink.warn(e);
        }
        ink.warn("Sisanya milik user lain atau butuh sudo. Pelakunya, seperti biasa, tak terjangkau hukum.");
    }
    ink.footer("Selesai. Asap menyebar, status resmi: “terkendali”.");
    i32::from(burned < victims.len())
}

async fn bakar_instances(args: BakarArgs, paths: &AppPaths) -> i32 {
    let ink = Ink::stdout();
    ink.header(
        "🧯 Padamkan Kebun (instances)",
        "pg_ctl stop -m fast. Datanya tidak ikut terbakar, tenang",
    );
    let registry = match Registry::load(&paths.instances_file()) {
        Ok(registry) => registry,
        Err(e) => {
            println!("  {} {e}", ink.red("✗"));
            return 1;
        }
    };
    if let Some(unknown) = args.targets.iter().find(|t| registry.get(t).is_none()) {
        println!(
            "  {} Kebun “{unknown}” tidak terdaftar. Ilegal, ya? Lihat `sawit list instances`.",
            ink.red("✗")
        );
        return 1;
    }
    let mut burning = Vec::new();
    for inst in &registry.instances {
        if !args.targets.is_empty() && !args.targets.contains(&inst.name) {
            continue;
        }
        if let Ok(PgCtlStatus::Running { pid }) = control::status(inst).await {
            burning.push((inst, pid));
        }
    }
    if burning.is_empty() {
        ink.empty("semua kebun sudah padam. Hujan buatan berhasil, katanya.");
        ink.footer("Tidak ada yang dipadamkan.");
        return 0;
    }
    ink.section(&format!("Kebun menyala ({})", burning.len()));
    for (ix, (inst, pid)) in burning.iter().enumerate() {
        println!(
            "  {} {:<16} :{:<6} {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            inst.name,
            inst.port,
            ink.gray(&format!("PID {pid}"))
        );
    }
    println!();
    if !args.yes {
        match confirm(&format!("Padamkan {} kebun di atas?", burning.len())) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Kebun tetap menyala, asap tetap mengepul.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y untuk memadamkan.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }
    let mut out = 0;
    for (inst, _) in &burning {
        match control::stop(inst).await {
            Ok(()) => println!("  {} {} padam", ink.green("✓"), inst.name),
            Err(e) => {
                println!("  {} {}: {e}", ink.red("✗"), inst.name);
                out += 1;
            }
        }
    }
    println!();
    ink.kv(
        "Padam",
        &format!("{} / {} kebun", burning.len() - out, burning.len()),
    );
    ink.footer("Selesai. Datanya aman, tidak seperti hutannya.");
    i32::from(out > 0)
}
