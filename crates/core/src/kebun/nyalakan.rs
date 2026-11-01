//! `sawit nyalakan` (`start`): starts Sawit's instances, and with `--ulang` restarts the
//! running ones.

use crate::control::{self, Preflight, StartOutcome};
use crate::instances::{AppPaths, Instance, Registry};
use crate::pgctl::PgCtlStatus;

use super::args::NyalakanArgs;
use super::ink::{Ink, confirm};

/// `sawit nyalakan …`: starts Sawit instances as their Start button and the `start_instance` tool
/// do, never over a port another process holds (`Preflight::Check`): that one's report says who
/// holds it, and the exit code is 1. No names: every stopped instance. `--ulang` restarts the
/// running ones too, as `restart_instance` does, which drops their connections, so it asks first
/// as `bakar instances` does (`-y` doesn't; without a terminal it needs `-y`).
pub async fn nyalakan(args: NyalakanArgs, paths: &AppPaths) -> i32 {
    let ink = Ink::stdout();
    ink.header(
        if args.again {
            "🔥 Nyalakan Ulang Kebun (instances)"
        } else {
            "🔥 Nyalakan Kebun (instances)"
        },
        if args.again {
            "pg_ctl stop lalu start, port diperiksa dulu: lahan orang tidak diserobot (kali ini)"
        } else {
            "pg_ctl start, port diperiksa dulu: lahan orang tidak diserobot (kali ini)"
        },
    );
    let registry = match Registry::load(&paths.instances_file()) {
        Ok(registry) => registry,
        Err(e) => {
            println!("  {} {e}", ink.red("✗"));
            return 1;
        }
    };
    if let Some(unknown) = args.names.iter().find(|n| registry.get(n).is_none()) {
        println!(
            "  {} Kebun “{unknown}” tidak terdaftar. Ilegal, ya? Lihat `sawit list instances`.",
            ink.red("✗")
        );
        return 1;
    }
    let mut running = Vec::new();
    let mut stopped = Vec::new();
    for inst in &registry.instances {
        if !args.names.is_empty() && !args.names.contains(&inst.name) {
            continue;
        }
        // One whose status can't be read is tried: starting it says why it can't.
        match control::status(inst).await {
            Ok(PgCtlStatus::Running { pid }) => running.push((inst, pid)),
            _ => stopped.push(inst),
        }
    }
    if running.is_empty() && stopped.is_empty() {
        ink.empty("belum ada kebun. Hutannya masih utuh, untuk sementara.");
        ink.footer("Tidak ada yang dinyalakan.");
        return 0;
    }
    // The question is only about the running ones: a no leaves them be, and the stopped ones
    // still start, as without `--ulang`.
    let mut again = args.again;
    if !args.again {
        for (inst, pid) in &running {
            ink.empty(&format!("{} sudah menyala, PID {pid}", inst.name));
        }
        if stopped.is_empty() {
            ink.footer("Semua sudah menyala. Asapnya sudah sampai negeri tetangga.");
            return 0;
        }
    } else if !running.is_empty() {
        ink.section(&format!(
            "Kebun menyala, dinyalakan ulang ({})",
            running.len()
        ));
        for (ix, (inst, pid)) in running.iter().enumerate() {
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
            match confirm(&format!(
                "Nyalakan ulang {} kebun di atas? Koneksinya putus sebentar.",
                running.len()
            )) {
                Some(true) => {}
                Some(false) if stopped.is_empty() => {
                    ink.footer("Batal. Kebun tetap menyala seperti tadi.");
                    return 0;
                }
                Some(false) => {
                    ink.empty("yang menyala dibiarkan menyala; yang padam tetap dinyalakan.");
                    again = false;
                }
                None => {
                    println!(
                        "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y untuk menyalakan ulang.",
                        ink.red("✗")
                    );
                    return 1;
                }
            }
        }
    }
    let restarts = running
        .iter()
        .filter(|_| again)
        .map(|(inst, _)| (*inst, true));
    let plan: Vec<(&Instance, bool)> = restarts
        .chain(stopped.iter().map(|inst| (*inst, false)))
        .collect();
    let mut failed = 0;
    for &(inst, restart) in &plan {
        let managed = registry.data_dirs();
        let outcome = match restart {
            true => control::restart(inst, paths, managed, Preflight::Check).await,
            false => control::start(inst, paths, managed, Preflight::Check).await,
        };
        match outcome {
            Ok(StartOutcome::Started) => println!(
                "  {} {} menyala  {}",
                ink.green("✓"),
                inst.name,
                ink.gray(&format!(
                    "postgres://postgres@localhost:{}/postgres",
                    inst.port
                ))
            ),
            Ok(StartOutcome::AlreadyRunning { pid }) => {
                ink.empty(&format!("{} sudah menyala, PID {pid}", inst.name));
            }
            Ok(StartOutcome::Blocked(report)) => {
                failed += 1;
                println!(
                    "  {} {} tidak dinyalakan: lahan :{} sudah ada yang menggarap.",
                    ink.red("✗"),
                    inst.name,
                    inst.port
                );
                for line in report.explain().lines() {
                    println!("      {line}");
                }
                println!(
                    "      {}",
                    ink.gray(
                        "Lihat siapa di sana: `sawit list ports`. Padamkan dulu, atau pakai Start Anyway di Sawit."
                    )
                );
            }
            Err(e) => {
                failed += 1;
                println!("  {} {}: {e}", ink.red("✗"), inst.name);
            }
        }
    }
    println!();
    ink.kv(
        "Menyala",
        &format!("{} / {} kebun", plan.len() - failed, plan.len()),
    );
    ink.footer(if failed == 0 {
        "Selesai. Kebun menyala; asapnya urusan musim depan."
    } else {
        "Sebagian tidak menyala; lihat di atas."
    });
    i32::from(failed > 0)
}
