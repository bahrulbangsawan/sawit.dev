//! The project a browser belongs to: its website data store, the dev server it opens, and the
//! project the app's state names for a folder.

use std::path::Path;

use serde_json::Value;

use crate::ports::PortJob;

/// The website data store (cookies, storage) of the project's browser:
/// one per folder, stable across launches.
pub fn data_store_id(project: &Path) -> [u8; 16] {
    let bytes = project.as_os_str().as_encoded_bytes();
    let fnv = |seed: u64| {
        bytes.iter().fold(seed, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    };
    let mut id = [0u8; 16];
    id[..8].copy_from_slice(&fnv(0xcbf2_9ce4_8422_2325).to_be_bytes());
    id[8..].copy_from_slice(&fnv(0x6c62_272e_07bb_0142).to_be_bytes());
    // A version 4 UUID shape, as WKWebsiteDataStore expects a UUID.
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

/// The project's dev server when exactly one is running from inside it.
pub fn pick_dev_server(jobs: &[PortJob], project: &Path) -> Option<String> {
    match dev_servers(jobs, project).as_slice() {
        [job] => dev_server_url(job),
        _ => None,
    }
}

/// The dev servers running from inside `folder` (what `npm run dev` and the like start, not a
/// database, a daemon or Docker), with a port.
pub fn dev_servers<'a>(jobs: &'a [PortJob], folder: &Path) -> Vec<&'a PortJob> {
    jobs.iter()
        .filter(|j| {
            j.dev
                && !j.is_daemon()
                && !j.is_docker()
                && j.cwd
                    .as_deref()
                    .is_some_and(|c| Path::new(c).starts_with(folder))
                && !j.ports.is_empty()
        })
        .collect()
}

/// Where the browser opens `job`, a dev server: its lowest port on localhost.
pub fn dev_server_url(job: &PortJob) -> Option<String> {
    let port = job.ports.iter().map(|(p, _)| *p).min()?;
    Some(format!("http://localhost:{port}"))
}

/// The front window's project, from the app's `state` (its answer to `Command::State`): its
/// browser shows the page `browser_navigate` and `sawit browse` open for no project of their own.
pub fn front_project(state: &Value) -> Option<std::path::PathBuf> {
    let windows = state["windows"].as_array()?;
    let front = windows.iter().find(|window| window["frontmost"] == true)?;
    front["project"].as_str().map(std::path::PathBuf::from)
}

/// The open project that holds `folder`, from the app's `state`: the deepest of the windows'
/// projects, as the app picks the window and the browser for a folder, and as it is on disk
/// (through the links on its way, [`crate::files::resolve`]), as `lsof` lists a dev server's.
pub fn project_holding(state: &Value, folder: &Path) -> Option<std::path::PathBuf> {
    let folder = crate::files::resolve(folder);
    state["windows"]
        .as_array()?
        .iter()
        .flat_map(|window| window["projects"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(|root| crate::files::resolve(Path::new(root)))
        .filter(|root| folder.starts_with(root))
        .max_by_key(|root| root.components().count())
}
