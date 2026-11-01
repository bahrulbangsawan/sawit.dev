//! `sawit pasang|pupuk|cabut`: installs, upgrades and uninstalls the Dev Tools page's tools.

use super::args::{CabutArgs, PasangArgs, PupukArgs};
use super::ink::{Ink, clip, confirm, tilde};

/// `sawit pasang <tool…>`.
pub async fn pasang(args: PasangArgs) -> i32 {
    use crate::dev_tools;
    let ink = Ink::stdout();
    ink.header(
        "🌱 Pasang Bibit (tools)",
        "skrip instal resmi tiap alat (curl … | sh). Prasyarat ditanam duluan",
    );
    let detection = dev_tools::detect().await;
    if let Some(problem) = &detection.problem {
        // Which tools are missing is unknown: don't run an installer over a
        // copy that's there.
        println!(
            "  {} Shell login tidak menjawab ({problem}), jadi tak ada yang dipasang. Coba lagi.",
            ink.red("✗")
        );
        return 1;
    }
    let found = detection.found;
    for id in args.tools.iter().filter(|t| found.contains_key(t.as_str())) {
        ink.empty(&format!("{id} sudah terpasang"));
    }
    let wanted: Vec<&str> = args.tools.iter().map(|t| t.as_str()).collect();
    let plan: Vec<_> = dev_tools::plan(&wanted, &|id| found.contains_key(id))
        .into_iter()
        .filter(|t| !found.contains_key(t.id))
        .collect();
    if plan.is_empty() {
        ink.footer("Tidak ada yang perlu dipasang.");
        return 0;
    }
    ink.section(&format!("Bibit ({})", plan.len()));
    for (ix, t) in plan.iter().enumerate() {
        println!(
            "  {} {:<14} {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            t.id,
            ink.gray(t.install)
        );
    }
    println!();
    if !args.yes {
        match confirm(&format!("Jalankan {} skrip instal di atas?", plan.len())) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Lahan dibiarkan kosong.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau memasang.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }
    for tool in plan {
        if tool.interactive {
            // It asks for a password, which the installer can't read here.
            ink.warn(&format!(
                "{} minta kata sandi. Jalankan sendiri, lalu ulangi:\n      {}",
                tool.id, tool.install
            ));
            ink.footer("Berhenti di sini; yang berikutnya mungkin butuh dia.");
            return 1;
        }
        println!("  {} {}", ink.yellow("⏳"), tool.name);
        let gray = |line: String| println!("     {}", ink.gray(&clip(&line, 100)));
        match dev_tools::install(tool, gray).await {
            Ok(()) => println!("  {} {} terpasang", ink.green("✓"), tool.id),
            Err(e) => {
                println!("  {} {}: {e}", ink.red("✗"), tool.id);
                ink.footer("Gagal tumbuh. Sisanya tidak ditanam.");
                return 1;
            }
        }
    }
    ink.footer("Selesai. Buka terminal baru supaya PATH-nya ikut segar.");
    0
}

/// Why `sawit pupuk` leaves alone a tool it was asked to upgrade: an app
/// installed outside what upgrades it (PureMac from its disk image), one
/// already up to date, or one not installed.
pub(super) fn pupuk_skip(
    id: &str,
    found: &std::collections::HashMap<&'static str, crate::dev_tools::Found>,
) -> String {
    use crate::dev_tools::{Found, Outside};
    match found.get(id) {
        Some(Found {
            outside: Some(outside),
            path,
            ..
        }) => format!(
            "{id} di {} dipasang di luar {}: tidak dipupuk Sawit",
            tilde(path),
            match outside {
                Outside::Homebrew => "Homebrew",
                Outside::InstallScript => "skrip instalnya",
            }
        ),
        Some(_) => format!("{id} sudah versi terbaru"),
        None => format!("{id} belum terpasang. Tanam dulu: `sawit pasang {id}`"),
    }
}

