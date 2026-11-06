//! Send to Agent: screens and artboards handed to a coding agent to build in the project's own
//! stack. One frame, or several as a flow in canvas order, go to a folder of their own
//! (`.nyawit/handoff/<name>/` for a project's canvas, [`folder`]):
//!
//! - `design.png`: the frames as the canvas shows them, at 2× (a screen's latest picture painted
//!   in its frame); for a flow, each frame's too (`01-<frame>.png`, …);
//! - each frame's code: a screen's HTML in one file and its React version
//!   ([`crate::screens::export`]), an artboard's from its layers ([`crate::frame_code`]);
//! - `spec.md`: sizes and breakpoints, the layer tree, the tokens, components and variants, the
//!   copy, the states drawn and the prototype links ([`spec`]);
//! - `prompt.md`: what the agent is asked ([`prompt`]): to build it in the project's stack and
//!   components, following Sawit's design standard, and to check it against `design.png` in
//!   Sawit's browser.

mod prompt;
mod spec;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use figcraft_doc::{Document, NodeId, NodeKind};

pub use prompt::{STANDARD, prompt};

use crate::design::export::{ExportRow, export_bytes};
use crate::doc::BoardDoc;
use crate::screens::{self, export};

/// One frame handed off: its id, its name on the canvas, and its screen's folder name when it
/// is a screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub frame: NodeId,
    pub title: String,
    pub screen: Option<String>,
    pub size: (f64, f64),
}

/// A frame's files in the handoff.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Files {
    pub png: Option<PathBuf>,
    pub html: Option<PathBuf>,
    pub tsx: Option<PathBuf>,
    pub css: Option<PathBuf>,
    /// A screen's source folder.
    pub source: Option<PathBuf>,
}

/// A written handoff.
#[derive(Clone, Debug, PartialEq)]
pub struct Handoff {
    pub name: String,
    pub folder: PathBuf,
    pub design: Option<PathBuf>,
    pub spec: PathBuf,
    pub prompt: String,
    pub prompt_file: PathBuf,
    pub files: Vec<PathBuf>,
}

