//! The local servers a browser can open: one row per port a dev server listens on, from the
//! listening jobs ([`super::PortJob`]), for the browser's list of servers.

use super::PortJob;

/// A port a local server listens on, as the browser lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalServer {
    pub port: u16,
    /// What runs it: `bun run dev`, `vite`.
    pub name: String,
    /// The project folder's name, when known.
    pub project: Option<String>,
}

impl LocalServer {
    /// `http://localhost:5173/`.
    pub fn url(&self) -> String {
        format!("http://localhost:{}/", self.port)
    }
}

/// The ports of the dev servers among `jobs`, ascending: databases and other servers running on
/// their own, Docker's published ports and apps that aren't dev tools are left out (they serve no
/// page, or not one being built).
pub fn local_servers(jobs: &[PortJob]) -> Vec<LocalServer> {
    let mut out: Vec<LocalServer> = jobs
        .iter()
        .filter(|j| j.dev && !j.is_daemon() && !j.is_docker())
        .flat_map(|j| {
            let project = j.cwd.as_deref().and_then(folder_name);
            j.ports.iter().map(move |(port, _)| LocalServer {
                port: *port,
                name: j.label.clone(),
                project: project.clone(),
            })
        })
        .collect();
    out.sort_by_key(|s| s.port);
    out.dedup_by_key(|s| s.port);
    out
}

fn folder_name(path: &str) -> Option<String> {
    let name = std::path::Path::new(path.trim_end_matches('/'))
        .file_name()?
        .to_string_lossy()
        .into_owned();
    (!name.is_empty()).then_some(name)
}