/// What `sawit cabut` warns before removing `tool`, which the installed
/// tools `needed_by` need: those that run through it wither with it; an app
/// from a Homebrew cask keeps running, but Homebrew no longer updates it.
pub(super) fn cabut_warnings(
    tool: &crate::dev_tools::Tool,
    needed_by: &[&crate::dev_tools::Tool],
) -> Vec<String> {
    use crate::dev_tools::{Impact, impact};
    let names = |which: Impact| -> Vec<&str> {
        needed_by
            .iter()
            .filter(|t| impact(t) == which)
            .map(|t| t.name)
            .collect()
    };
    let mut out = Vec::new();
    let wither = names(Impact::StopsWorking);
    if !wither.is_empty() {
        out.push(format!(
            "{} butuh {}; ikut layu kalau dicabut.",
            wither.join(", "),
            tool.name
        ));
    }
    let apps = names(Impact::NoHomebrewUpdates);
    if !apps.is_empty() {
        out.push(format!(
            "{} tetap jalan, tapi tak lagi diperbarui lewat {}.",
            apps.join(", "),
            tool.name
        ));
    }
    out
}

/// `sawit pupuk [tool…]`.
pub async fn pupuk(args: PupukArgs) -> i32 {
    use crate::dev_tools;
    let ink = Ink::stdout();
    ink.header(
        "🧪 Pupuk Bibit (upgrade tools)",
        "perintah upgrade tiap alat, atau skrip instalnya lagi",
    );
    let (detection, latest) = futures::join!(dev_tools::detect(), dev_tools::latest_versions());
    let found = detection.found;
    if let Some(problem) = &detection.problem {
        ink.warn(&format!(
            "Shell login tidak menjawab ({problem}): hanya alat di tempat biasanya yang dicek."
        ));
    }
    let wanted: Vec<&str> = args.tools.iter().map(|t| t.as_str()).collect();
    let plan = dev_tools::upgrade_plan(&wanted, &found, &latest);
    for id in wanted
        .iter()
        .filter(|id| !plan.iter().any(|t| t.id == **id))
    {
        ink.empty(&pupuk_skip(id, &found));
    }
    if plan.is_empty() {
        ink.footer("Tidak ada yang perlu dipupuk. Kebun sudah subur.");
        return 0;
    }
    ink.section(&format!("Bibit ({})", plan.len()));
    for (ix, t) in plan.iter().enumerate() {
        let from = found
            .get(t.id)
            .and_then(|f| f.version.clone())
            .unwrap_or_else(|| "?".into());
        let to = dev_tools::newer(t.id, &found, &latest).unwrap_or("?");
        println!(
            "  {} {:<14} {:<22} {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            t.id,
            clip(&format!("{from} → {to}"), 22),
            ink.gray(t.upgrade_command())
        );
    }
    println!();
    if !args.yes {
        match confirm(&format!(
            "Jalankan {} perintah upgrade di atas?",
            plan.len()
        )) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Pupuknya disimpan dulu.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau memupuk.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }
    let mut code = 0;
    for tool in plan {
        if tool.interactive {
            // It may ask for a password, which can't be read here.
            ink.warn(&format!(
                "{} mungkin minta kata sandi. Jalankan sendiri:\n      {}",
                tool.id,
                tool.upgrade_command()
            ));
            code = 1;
            continue;
        }
        println!("  {} {}", ink.yellow("⏳"), tool.name);
        let gray = |line: String| println!("     {}", ink.gray(&clip(&line, 100)));
        match dev_tools::run(tool, tool.upgrade_command(), gray).await {
            Ok(()) => println!("  {} {} dipupuk", ink.green("✓"), tool.id),
            Err(e) => {
                println!("  {} {}: {e}", ink.red("✗"), tool.id);
                code = 1;
            }
        }
    }
    ink.footer(if code == 0 {
        "Selesai. Buka terminal baru supaya versinya ikut segar."
    } else {
        "Sebagian gagal tumbuh; lihat di atas."
    });
    code
}

