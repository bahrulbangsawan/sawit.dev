//! An artboard (a frame built of vector layers) as code: Export HTML and Export React for frames,
//! as a screen has them ([`crate::screens::export`]). A mechanical reference for whoever builds
//! it in a real stack, not production code; each file says so at its top.
//!
//! The layer tree is walked once ([`tree`]) into elements, each with the CSS Copy as CSS gives
//! its layer (FigCraft's Dev Mode CSS, [`figcraft_engine::codegen`]): no second style mapper.
//! What the walk adds is where a layer sits and how it sizes: auto layout frames are flexboxes
//! (their children in the flow, `flex: 1 1 0` or `align-self: stretch` when they fill, their
//! size left out where they hug), other children are placed absolutely in their parent. Text keeps
//! its type; image fills and vector layers are embedded (a `data:` URL, inline SVG); variables
//! bound to a layer are CSS custom properties declared on `:root`; layer names are class names and
//! `data-name`.
//!
//! - **HTML** ([`to_html`]): one self-contained file.
//! - **React** ([`to_react`]): a `.tsx` component (named as [`crate::screens::component_name`]
//!   names a screen's) and its CSS module. Instances of components are calls to a React
//!   component of their own, with a prop for each variant property and their text overrides
//!   ([`react`]).

mod components;
mod layers;
mod react;
mod render;
#[cfg(test)]
mod tests;
mod tree;

use std::path::Path;

use figcraft_doc::{Document, NodeId, NodeKind};

use crate::doc::{BoardDoc, BoardError};
use crate::screens::component_name;
use crate::screens::export::Written;

/// What the top of every exported file says the code is.
pub const REFERENCE: &str = "Mechanical reference: Sawit converted the canvas frame layer by \
     layer, for an agent or a developer to rebuild it in the project's own stack, components and \
     tokens. It is not production code: positions, sizes and styles are the design's, as drawn.";

/// A frame as React: the component's name, its `.tsx` and its CSS module.
#[derive(Clone, Debug, PartialEq)]
pub struct ReactCode {
    pub component: String,
    pub tsx: String,
    pub css: String,
    pub notes: Vec<String>,
}

/// The frame `wanted` names in `doc`: its id, or the name of a frame (a component, an instance)
/// on any page, the page on show first.
pub fn find_frame(doc: &Document, page: Option<NodeId>, wanted: &str) -> Option<NodeId> {
    let wanted = wanted.trim();
    let framed = |id: NodeId| doc.node(id).is_some_and(|n| n.kind.is_frame_like());
    if let Ok(id) = wanted.parse::<NodeId>()
        && framed(id)
    {
        return Some(id);
    }
    let named = |id: &NodeId| framed(*id) && doc.node(*id).is_some_and(|n| n.name.trim() == wanted);
    let on_show = page.and_then(|p| doc.subtree(p).into_iter().find(named));
    on_show.or_else(|| doc.nodes().map(|n| n.id).find(named))
}

/// Whether `id` is an artboard: a frame (a component, an instance) whose children are layers,
/// right on its page, that isn't a board object (a sticky, a shape with text, a table).
pub fn is_artboard(doc: &Document, id: NodeId) -> bool {
    let Some(node) = doc.node(id) else {
        return false;
    };
    let on_page = node
        .parent
        .and_then(|p| doc.node(p))
        .is_some_and(|p| matches!(p.kind, NodeKind::Page { .. } | NodeKind::Section { .. }));
    // A board's stickies, shapes with text and tables are frames too, and no artboards.
    let board_object = node.board.is_some();
    node.kind.is_frame_like()
        && on_page
        && !board_object
        && !matches!(node.kind, NodeKind::ComponentSet { .. })
}

/// Frame `frame` of `board` as one HTML file.
pub fn to_html(board: &mut BoardDoc, frame: NodeId) -> Result<(String, Vec<String>), BoardError> {
    let mut built = tree::build(board, frame, false)?;
    let title = built.title.clone();
    let html = render::html_page(&title, &mut built);
    Ok((html, built.notes))
}

/// Frame `frame` of `board` as React component `component` (its `.tsx` imports `css_file`).
pub fn to_react(
    board: &mut BoardDoc,
    frame: NodeId,
    component: Option<&str>,
    css_file: &str,
) -> Result<ReactCode, BoardError> {
    let mut built = tree::build(board, frame, true)?;
    let name = component.map_or_else(|| component_name(&built.title), str::to_string);
    let (tsx, css) = react::tsx(&name, css_file, &mut built);
    Ok(ReactCode {
        component: name,
        tsx,
        css,
        notes: built.notes,
    })
}

/// The file name an export of frame `title` takes: `settings-mobile.html`, `SettingsMobile.tsx`.
pub fn file_name(title: &str, react: bool) -> String {
    match react {
        true => format!("{}.tsx", component_name(title)),
        false => format!("{}.html", crate::screens::name_for(title, &[])),
    }
}

/// Writes frame `frame` as HTML at `path`.
pub fn write_html(board: &mut BoardDoc, frame: NodeId, path: &Path) -> Result<Written, String> {
    let (html, notes) = to_html(board, frame).map_err(|e| e.for_user())?;
    write(path, &html)?;
    Ok(Written {
        files: vec![path.to_path_buf()],
        notes,
    })
}

/// Writes frame `frame` as React at `tsx` (`Settings.tsx`), its CSS module beside it
/// (`Settings.module.css`).
pub fn write_react(board: &mut BoardDoc, frame: NodeId, tsx: &Path) -> Result<Written, String> {
    let stem = tsx.file_stem().map(|s| s.to_string_lossy().into_owned());
    let css_file = format!(
        "{}.module.css",
        stem.clone().unwrap_or_else(|| "Frame".into())
    );
    let code = to_react(board, frame, stem.as_deref(), &css_file).map_err(|e| e.for_user())?;
    write(tsx, &code.tsx)?;
    let css = tsx.with_file_name(&css_file);
    write(&css, &code.css)?;
    Ok(Written {
        files: vec![tsx.to_path_buf(), css],
        notes: code.notes,
    })
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("couldn't write {}: {e}", path.display()))
}
