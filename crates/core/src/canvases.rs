//! A project's canvases: each a `.sawit` file in its `.nyawit/`, with a tab of its own in the app.
//! `canvas.sawit` is the project's canvas and comes first; the others are named by the user
//! (`canvas-2.sawit`, `flows.sawit`). Which were open in tabs is kept in `canvas-tabs.json`, so
//! they open again with the project's canvas.

use std::path::{Path, PathBuf};

use crate::nyawit;

/// The list of canvases open in tabs, beside them.
pub const OPEN_FILE: &str = "canvas-tabs.json";

/// The canvases of `project`, the project's canvas first (whether or not it is on disk yet), then
/// the others by name. Backups beside them are not canvases.
pub fn canvases(project: &Path) -> Vec<PathBuf> {
    let main = nyawit::board_file(project);
    let mut others: Vec<PathBuf> = std::fs::read_dir(nyawit::dir(project))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "sawit") && p.is_file() && *p != main)
        .collect();
    others.sort();
    std::iter::once(main).chain(others).collect()
}

/// A canvas's name: its file's stem (`canvas`, `flows`).
pub fn canvas_name(file: &Path) -> String {
    file.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The file of the canvas named `name` in `project`: `name` is a canvas's name or its file name.
/// Refused when it can't be a file name of its own (a path, `..`, hidden, empty or too long).
pub fn canvas_file(project: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    let stem = name.strip_suffix(".sawit").unwrap_or(name).trim();
    let bad = stem.is_empty()
        || stem.len() > 80
        || stem.starts_with('.')
        || stem.contains(['/', '\\', ':', '\0']);
    if bad {
        return Err(format!(
            "“{name}” can't name a canvas: use letters, numbers, spaces, - or _, up to 80."
        ));
    }
    Ok(nyawit::dir(project).join(format!("{stem}.sawit")))
}

/// A name for a new canvas in `project`: `canvas-2`, `canvas-3`, … the first that is neither a
/// file nor in `taken` (canvases open in tabs but not written yet).
pub fn new_canvas_file(project: &Path, taken: &[PathBuf]) -> PathBuf {
    (2..)
        .map(|n| nyawit::dir(project).join(format!("canvas-{n}.sawit")))
        .find(|p| std::fs::symlink_metadata(p).is_err() && !taken.contains(p))
        .expect("a free name")
}

/// The canvases of `project` that were open in tabs, by file name, in their order.
pub fn load_open(project: &Path) -> Vec<String> {
    std::fs::read(nyawit::dir(project).join(OPEN_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|n| canvas_file(project, n).is_ok())
        .collect()
}

/// Keeps `open` (file names) as the canvases of `project` open in tabs.
pub fn save_open(project: &Path, open: &[String]) -> std::io::Result<()> {
    let dir = nyawit::dir(project);
    nyawit::ensure(project, &dir)?;
    let json = serde_json::to_vec(open).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(OPEN_FILE), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_projects_canvases_start_with_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let main = nyawit::board_file(project);
        assert_eq!(
            canvases(project),
            std::slice::from_ref(&main),
            "only the project's, before any file"
        );
        let n = nyawit::dir(project);
        std::fs::create_dir_all(&n).unwrap();
        for f in [
            "flows.sawit",
            "canvas.sawit",
            "canvas.sawit.bak",
            "b.sawit",
            "x.png",
        ] {
            std::fs::write(n.join(f), b"{}").unwrap();
        }
        assert_eq!(
            canvases(project),
            [main, n.join("b.sawit"), n.join("flows.sawit")]
        );
        assert_eq!(canvas_name(&n.join("flows.sawit")), "flows");
    }

    #[test]
    fn canvas_names_are_plain_file_names() {
        let p = Path::new("/p");
        assert_eq!(
            canvas_file(p, "flows").unwrap(),
            Path::new("/p/.nyawit/flows.sawit")
        );
        assert_eq!(
            canvas_file(p, " flows.sawit ").unwrap(),
            Path::new("/p/.nyawit/flows.sawit")
        );
        for bad in ["", " ", "../x", "a/b", ".hidden", ".sawit"] {
            assert!(canvas_file(p, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn new_canvases_take_the_next_free_name_and_open_ones_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let n = nyawit::dir(project);
        std::fs::create_dir_all(&n).unwrap();
        std::fs::write(n.join("canvas-2.sawit"), b"{}").unwrap();
        let open = vec![n.join("canvas-3.sawit")];
        assert_eq!(new_canvas_file(project, &open), n.join("canvas-4.sawit"));
        assert_eq!(load_open(project), Vec::<String>::new());
        save_open(
            project,
            &[
                "canvas.sawit".into(),
                "canvas-2.sawit".into(),
                "../x".into(),
            ],
        )
        .unwrap();
        assert_eq!(load_open(project), ["canvas.sawit", "canvas-2.sawit"]);
    }
}