/// `sawit cabut <tool…>`: what the Dev Tools page's trash button runs, and
/// only that ([`crate::dev_tools::uninstall_plan`]).
pub async fn cabut(args: CabutArgs) -> i32 {
    use crate::dev_tools::{self, Removal};
    let ink = Ink::stdout();
    ink.header(
        "🪓 Cabut Bibit (uninstall tools)",
        "hanya salinan dari installer-nya sendiri, Homebrew atau npm. Sisanya dibiarkan tumbuh",
    );
    let detection = dev_tools::detect().await;
    if let Some(problem) = &detection.problem {
        // Which copy is the one in use is unknown: don't remove one the user
        // didn't mean.
        println!(
            "  {} Shell login tidak menjawab ({problem}), jadi tak ada yang dicabut. Coba lagi.",
            ink.red("✗")
        );
        return 1;
    }
    let wanted: Vec<&str> = args.tools.iter().map(|t| t.as_str()).collect();
    let plan = dev_tools::uninstall_plan(&wanted, &detection.found);
    let mut code = 0;
    for step in &plan {
        let id = step.tool.id;
        let at = step.found.as_deref().map(tilde).unwrap_or_default();
        match &step.removal {
            Removal::Missing => ink.empty(&format!("{id} belum terpasang")),
            Removal::LeftAlone => ink.empty(&format!(
                "{id} di {at} bukan tanaman installer-nya, Homebrew atau npm: dibiarkan"
            )),
            Removal::Bundled(with) => ink.empty(&format!(
                "{id} tumbuh bersama {}: ikut tercabut kalau itu dicabut, tidak sendirian",
                with.join(", ")
            )),
            Removal::Terminal(command) => {
                // It asks for a password, which the uninstaller can't read here.
                ink.warn(&format!(
                    "{id} minta kata sandi. Jalankan sendiri:\n      {}",
                    command.replace('\n', "\n      ")
                ));
                code = 1;
            }
            Removal::Run(_) => {}
        }
    }
    let runs: Vec<(&dev_tools::Uninstall, &str)> = plan
        .iter()
        .filter_map(|step| match &step.removal {
            Removal::Run(command) => Some((step, command.as_str())),
            _ => None,
        })
        .collect();
    if runs.is_empty() {
        ink.footer("Tidak ada yang perlu dicabut.");
        return code;
    }
    ink.section(&format!("Bibit ({})", runs.len()));
    for (ix, (step, command)) in runs.iter().enumerate() {
        let at = step.found.as_deref().map(tilde).unwrap_or_default();
        println!(
            "  {} {:<14} {}",
            ink.gray(&format!("{:>3}.", ix + 1)),
            step.tool.id,
            ink.gray(&at)
        );
        for line in command.lines() {
            println!("       {}", ink.gray(line));
        }
        for warning in cabut_warnings(step.tool, &step.needed_by) {
            ink.warn(&warning);
        }
    }
    println!();
    if !args.yes {
        match confirm(&format!(
            "Cabut {} bibit di atas sampai ke akarnya? Tidak bisa ditanam balik tanpa pasang ulang.",
            runs.len()
        )) {
            Some(true) => {}
            Some(false) => {
                ink.footer("Batal. Bibitnya tetap tumbuh, untuk musim ini.");
                return 0;
            }
            None => {
                println!(
                    "  {} Tidak ada saksi (stdin bukan terminal). Tambahkan -y kalau memang mau mencabut.",
                    ink.red("✗")
                );
                return 1;
            }
        }
    }
    let mut removed = Vec::new();
    for (step, command) in &runs {
        println!("  {} {}", ink.yellow("⏳"), step.tool.name);
        let gray = |line: String| println!("     {}", ink.gray(&clip(&line, 100)));
        match dev_tools::uninstall(step.tool, command, gray).await {
            Ok(()) => {
                println!("  {} {} dicabut", ink.green("✓"), step.tool.id);
                removed.push(step.tool.id);
            }
            Err(e) => {
                println!("  {} {}: {e}", ink.red("✗"), step.tool.id);
                code = 1;
            }
        }
    }
    if !removed.is_empty() {
        // As the page does: a copy installed another way is still found.
        let after = dev_tools::detect().await;
        for id in removed {
            if let Some(f) = after.found.get(id) {
                ink.warn(&format!(
                    "{id} masih tumbuh di {}: salinan itu dipasang dengan cara lain (Homebrew, misalnya).",
                    tilde(&f.path)
                ));
                code = 1;
            }
        }
    }
    ink.footer(if code == 0 {
        "Selesai. Lahan kosong lagi; buka terminal baru supaya PATH-nya ikut segar."
    } else {
        "Sebagian masih berakar; lihat di atas."
    });
    code
}
