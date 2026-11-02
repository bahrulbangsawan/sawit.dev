//! The canvas's one export writer (ruling P6-C8), which the Export button and File ▸ Export, Export
//! as SVG and Export frames to PDF write through: the files made, named after their layer, and
//! written into the canvas's exports folder.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use figcraft_doc::{Document, NodeId};
use figcraft_engine::guard;
use gpui_kit::*;
use sawit_board::design::DesignSelection;
use sawit_board::design::export::{ExportRow, export_bytes, export_rows};
use sawit_board::{BoardDoc, BoardError, ops};
use sawit_core::instances::AppPaths;
use sawit_core::nyawit;

use super::super::number_field::number_text;
use super::{EXPORTS, PAGE_FORMAT, PIXEL_FORMATS};
use crate::board::BoardView;

impl BoardView {
    /// Exports the selected layers as their export settings say: a file for each layer and each
    /// of its settings, and a PNG at 1× for a layer that has none (`sawit_board::design::export`).
    /// With nothing selected, the page: its top-level layers together, as one PNG at 1× named
    /// after the page (rulings P5-C13 and P6-C8). It is what the Export button and File ▸ Export
    /// do.
    ///
    /// - The files go into the project's `.nyawit/exports/`, made if it isn't there, for a canvas
    ///   in a project's `.nyawit` folder; any other canvas's go into `exports/` in Sawit's own
    ///   folder (ruling P5-C12).
    /// - A file is named after its layer, `<name>.<ext>`, with `@<scale>x` before the extension
    ///   for pixels at another scale than 1×, and a number after the name when the name is taken
    ///   (`nyawit::unique`), or a link is there (`nyawit::write_unique`).
    /// - Every export is made before any file is written: a layer the engine can't export (one
    ///   that paints nothing) is the error, and no file is written.
    ///
    /// The files written, in order; nothing in the document changes. Showing them is the
    /// caller's ([`Self::reveal_exports`]).
    pub fn export_selection(&mut self, cx: &mut Context<Self>) -> Result<Vec<PathBuf>, BoardError> {
        self.export(|board| export_files(board, None), cx)
    }

