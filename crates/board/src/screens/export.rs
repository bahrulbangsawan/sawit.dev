//! Writing a screen's downloads where the user or an agent asks: one HTML file ([`write_html`]),
//! or a React component with its CSS and the files it uses beside it ([`write_react`]).

use std::path::{Path, PathBuf};

use super::component_name;
use super::inline::single_file;
use super::react::to_react;

/// What an export wrote, and what it noted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Written {
    pub files: Vec<PathBuf>,
    pub notes: Vec<String>,
}

/// Writes screen folder `folder` as one HTML file at `path`, its CSS and images inlined.
pub fn write_html(folder: &Path, path: &Path) -> Result<Written, String> {
    let html = single_file(folder)?;
    write(path, html.as_bytes())?;
    Ok(Written {
        files: vec![path.to_path_buf()],
        notes: Vec::new(),
    })
}

/// Writes screen `name` (its folder `folder`) as a React component at `tsx` (`PricingPage.tsx`):
/// the CSS it imports beside it, named after it (`PricingPage.css`), and the files it uses in a
/// folder beside them (`PricingPage-assets/`).
pub fn write_react(folder: &Path, name: &str, tsx: &Path) -> Result<Written, String> {
    let stem = tsx.file_stem().map_or_else(
        || component_name(name),
        |s| s.to_string_lossy().into_owned(),
    );
    let dir = tsx.parent().unwrap_or(Path::new("."));
    let (css_file, assets_dir) = (format!("{stem}.css"), format!("{stem}-assets"));
    let export = to_react(folder, &component_name(name), &css_file, &assets_dir)?;
    write(tsx, export.tsx.as_bytes())?;
    let css = dir.join(&css_file);
    write(&css, export.css.as_bytes())?;
    let mut files = vec![tsx.to_path_buf(), css];
    for (to, from) in &export.assets {
        let target = dir.join(to);
        let bytes = std::fs::read(from).map_err(|e| format!("{}: {e}", from.display()))?;
        write(&target, &bytes)?;
        files.push(target);
    }
    Ok(Written {
        files,
        notes: export.notes,
    })
}

/// Writes `bytes` at `path`, its folder made if need be.
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("couldn't write {}: {e}", path.display()))
}
