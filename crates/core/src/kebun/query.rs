//! `sawit <db> [table] ["sql"] [-y]`: runs the SQL, read-only first, and prints what it
//! harvests.

use std::io::Write;
use std::path::Path;

use crate::control;
use crate::db::{self, ConnProfile, format_table};
use crate::instances::{AppPaths, Registry};
use crate::pgctl::PgCtlStatus;

use super::args::QueryArgs;
use super::find::find;
use super::ink::{Ink, clip, confirm_on_stderr};
use super::sql::plan_sql;

/// Rows printed per result set at most.
const MAX_ROWS: usize = 10_000;

/// `sawit <db> [table] ["sql"] [-y]`. `None` when `db` isn't a database and
/// the arguments could just as well be files to open. The SQL is tried
/// read-only first, as the `query` MCP tool runs it; SQL that writes is
/// shown, and runs once the user says yes (or with `-y`).
pub async fn query(args: QueryArgs, paths: &AppPaths, cwd: &Path) -> Option<i32> {
    let ink = Ink::stderr();
    let kebun = match find(&args.db, paths, cwd).await {
        Ok(Some(kebun)) => kebun,
        Ok(None) if !args.explicit() => return None,
        Ok(None) => {
            eprintln!(
                "{} Kebun “{}” tidak ada di peta konsesi mana pun.\n   Sudah dicek: instance Sawit, koneksi tersimpan, .env, dan Postgres lokal yang menyala.\n   Mungkin lahannya belum dibuka, atau sudah jadi abu. Lihat `sawit list db`.",
                ink.red("🔥"),
                args.db
            );
            return Some(1);
        }
        Err(e) => {
            eprintln!("{} {e}", ink.red("🔥"));
            return Some(1);
        }
    };
    let p = &kebun.profile;
    eprintln!(
        "{}",
        ink.gray(&format!(
            "🌴 {} · {}:{} · {}",
            p.database, p.host, p.port, kebun.via
        ))
    );
    // Read-only first: SQL that writes shows itself and asks before it runs.
    let reader = match db::connect_read_only(p, kebun.password.as_deref()).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("{} Gagal masuk kebun {}: {e}", ink.red("🔥"), p.database);
            if let Some(name) = stopped_instance(p, paths).await {
                eprintln!("   Kebun {name} padam. Nyalakan dulu: `sawit nyalakan {name}`");
            }
            return Some(1);
        }
    };
    let sql = plan_sql(args.table.as_deref(), args.sql.as_deref());
    let why = match db::run_if_read_only(&reader, &sql, MAX_ROWS).await {
        Ok(db::ReadOnlyTry::Read(outcome)) => return Some(harvest(&outcome, &ink)),
        Ok(db::ReadOnlyTry::Writes(refusal)) => format!(
            "Pernyataan ini mengubah kebun; mode baca-saja menolaknya ({}).",
            refusal.lines().next().unwrap_or_default()
        ),
        Ok(db::ReadOnlyTry::Several(_)) => {
            "Ada beberapa pernyataan sekaligus, dan Sawit tidak bisa mencobanya baca-saja dulu."
                .into()
        }
        Err(e) => {
            eprintln!("{} Gagal panen: {e}", ink.red("🔥"));
            return Some(1);
        }
    };
    if !may_run_writes(&sql, &why, &p.database, args.yes, &ink) {
        return Some(1);
    }
    let writer = match db::connect(p, kebun.password.as_deref()).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("{} Gagal masuk kebun {}: {e}", ink.red("🔥"), p.database);
            return Some(1);
        }
    };
    match db::run_sql(&writer, &sql, MAX_ROWS).await {
        Ok(outcome) => Some(harvest(&outcome, &ink)),
        Err(e) => {
            eprintln!("{} Gagal panen: {e}", ink.red("🔥"));
            Some(1)
        }
    }
}

/// The name of the Sawit instance `profile` connects to, when it is one and it is stopped: a
/// connection that failed then needs `sawit nyalakan <name>`.
pub(super) async fn stopped_instance(profile: &ConnProfile, paths: &AppPaths) -> Option<String> {
    if !matches!(profile.host.as_str(), "localhost" | "127.0.0.1" | "::1") {
        return None;
    }
    let registry = Registry::load(&paths.instances_file()).ok()?;
    let inst = registry
        .instances
        .iter()
        .find(|inst| inst.port == profile.port)?;
    let stopped = matches!(control::status(inst).await, Ok(PgCtlStatus::Stopped));
    stopped.then(|| inst.name.clone())
}

/// Whether `sql`, which the read-only try refused (`why`), may run in `database` as it is: with
/// `-y`, or when the user says yes. The statement and why it asks come first, on stderr, where
/// `sawit <db>` says all but its tables. Without a terminal to ask on, and without `-y`, it may not.
fn may_run_writes(sql: &str, why: &str, database: &str, yes: bool, ink: &Ink) -> bool {
    eprintln!("{} {why}", ink.yellow("!"));
    for line in statement_preview(sql) {
        eprintln!("     {}", ink.gray(&line));
    }
    if yes {
        return true;
    }
    match confirm_on_stderr(&format!(
        "Jalankan di {database}? Lahan yang terbakar tidak tumbuh lagi."
    )) {
        Some(true) => true,
        Some(false) => {
            eprintln!("{}", ink.gray("🌾 Batal. Kebun tidak disentuh."));
            false
        }
        None => {
            eprintln!(
                "{} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau menjalankannya.",
                ink.red("🔥")
            );
            false
        }
    }
}

/// The lines of `sql` shown before `sawit <db>` asks to run it: the first 20 (each cut at 200
/// characters), then how many more there are.
pub(super) fn statement_preview(sql: &str) -> Vec<String> {
    const SHOWN: usize = 20;
    let lines: Vec<&str> = sql.trim().lines().collect();
    let mut shown: Vec<String> = lines
        .iter()
        .take(SHOWN)
        .map(|line| clip(line, 200))
        .collect();
    if lines.len() > SHOWN {
        shown.push(format!("… ({} baris lagi)", lines.len() - SHOWN));
    }
    shown
}

/// Prints the result sets of `outcome` on stdout, and its tally on stderr; the exit code.
fn harvest(outcome: &db::QueryOutcome, ink: &Ink) -> i32 {
    let mut out = std::io::stdout().lock();
    for (ix, set) in outcome.results.iter().enumerate() {
        let gap = if ix > 0 { "\n" } else { "" };
        let _ = writeln!(out, "{gap}{}", format_table(set));
    }
    let _ = out.flush();
    let rows: usize = outcome.results.iter().map(|s| s.rows.len()).sum();
    let mut summary = vec![format!("panen {rows} baris")];
    if let Some(&n) = outcome.affected.last() {
        summary.push(format!("{n} baris terdampak"));
    }
    if outcome.results.iter().any(|s| s.truncated) {
        summary.push(format!(
            "dibatasi {MAX_ROWS} baris, sisanya lahan gambut, jangan disentuh"
        ));
    }
    summary.push(format!("{} ms", outcome.duration.as_millis()));
    eprintln!("{}", ink.gray(&format!("🌾 {}", summary.join(" · "))));
    0
}
