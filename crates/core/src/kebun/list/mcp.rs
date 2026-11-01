//! `sawit list mcp`: the MCP page's servers and the AI clients that have them.

use std::path::PathBuf;

use crate::kebun::ink::{Ink, clip};

/// The `sawit-mcp` next to this program, which the MCP page installs as
/// Sawit's own server: the app's binary, also when `sawit` is reached
/// through the `/usr/local/bin/sawit` link.
fn own_mcp_server() -> PathBuf {
    std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("sawit-mcp")))
        .unwrap_or_else(|| PathBuf::from("sawit-mcp"))
}

/// A client as `sawit list mcp` names it: "Codex", "Claude Code (profil work)".
fn client_name(target: &crate::mcp_clients::Target) -> String {
    match target.profile.as_str() {
        "default" => target.client.name().to_string(),
        profile => format!("{} (profil {profile})", target.client.name()),
    }
}

/// `sawit list mcp`: the MCP page's servers and the clients that have them.
pub(super) fn list_mcp(ink: &Ink) {
    use crate::mcp_catalog::{self, Auth, DEFAULT_AWS_REGION, Source};
    use crate::mcp_clients::Status;
    let report = mcp_catalog::report(&own_mcp_server());
    ink.section("🔌 Colokan MCP (servers)");
    let clients: Vec<String> = report.targets.iter().map(client_name).collect();
    if clients.is_empty() {
        ink.empty("belum ada klien AI di Mac ini. Kebunnya masih digarap tangan manusia.");
    } else {
        println!("  {} {}", ink.gray("klien:"), clients.join(", "));
    }
    // A config that can't be read, or that Sawit doesn't edit (YAML, a
    // script), is so for every server: said once, here, not under each.
    let mut noted: Vec<usize> = Vec::new();
    for (ix, status) in report.servers.iter().flat_map(|l| &l.installed) {
        let note = match status {
            Status::Unreadable(why) => format!("config tak terbaca ({why}): periksa sendiri"),
            Status::Manual => "config-nya YAML atau skrip: server ditambahkan sendiri".to_string(),
            _ => continue,
        };
        if !noted.contains(ix) {
            noted.push(*ix);
            ink.warn(&format!("{}: {note}", client_name(&report.targets[*ix])));
        }
    }
    for listed in &report.servers {
        let server = listed.server;
        let address = match server.source {
            Source::Bundled => "bawaan Sawit".to_string(),
            _ => server.address(DEFAULT_AWS_REGION),
        };
        println!(
            "  {:<24} {:<20} {}",
            server.id,
            clip(server.name, 20),
            ink.gray(&address)
        );
        let pad = " ".repeat(24);
        // Only names: what a key holds never comes near this list.
        let sign_in = match server.auth {
            Auth::None => None,
            Auth::Browser => Some("masuk lewat browser dari kliennya".to_string()),
            Auth::EnvKey(var) => Some(format!("butuh variabel {var} sebelum kliennya dibuka")),
            Auth::Program => Some("pakai login programnya sendiri".to_string()),
            Auth::GitHub => Some(
                "pakai login GitHub Sawit lewat `sawit github token`; tokennya tak ditulis di config"
                    .to_string(),
            ),
        };
        if let Some(sign_in) = sign_in {
            println!("  {pad} {}", ink.gray(&sign_in));
        }
        if let Some(program) = listed.missing_program {
            println!(
                "  {pad} {}",
                ink.yellow(&format!("! {program} belum terpasang"))
            );
        }
        // The clients that have it; the ones noted above are left out.
        let installed: Vec<String> = listed
            .installed
            .iter()
            .filter_map(|(ix, status)| {
                let name = client_name(&report.targets[*ix]);
                match status {
                    Status::Installed => Some(name),
                    Status::Outdated(sig) => Some(match server.region_in(sig) {
                        Some(region) => format!("{name} (region {region})"),
                        None => format!("{name} (menunjuk ke tempat lain; pasang ulang)"),
                    }),
                    _ => None,
                }
            })
            .collect();
        if installed.is_empty() {
            println!("  {pad} {}", ink.gray("belum di klien mana pun"));
        } else {
            println!("  {pad} {} {}", ink.green("✓"), installed.join(", "));
        }
    }
    println!(
        "  {}",
        ink.gray("Pasang dari halaman MCP di app; agen pakai `install_mcp_server`.")
    );
}