/// The frames `ids` names in `doc`, in canvas order (rows top to bottom, each left to right):
/// screens and frames only, an error naming what else is there.
pub fn targets(doc: &Document, ids: &[NodeId]) -> Result<Vec<Target>, String> {
    let mut out = Vec::new();
    for id in ids {
        let node = doc.node(*id).ok_or_else(|| format!("no layer {id}"))?;
        if !node.kind.is_frame_like() || matches!(node.kind, NodeKind::ComponentSet { .. }) {
            return Err(format!(
                "“{}” isn't a frame: hand off screens and frames",
                node.name
            ));
        }
        let bounds = doc.world_bounds(*id);
        out.push((
            bounds.map_or((0.0, 0.0), |b| (b.y0, b.x0)),
            Target {
                frame: *id,
                title: node.name.clone(),
                screen: screens::screen_of(node).map(|s| s.screen),
                size: (node.width, node.height),
            },
        ));
    }
    if out.is_empty() {
        return Err("select a screen or a frame to hand off".into());
    }
    // A row is frames whose tops are within 100 of each other (cnv.7 puts 200 between rows).
    out.sort_by(|(a, _), (b, _)| {
        let row = |y: f64| (y / 100.0).floor() as i64;
        (row(a.0), a.1)
            .partial_cmp(&(row(b.0), b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out.into_iter().map(|(_, t)| t).collect())
}

/// The handoff's name: the screen's folder name, a frame's name as one (`settings-mobile`), or
/// for a flow its first frame's with `-flow`.
pub fn name(targets: &[Target]) -> String {
    let first = targets.first();
    let base = match first.and_then(|t| t.screen.clone()) {
        Some(screen) => screen,
        None => screens::name_for(first.map_or("design", |t| t.title.as_str()), &[]),
    };
    match targets.len() {
        0 | 1 => base,
        _ => format!("{base}-flow"),
    }
}

/// Where a handoff called `name` goes for a canvas whose screens are in `screens_dir`: a
/// `handoff` folder beside it (`.nyawit/handoff/<name>/`).
pub fn folder(screens_dir: &Path, name: &str) -> PathBuf {
    let beside = screens_dir.parent().unwrap_or(Path::new("."));
    beside.join("handoff").join(name)
}

/// Writes the handoff of `targets` of `board` (its screens in `screens_dir`) into `out`;
/// `pictures` are the screens' latest pictures, painted in their frames. What can't be made is
/// left out and the prompt doesn't name it; only the prompt and the spec failing is an error.
pub fn write(
    board: &mut BoardDoc,
    targets: &[Target],
    screens_dir: &Path,
    out: &Path,
    pictures: &[(NodeId, Vec<u8>)],
) -> Result<Handoff, String> {
    std::fs::create_dir_all(out).map_err(|e| format!("Couldn't make {}: {e}", out.display()))?;
    let doc = board.doc().map_err(|e| e.for_user())?;
    let mut painted = match pictures.is_empty() {
        true => None,
        false => crate::ops::with_pictures(board, pictures).ok(),
    };
    let ids: Vec<NodeId> = targets.iter().map(|t| t.frame).collect();
    let mut written = Vec::new();
    let design = png(
        painted.as_mut().unwrap_or(&mut *board),
        &ids,
        out.join("design.png"),
    );
    written.extend(design.clone());
    let flow = targets.len() > 1;
    let mut frames: Vec<(Target, Files)> = Vec::new();
    for (i, target) in targets.iter().enumerate() {
        let mut files = Files::default();
        let base = target
            .screen
            .clone()
            .unwrap_or_else(|| screens::name_for(&target.title, &[]));
        let stem = match flow {
            true => format!("{:02}-{base}", i + 1),
            false => base.clone(),
        };
        if flow {
            let source = painted.as_mut().unwrap_or(&mut *board);
            files.png = png(source, &[target.frame], out.join(format!("{stem}.png")));
        }
        let html = out.join(format!("{stem}.html"));
        let tsx = out.join(format!("{}.tsx", screens::component_name(&base)));
        match &target.screen {
            Some(name) => {
                let source = screens::folder(screens_dir, name);
                if export::write_html(&source, &html).is_ok() {
                    files.html = Some(html);
                }
                if let Ok(react) = export::write_react(&source, name, &tsx) {
                    files.css = react.files.get(1).cloned();
                    files.tsx = Some(tsx);
                }
                files.source = Some(source);
            }
            None => {
                if crate::frame_code::write_html(board, target.frame, &html).is_ok() {
                    files.html = Some(html);
                }
                if let Ok(react) = crate::frame_code::write_react(board, target.frame, &tsx) {
                    files.css = react.files.get(1).cloned();
                    files.tsx = Some(tsx);
                }
            }
        }
        written.extend(
            [&files.png, &files.html, &files.tsx, &files.css]
                .into_iter()
                .flatten()
                .cloned(),
        );
        frames.push((target.clone(), files));
    }
    let spec_text = spec::spec(&doc, &frames, screens_dir);
    let spec_file = out.join("spec.md");
    std::fs::write(&spec_file, &spec_text)
        .map_err(|e| format!("Couldn't write {}: {e}", spec_file.display()))?;
    written.push(spec_file.clone());
    let prompt = prompt(&frames, design.as_deref(), &spec_file, out);
    let prompt_file = out.join("prompt.md");
    std::fs::write(&prompt_file, &prompt)
        .map_err(|e| format!("Couldn't write {}: {e}", prompt_file.display()))?;
    written.push(prompt_file.clone());
    Ok(Handoff {
        name: out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        folder: out.to_path_buf(),
        design,
        spec: spec_file,
        prompt,
        prompt_file,
        files: written,
    })
}

/// Layers `ids` of `board` as one PNG at 2× at `path`, if it can be made.
fn png(board: &mut BoardDoc, ids: &[NodeId], path: PathBuf) -> Option<PathBuf> {
    let row = ExportRow {
        format: "png".into(),
        scale: 2.0,
    };
    let bytes = export_bytes(board, ids, &row).ok()?;
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}
