//! The browser's tabs as they are kept between runs: each tab's address, title, device size and
//! history, and which one was on show, in the project's `.nyawit/browser-tabs.json` (Sawit's own
//! folder for the browser of no project). Agents name a tab by its id or its title.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::ViewportMode;

/// The file's name, in `.nyawit/` or Sawit's own folder.
pub const TABS_FILE: &str = "browser-tabs.json";

/// One tab as it was left.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub viewport: ViewportMode,
    /// WebKit's history of the tab (back and forward), base64; WebKit's own format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,
}

/// The tabs of one browser, in order, and the one on show.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedTabs {
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
}

/// Where the tabs of the browser of `project` (`None`: of no project) are kept; `app_root` is
/// Sawit's own folder.
pub fn tabs_file(project: Option<&Path>, app_root: &Path) -> PathBuf {
    match project {
        Some(project) => crate::nyawit::dir(project).join(TABS_FILE),
        None => app_root.join(TABS_FILE),
    }
}

/// The tabs kept in `file`; none when there is no file or it can't be read (a broken file starts
/// the browser afresh rather than failing it).
pub fn load_tabs(file: &Path) -> SavedTabs {
    let mut saved: SavedTabs = std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    saved.tabs.retain(|t| !t.url.is_empty());
    saved.active = saved.active.min(saved.tabs.len().saturating_sub(1));
    saved
}

/// Writes the tabs to `file` (making `.nyawit/` with its `.gitignore` for a project's), whole,
/// through a file beside it: a crash mid-write leaves the last good list.
pub fn save_tabs(file: &Path, project: Option<&Path>, saved: &SavedTabs) -> std::io::Result<()> {
    let dir = file.parent().unwrap_or(Path::new("."));
    match project {
        Some(project) => crate::nyawit::ensure(project, dir)?,
        None => std::fs::create_dir_all(dir)?,
    }
    let json = serde_json::to_vec_pretty(saved).map_err(std::io::Error::other)?;
    let partial = file.with_extension("json.partial");
    std::fs::write(&partial, json)?;
    std::fs::rename(&partial, file)
}

/// The tab an agent names: by its id (`2`), else by its title or address, ignoring case (whole,
/// else the first that contains it: a port, say). `tabs` are `(id, title, url)` in order; the index.
pub fn find_tab(tabs: &[(u64, &str, &str)], name: &str) -> Option<usize> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if let Ok(id) = name.trim_start_matches('#').parse::<u64>()
        && let Some(ix) = tabs.iter().position(|(i, _, _)| *i == id)
    {
        return Some(ix);
    }
    let lower = name.to_lowercase();
    let exact = tabs
        .iter()
        .position(|(_, title, url)| title.to_lowercase() == lower || url.to_lowercase() == lower);
    exact.or_else(|| {
        tabs.iter().position(|(_, title, url)| {
            title.to_lowercase().contains(&lower) || url.to_lowercase().contains(&lower)
        })
    })
}
