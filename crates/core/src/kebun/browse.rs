//! `sawit browse` (`intip`), and `sawit <url>`: a page in the app's browser, by default the
//! dev server of this folder's project.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::app_control::{self, Command, Request};
use crate::browser::{self, BrowserOp};
use crate::cli::BrowseTarget;
use crate::instances::AppPaths;
use crate::ports::PortJob;
use crate::probe;

use super::bakar::ports_of;
use super::block_on;
use super::ink::{Ink, clip, tilde};

/// How long `sawit browse` waits for each answer of the app, which gives a page op 30 s.
const BROWSE_TIMEOUT: Duration = Duration::from_secs(35);

/// `sawit browse [url|port]` (`intip`), and `sawit <url>`: opens the page in Sawit's browser and
/// brings Sawit forward. It sends what `browser_navigate` sends for an agent working in `cwd`, so
/// the browser is that of the open project that holds `cwd` (else the front window's). With no
/// address, the dev server running in `cwd`, else in that project ([`dev_server_in`]).
/// `start_app` starts Sawit when it isn't running, as `sawit <path>` does.
pub fn browse(
    target: BrowseTarget,
    paths: &AppPaths,
    cwd: &Path,
    start_app: impl FnOnce() -> Result<(), String>,
) -> i32 {
    let ink = Ink::stderr();
    if !app_control::is_running(paths)
        && let Err(e) = start_app()
    {
        eprintln!("{} Sawit tidak mau menyala: {e}", ink.red("🔥"));
        return 1;
    }
    let folder = own_folder(cwd);
    let ask = |command| {
        let request = Request {
            project: None,
            agent_folder: folder.clone(),
            agent: None,
            command,
        };
        app_control::send(paths, &request, BROWSE_TIMEOUT)
    };
    let url = match target {
        BrowseTarget::Address(address) => browser::normalize_address(&address),
        BrowseTarget::DevServer => {
            let found = ask(Command::State).and_then(|state| {
                let folders = dev_server_folders(&state, folder.as_deref())?;
                let jobs =
                    block_on(probe::listening_jobs()).map_err(|e| format!("lsof gagal: {e}"))?;
                dev_server_in(&jobs, &folders)
            });
            match found {
                Ok(url) => url,
                Err(e) => {
                    eprintln!("{} {e}", ink.red("🔥"));
                    return 1;
                }
            }
        }
    };
    let op = BrowserOp::Navigate { url: url.clone() };
    if let Err(e) = ask(Command::Browser { op }) {
        eprintln!("{} {url} tidak bisa diintip: {e}", ink.red("🔥"));
        return 1;
    }
    let _ = ask(Command::Focus);
    eprintln!(
        "{}",
        ink.gray(&format!("🌐 {url} · terbuka di browser Sawit"))
    );
    0
}

/// The folder `sawit browse` works in, as an agent's would be: its own, unless that is `/` or
/// the home folder, which hold no one project.
pub(super) fn own_folder(cwd: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    (cwd.parent().is_some() && Some(cwd) != home.as_deref()).then(|| cwd.to_path_buf())
}

/// Where `sawit browse` looks for the dev server, nearest first, from the app's `state`: `folder`,
/// then the open project that holds it; with no folder of its own, the front window's project,
/// whose browser shows the page.
pub(super) fn dev_server_folders(
    state: &serde_json::Value,
    folder: Option<&Path>,
) -> Result<Vec<PathBuf>, String> {
    let Some(folder) = folder else {
        return browser::front_project(state).map(|project| vec![project]).ok_or_else(|| {
            "Jendela Sawit di depan tidak punya proyek, jadi tidak ada dev server-nya. Sebut alamatnya: `sawit browse 5173`."
                .to_string()
        });
    };
    let mut folders = vec![folder.to_path_buf()];
    if let Some(project) = browser::project_holding(state, folder)
        && !folders.contains(&project)
    {
        folders.push(project);
    }
    Ok(folders)
}

/// The address of the dev server running in the first of `folders` that runs any
/// ([`browser::dev_servers`]); several there, or none in any, is an error that says what to type.
pub(super) fn dev_server_in(jobs: &[PortJob], folders: &[PathBuf]) -> Result<String, String> {
    for folder in folders {
        let found = browser::dev_servers(jobs, folder);
        match found.as_slice() {
            [] => continue,
            [job] => {
                if let Some(url) = browser::dev_server_url(job) {
                    return Ok(url);
                }
            }
            several => {
                let named: Vec<String> = several
                    .iter()
                    .map(|job| format!("{} ({})", ports_of(job), clip(&job.label, 30)))
                    .collect();
                let port = several[0]
                    .ports
                    .iter()
                    .map(|(p, _)| *p)
                    .min()
                    .unwrap_or_default();
                return Err(format!(
                    "Ada {} dev server di {}: {}. Sebut salah satu: `sawit browse {port}`.",
                    several.len(),
                    tilde(folder),
                    named.join(", ")
                ));
            }
        }
    }
    let place = folders.last().map(|f| tilde(f)).unwrap_or_default();
    Err(format!(
        "Tidak ada dev server yang menyala di {place}. Jalankan dulu (`npm run dev`, `bun dev`, …), atau sebut alamatnya: `sawit browse 5173`. Lihat `sawit list ports`."
    ))
}
