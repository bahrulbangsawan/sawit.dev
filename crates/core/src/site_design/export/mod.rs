//! The files of a design export. Everything written traces to something the page showed: a
//! section, token or shadcn variable with no source is left out.

mod css;
mod design_md;
mod dtcg;
mod readme;
#[cfg(test)]
mod tests;

use std::path::Path;

use super::scan::{AssetOutcome, Scan};
use super::tokens::Tokens;

pub use css::{shadcn_css, tailwind_theme, variables_css};
pub use design_md::design_md;
pub use dtcg::dtcg;
pub use readme::readme;

/// `17` → `17`, `1.470` → `1.47`.
fn n(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn px(v: f64) -> String {
    format!("{}px", n(v))
}

fn plural(count: u32, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn semantic<'a>(t: &'a Tokens, name: &str) -> Option<&'a super::color::Rgba> {
    t.semantic.iter().find(|(s, _)| s == name).map(|(_, c)| c)
}

// ---------- folder ----------

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = format!("{prefix}{}", e.file_name().to_string_lossy());
        if e.file_type()?.is_dir() {
            walk(&e.path(), &format!("{name}/"), out)?;
        } else {
            out.push(name);
        }
    }
    Ok(())
}

fn safe_kind(kind: &str) -> bool {
    !kind.is_empty()
        && kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Replaces the export in `dir`, keeping `assets/` (the app refills it before this call).
pub fn write_folder(
    dir: &Path,
    t: &Tokens,
    scan: &Scan,
    screenshot_png: Option<&[u8]>,
    assets: &[AssetOutcome],
) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        if e.file_name() == "assets" {
            continue;
        }
        if e.file_type()?.is_dir() {
            std::fs::remove_dir_all(e.path())?;
        } else {
            std::fs::remove_file(e.path())?;
        }
    }
    let write = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes);
    write("DESIGN.md", design_md(t).as_bytes())?;
    let tokens = serde_json::to_string_pretty(&dtcg(t)).map_err(std::io::Error::other)?;
    write("tokens.json", tokens.as_bytes())?;
    write("theme.css", tailwind_theme(t).as_bytes())?;
    write("shadcn.css", shadcn_css(t).as_bytes())?;
    write("variables.css", variables_css(&t.variables).as_bytes())?;
    if let Some(png) = screenshot_png {
        write("screenshot.png", png)?;
    }
    for c in scan.components.iter().filter(|c| safe_kind(&c.kind)) {
        std::fs::create_dir_all(dir.join("components"))?;
        write(&format!("components/{}.html", c.kind), c.html.as_bytes())?;
    }
    let manifest = serde_json::to_string_pretty(assets).map_err(std::io::Error::other)?;
    write("assets.json", manifest.as_bytes())?;

    // README lists the files, itself included.
    std::fs::write(dir.join("README.md"), "")?;
    let mut files = Vec::new();
    walk(dir, "", &mut files)?;
    files.sort();
    write("README.md", readme(t, &files).as_bytes())?;
    Ok(files)
}