    /// File ▸ Export as SVG: as [`Self::export_selection`], with one SVG file for each selected
    /// layer whatever its settings, and for the page with nothing selected.
    pub(in crate::board) fn export_selection_as_svg(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<Vec<PathBuf>, BoardError> {
        self.export(|board| export_files(board, Some("svg")), cx)
    }

    /// File ▸ Export frames to PDF: the page's visible top-level frames, a PDF page each, as one
    /// file named after the page (`sawit_board::ops::frames_pdf`), written where
    /// [`Self::export_selection`] writes. A page without frames is the error, which says so.
    pub(in crate::board) fn export_frames_pdf(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<Vec<PathBuf>, BoardError> {
        self.export(
            |board| {
                let pdf = ops::frames_pdf(board)?;
                Ok(vec![(page_file_name(board, "pdf")?, pdf)])
            },
            cx,
        )
    }

    /// The canvas's one export writer (ruling P6-C8): makes the files with `make`, each one's name
    /// and bytes, then writes them into the canvas's exports folder ([`exports_folder`]), each
    /// under a name of its own there, as [`Self::export_selection`] says. The files are all made
    /// before any is written, so an export that fails writes nothing. What failed is the error.
    fn export(
        &mut self,
        make: impl FnOnce(&mut BoardDoc) -> Result<Vec<(String, Vec<u8>)>, BoardError>,
        cx: &mut Context<Self>,
    ) -> Result<Vec<PathBuf>, BoardError> {
        let (folder, project) = exports_folder(self.path(), || AppPaths::default_macos().root);
        let Some(board) = self.board_mut() else {
            return Err(BoardError::Engine(
                "the canvas file couldn't be opened".to_string(),
            ));
        };
        let files = make(board);
        // What the engine asked of the canvas meanwhile, as after every command it runs.
        self.handle_ui_requests(Vec::new(), cx);
        let files = files?;
        let made = match &project {
            Some(project) => nyawit::ensure(project, &folder),
            None => std::fs::create_dir_all(&folder),
        };
        made.map_err(|error| BoardError::Io(format!("{}: {error}", folder.display())))?;
        let mut written = Vec::with_capacity(files.len());
        for (name, bytes) in files {
            // Made new, at a name nothing is at: never through a link a cloned project holds
            // there (CANVAS-CORE-1).
            let path = nyawit::write_unique(&folder, &name, &bytes).map_err(|error| {
                BoardError::Io(format!("{}: {error}", folder.join(&name).display()))
            })?;
            written.push(path);
        }
        Ok(written)
    }

    /// After an export: shows the first of the files it wrote, `written`, in the Finder, or says
    /// in a toast why it failed. The Export button and File ▸ Export's commands end with it.
    pub(in crate::board) fn reveal_exports(
        &mut self,
        written: Result<Vec<PathBuf>, BoardError>,
        cx: &mut Context<Self>,
    ) {
        match written {
            Ok(written) => {
                if let Some(first) = written.first() {
                    cx.reveal_path(first);
                }
            }
            Err(error) => self.show_error(&error, cx),
        }
    }

    /// The Export button's click: exports the selected layers ([`Self::export_selection`]) and
    /// shows the first file in the Finder, or says in a toast why it couldn't.
    pub(super) fn export_and_reveal(&mut self, cx: &mut Context<Self>) {
        let written = self.export_selection(cx);
        self.reveal_exports(written, cx);
    }
}

/// Where the exports of the canvas file at `board` go, and the project they are made for: the
/// project's `.nyawit/exports/` for a file in a project's `.nyawit` folder, else `exports/` in
/// `app_root`, Sawit's own folder (ruling P5-C12).
pub(super) fn exports_folder(
    board: &Path,
    app_root: impl FnOnce() -> PathBuf,
) -> (PathBuf, Option<PathBuf>) {
    let nyawit = board
        .parent()
        .filter(|folder| folder.file_name() == Some(OsStr::new(nyawit::DIR)));
    match nyawit.and_then(Path::parent) {
        Some(project) => (nyawit::exports_dir(project), Some(project.to_path_buf())),
        None => (app_root().join(EXPORTS), None),
    }
}

/// The files exporting the selected layers makes, each one's name and bytes, in the order of the
/// layers and of their rows; with `format`, one file in that format for each layer instead,
/// whatever its rows. With nothing selected, one file of the page, its top-level layers together:
/// a PNG at 1×, or `format` (ruling P6-C8).
fn export_files(
    board: &mut BoardDoc,
    format: Option<&str>,
) -> Result<Vec<(String, Vec<u8>)>, BoardError> {
    let selection = board.session.ds()?.selection.clone();
    if selection.is_empty() {
        let format = format.unwrap_or(PAGE_FORMAT);
        let bytes = ops::export_page(board, format, 1.0)?;
        return Ok(vec![(page_file_name(board, format)?, bytes)]);
    }
    let doc = board.doc()?;
    let exports = guard::run("ui.export", || Ok(exports_of(&doc, &selection, format)))?;
    if exports.is_empty() {
        return Err(BoardError::Engine("there is nothing to export".to_string()));
    }
    let mut files = Vec::with_capacity(exports.len());
    for (id, stem, row) in exports {
        let bytes = export_bytes(board, &[id], &row)?;
        files.push((file_name(&stem, &row), bytes));
    }
    Ok(files)
}

/// The name of a file of the whole page on show: `<page name>.<ext>`, by [`file_stem`]'s rules.
fn page_file_name(board: &BoardDoc, ext: &str) -> Result<String, BoardError> {
    let ds = board.session.ds()?;
    let name = ds.doc.node(ds.page).map_or("", |page| page.name.as_str());
    Ok(format!("{}.{ext}", file_stem(name, "Page")))
}

/// The exports of layers `ids` of `doc`: each layer, the name of its files, and each of its rows.
/// Layers whose settings differ are exported each with its own rows. With `format`, each layer
/// has one row of that format instead, at 1×.
fn exports_of(
    doc: &Document,
    ids: &[NodeId],
    format: Option<&str>,
) -> Vec<(NodeId, String, ExportRow)> {
    let sel = DesignSelection::read(doc, ids);
    let shared = export_rows(&sel);
    let mut exports = Vec::new();
    for id in &sel.ids {
        let rows = match format {
            Some(format) => vec![ExportRow {
                format: format.to_string(),
                scale: 1.0,
            }],
            None if shared.is_empty() => export_rows(&DesignSelection::read(doc, &[*id])),
            None => shared.clone(),
        };
        let stem = doc.node(*id).map_or_else(
            || "Layer".to_string(),
            |node| file_stem(&node.name, node.kind.label()),
        );
        exports.extend(rows.into_iter().map(|row| (*id, stem.clone(), row)));
    }
    exports
}

/// The name of a layer's files for a layer called `name`, of kind `kind`: the name without what a
/// file name can't hold (a `/`, a `:`, a control character), nor a dot it starts with, which
/// would hide the file; the kind when that leaves nothing.
pub(super) fn file_stem(name: &str, kind: &str) -> String {
    let kept: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let kept = kept.trim().trim_start_matches('.').trim();
    if kept.is_empty() {
        kind.to_string()
    } else {
        kept.to_string()
    }
}

/// The file name of `row`'s export of a layer whose files are called `stem`: `Card.png`, and
/// `Card@2x.png` for pixels at another scale than 1×.
pub(super) fn file_name(stem: &str, row: &ExportRow) -> String {
    if PIXEL_FORMATS.contains(&row.format.as_str()) && row.scale != 1.0 {
        format!("{stem}@{}x.{}", number_text(row.scale), row.format)
    } else {
        format!("{stem}.{}", row.format)
    }
}
