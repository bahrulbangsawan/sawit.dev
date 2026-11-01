//! The Export section (spec "Design panel" 9): the layers' export settings, a format and a scale
//! each, and the button that exports the layers as they say ([`BoardView::export_selection`]).
//!
//! - A layer with no setting shows one row, PNG at 1×, which is what exporting it makes. The row
//!   goes into the layer once it is changed, or another is added.
//! - Layers whose settings differ show "Mixed", and its "Replace all" gives them all that one
//!   row. Until then the Export button exports each with its own settings.
//! - The settings change only through `sawit_board::design::export::set_export_rows`, which keeps
//!   what a setting holds besides its format and scale while its row is left as it is.
//!
//! It also holds the canvas's one export writer (ruling P6-C8), which File ▸ Export, Export as SVG
//! and Export frames to PDF write through too (`super::super::commands`): into the exports folder,
//! then the first file shows in the Finder ([`BoardView::reveal_exports`]).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use figcraft_doc::{Document, NodeId};
use figcraft_engine::guard;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::export::{ExportRow, export_bytes, export_rows, set_export_rows};
use sawit_board::design::{DesignSelection, Edit, Mixed};
use sawit_board::{BoardDoc, BoardError, ops};
use sawit_core::instances::AppPaths;
use sawit_core::nyawit;
use serde_json::Value;

use super::number_field::number_text;
use super::paints::{ADD_ICON, REMOVE_ICON};
use super::{
    Choices, choice_menu, guarded, icon_button, mixed_row, on_click_edit, row, row_button, section,
};
use crate::board::BoardView;
use crate::board::option_icons;

/// The formats a row can be set to: each one's extension, and what the menu calls it.
pub(super) const FORMATS: [(&str, &str); 4] = [
    ("png", "PNG"),
    ("jpg", "JPG"),
    ("svg", "SVG"),
    ("pdf", "PDF"),
];
/// The scales a row can be set to.
const SCALES: [f64; 5] = [0.5, 1.0, 2.0, 3.0, 4.0];
/// The formats made of pixels, which a scale makes larger or smaller: the others are the same
/// file whatever the scale.
const PIXEL_FORMATS: [&str; 3] = ["png", "jpg", "webp"];
/// The folder in Sawit's own folder that takes the exports of a canvas outside a project.
const EXPORTS: &str = "exports";
/// The format the page is exported in with nothing selected, when no other is asked for.
const PAGE_FORMAT: &str = "png";

/// The Export section for the layers of `sel`.
pub(super) fn render(sel: &Rc<DesignSelection>, cx: &mut Context<BoardView>) -> AnyElement {
    let this = cx.entity().downgrade();
    let shown = guarded("ui.exports", Vec::new(), || export_rows(sel));
    let settings = sel.get("exports");
    // Whether the layers hold settings of their own, which a row's button can remove.
    let held = matches!(&settings, Mixed::Same(Value::Array(held)) if !held.is_empty());
    let add = icon_button("design-exports-add", ADD_ICON, "Add export")
        .debug_selector(|| "design-exports-add".into())
        .on_click(on_click_edit(&this, |sel| {
            Some(set_export_rows(sel, &with_another(export_rows(sel))))
        }))
        .into_any_element();
    let mut rows: Vec<AnyElement> = if settings == Mixed::Mixed {
        let replace = |sel: &DesignSelection| Some(set_export_rows(sel, &[ExportRow::default()]));
        vec![mixed_row("exports", &this, replace, cx)]
    } else {
        let rows = shown.iter().enumerate();
        rows.map(|(index, row)| export_row(index, row, held, &this))
            .collect()
    };
    rows.push(export_button(sel.ids.len(), &this));
    section("export", "Export", vec![add], rows, cx)
}

/// Row `index`, `row`: its format and its scale, and while the layers hold settings of their
/// own (`held`), the button that removes it.
fn export_row(
    index: usize,
    shown: &ExportRow,
    held: bool,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let id = format!("exports-{index}");
    let drawn = row()
        .items_center()
        .child(format_menu(&id, index, shown, view))
        .child(scale_menu(&id, index, shown, view));
    if !held {
        return drawn.into_any_element();
    }
    let remove = row_button(
        &id,
        "remove",
        REMOVE_ICON,
        "Remove export",
        view,
        move |sel| {
            let mut rows = export_rows(sel);
            (index < rows.len()).then(|| {
                rows.remove(index);
                set_export_rows(sel, &rows)
            })
        },
    );
    drawn.child(remove).into_any_element()
}

/// The format menu of row `index`, which shows `shown`'s.
fn format_menu(
    id: &str,
    index: usize,
    shown: &ExportRow,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let current = FORMATS
        .iter()
        .map(|(format, _)| *format)
        .find(|format| *format == shown.format);
    let choices = || {
        let named = FORMATS.iter();
        named
            .map(|(format, name)| (*format, (*name).into()))
            .collect()
    };
    let view = view.clone();
    let pick = move |format: &&'static str, cx: &mut App| {
        let format = *format;
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(change_row(index, |row| row.format = format.to_string()), cx);
        });
    };
    let menu = format!("{id}-format");
    let label = shown.format.to_uppercase();
    let choices = Choices::new(choices, current, pick)
        .with_pictures(|format: &&'static str| option_icons::export_format(format));
    choice_menu(&menu, label, "Format", choices, false).into_any_element()
}

/// The scale menu of row `index`, which shows `shown`'s. Off for a format that isn't pixels.
fn scale_menu(
    id: &str,
    index: usize,
    shown: &ExportRow,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let current = SCALES.into_iter().find(|scale| *scale == shown.scale);
    let choices = || {
        let named = SCALES.into_iter();
        named.map(|scale| (scale, scale_name(scale))).collect()
    };
    let view = view.clone();
    let pick = move |scale: &f64, cx: &mut App| {
        let scale = *scale;
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(change_row(index, |row| row.scale = scale), cx);
        });
    };
    let pixels = PIXEL_FORMATS.contains(&shown.format.as_str());
    let menu = format!("{id}-scale");
    let choices = Choices::new(choices, current, pick);
    choice_menu(&menu, scale_name(shown.scale), "Scale", choices, !pixels).into_any_element()
}

/// A scale as the menu names it: "2×".
fn scale_name(scale: f64) -> SharedString {
    format!("{}×", number_text(scale)).into()
}

/// The edit that changes row `index` of the layers' export rows, as they are when it is made,
/// with `change`. `None` when there is no such row.
fn change_row(
    index: usize,
    change: impl FnOnce(&mut ExportRow),
) -> impl FnOnce(&DesignSelection) -> Option<Edit> {
    move |sel| {
        let mut rows = export_rows(sel);
        change(rows.get_mut(index)?);
        Some(set_export_rows(sel, &rows))
    }
}

/// `rows` and one more: the last one's format at the next scale it isn't at yet (1× when it is at
/// every larger one), or PNG at 1× after none.
fn with_another(mut rows: Vec<ExportRow>) -> Vec<ExportRow> {
    let next = match rows.last() {
        None => ExportRow::default(),
        Some(last) => {
            let taken = |scale: f64| {
                (rows.iter()).any(|row| row.format == last.format && row.scale == scale)
            };
            let mut larger = SCALES.into_iter().filter(|scale| *scale > last.scale);
            let scale = larger.find(|scale| !taken(*scale));
            ExportRow {
                format: last.format.clone(),
                scale: scale.unwrap_or(1.0),
            }
        }
    };
    rows.push(next);
    rows
}

/// The Export button, for `count` layers: it exports them, then shows the first file in the
/// Finder; a failed export says why in a toast.
fn export_button(count: usize, view: &WeakEntity<BoardView>) -> AnyElement {
    let label = if count == 1 {
        "Export 1 layer".to_string()
    } else {
        format!("Export {count} layers")
    };
    let view = view.clone();
    Button::new("design-export")
        .outline()
        .small()
        .w_full()
        .label(label)
        .debug_selector(|| "design-export".into())
        .on_click(move |_, _, cx| {
            let _ = view.update(cx, |view, cx| view.export_and_reveal(cx));
        })
        .into_any_element()
}

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
    fn export_and_reveal(&mut self, cx: &mut Context<Self>) {
        let written = self.export_selection(cx);
        self.reveal_exports(written, cx);
    }
}

/// Where the exports of the canvas file at `board` go, and the project they are made for: the
/// project's `.nyawit/exports/` for a file in a project's `.nyawit` folder, else `exports/` in
/// `app_root`, Sawit's own folder (ruling P5-C12).
fn exports_folder(board: &Path, app_root: impl FnOnce() -> PathBuf) -> (PathBuf, Option<PathBuf>) {
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
fn file_stem(name: &str, kind: &str) -> String {
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
fn file_name(stem: &str, row: &ExportRow) -> String {
    if PIXEL_FORMATS.contains(&row.format.as_str()) && row.scale != 1.0 {
        format!("{stem}@{}x.{}", number_text(row.scale), row.format)
    } else {
        format!("{stem}.{}", row.format)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use gpui_kit::TestAppContext;
    use sawit_board::design::export::ExportRow;
    use serde_json::json;

    // Not `super::*`: gpui-kit's `test` attribute would come with it, in place of the standard one.
    use super::super::tests::{
        Design, Scratch, click, drawn, open, open_in_window, prop, undo_steps,
    };
    use super::{exports_folder, file_name, file_stem, scale_name, with_another};

    /// The canvas file of a project at `project`, `.nyawit/canvas.sawit`, written with the
    /// layers `design` makes; their ids.
    fn project_canvas(
        project: &Path,
        make: impl FnOnce(&mut Design) -> Vec<figcraft_doc::NodeId>,
    ) -> (PathBuf, Vec<figcraft_doc::NodeId>) {
        let path = sawit_core::nyawit::board_file(project);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut design = Design::new();
        let ids = make(&mut design);
        design.save(&path);
        (path, ids)
    }

    /// A 100 × 60 rectangle called `name` at (`x`, 0).
    fn card(design: &mut Design, name: &str, x: f64) -> figcraft_doc::NodeId {
        design.make(
            json!({"type": "rectangle", "name": name, "x": x, "y": 0, "width": 100, "height": 60}),
        )
    }

    /// Ruling P5-C12: a project canvas exports the selection into the project's
    /// `.nyawit/exports/`, made with its `.gitignore`, and returns what it wrote.
    #[gpui_kit::test]
    fn export_writes_into_nyawit_exports(cx: &mut TestAppContext) {
        let tmp = Scratch::new("export");
        let project = tmp.0.join("proj");
        let (path, ids) = project_canvas(&project, |design| vec![card(design, "Card", 0.0)]);
        let view = open(cx, &path, &ids);
        let written = view
            .update(cx, |view, cx| view.export_selection(cx))
            .unwrap();
        let exports = project.join(".nyawit").join("exports");
        assert_eq!(written, [exports.join("Card.png")]);
        let png = std::fs::read(&written[0]).unwrap();
        assert!(png.starts_with(b"\x89PNG"), "{:02x?}", &png[..8]);
        assert!(project.join(".nyawit").join(".gitignore").exists());
        // Again: a file of its own beside the first.
        let again = view
            .update(cx, |view, cx| view.export_selection(cx))
            .unwrap();
        assert_eq!(again, [exports.join("Card-2.png")]);
        // Exporting changes nothing in the file.
        assert_eq!(undo_steps(&view, cx), 0);
    }

    /// CANVAS-CORE-1: a cloned project's `.nyawit/exports/` can hold a link, named as an export
    /// would be, to a file that isn't there yet, anywhere. The export takes a name of its own
    /// beside it, and makes nothing where the link points.
    #[gpui_kit::test]
    fn an_export_never_makes_a_file_through_a_link(cx: &mut TestAppContext) {
        let tmp = Scratch::new("export-link");
        let project = tmp.0.join("proj");
        let (path, ids) = project_canvas(&project, |design| vec![card(design, "Card", 0.0)]);
        let exports = project.join(".nyawit").join("exports");
        std::fs::create_dir_all(&exports).unwrap();
        let elsewhere = tmp.0.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(elsewhere.join("made"), exports.join("Card.png")).unwrap();
        let view = open(cx, &path, &ids);
        let written = view
            .update(cx, |view, cx| view.export_selection(cx))
            .unwrap();
        assert_eq!(written, [exports.join("Card-2.png")]);
        assert!(std::fs::read(&written[0]).unwrap().starts_with(b"\x89PNG"));
        assert!(
            std::fs::read_dir(&elsewhere).unwrap().next().is_none(),
            "the link's target was made"
        );
    }

    /// A layer's own settings make its files, a format and a scale each; with nothing selected,
    /// the page is exported, its top-level layers together in one PNG (rulings P5-C13, P6-C8).
    #[gpui_kit::test]
    fn exports_follow_the_settings_and_fall_back_to_the_page(cx: &mut TestAppContext) {
        let tmp = Scratch::new("export-settings");
        let project = tmp.0.join("proj");
        let (path, ids) = project_canvas(&project, |design| {
            let first = card(design, "Card", 0.0);
            let settings = json!([
                {"format": "png", "size": {"by": "scale", "value": 2.0}},
                {"format": "svg", "size": {"by": "scale", "value": 1.0}},
            ]);
            design.set(&[first], json!({ "exports": settings }));
            vec![first, card(design, "Icons/Arrow", 200.0)]
        });
        let view = open(cx, &path, &ids[..1]);
        let exports = project.join(".nyawit").join("exports");
        let written = view
            .update(cx, |view, cx| view.export_selection(cx))
            .unwrap();
        assert_eq!(
            written,
            [exports.join("Card@2x.png"), exports.join("Card.svg")]
        );
        let svg = std::fs::read_to_string(&written[1]).unwrap();
        assert!(svg.contains("<svg"), "{svg}");
        // Nothing selected: the page, both cards in one PNG named after it, whatever their
        // settings.
        view.update(cx, |view, _| {
            view.board_mut().unwrap().session.set_selection(Vec::new());
        });
        let written = view
            .update(cx, |view, cx| view.export_selection(cx))
            .unwrap();
        assert_eq!(written, [exports.join("Page 1.png")]);
        let png = std::fs::read(&written[0]).unwrap();
        let picture = image::load_from_memory_with_format(&png, image::ImageFormat::Png).unwrap();
        // From the first card's left edge to the second's right edge.
        assert_eq!((picture.width(), picture.height()), (300, 60));
    }

    /// File ▸ Export as SVG makes one SVG of each selected layer whatever its settings, or of the
    /// page, and Export frames to PDF one PDF of the page's frames; they write where Export does
    /// (ruling P6-C8).
    #[gpui_kit::test]
    fn export_as_svg_and_frames_pdf_write_where_export_does(cx: &mut TestAppContext) {
        let tmp = Scratch::new("export-svg-pdf");
        let project = tmp.0.join("proj");
        let (path, ids) = project_canvas(&project, |design| {
            let first = card(design, "Card", 0.0);
            let settings = json!([
                {"format": "png", "size": {"by": "scale", "value": 2.0}},
                {"format": "jpg", "size": {"by": "scale", "value": 1.0}},
            ]);
            design.set(&[first], json!({ "exports": settings }));
            let slide = json!({
                "type": "frame", "name": "Slide", "x": 200, "y": 0, "width": 160, "height": 90,
            });
            vec![first, design.make(slide)]
        });
        let view = open(cx, &path, &ids[..1]);
        let exports = project.join(".nyawit").join("exports");
        let svg = view
            .update(cx, |view, cx| view.export_selection_as_svg(cx))
            .unwrap();
        assert_eq!(svg, [exports.join("Card.svg")]);
        let text = std::fs::read_to_string(&svg[0]).unwrap();
        assert!(text.starts_with("<svg"), "{text}");
        view.update(cx, |view, _| {
            view.board_mut().unwrap().session.set_selection(Vec::new());
        });
        let page = view
            .update(cx, |view, cx| view.export_selection_as_svg(cx))
            .unwrap();
        assert_eq!(page, [exports.join("Page 1.svg")]);
        let pdf = view
            .update(cx, |view, cx| view.export_frames_pdf(cx))
            .unwrap();
        assert_eq!(pdf, [exports.join("Page 1.pdf")]);
        let bytes = std::fs::read(&pdf[0]).unwrap();
        assert!(bytes.starts_with(b"%PDF"), "{:02x?}", &bytes[..8]);
        assert_eq!(undo_steps(&view, cx), 0);
    }

    /// The Export section: a row's format and scale are picked from its menus, rows are added
    /// and removed, and each change is one undo step of the layer's settings.
    #[gpui_kit::test]
    fn the_export_section_edits_the_settings(cx: &mut TestAppContext) {
        let tmp = Scratch::new("export-rows");
        let path = tmp.0.join("design.sawit");
        let mut design = Design::new();
        let id = card(&mut design, "Card", 0.0);
        design.save(&path);
        let (view, mut cx) = open_in_window(cx, &path, &[id]);
        let settings = |cx: &gpui_kit::VisualTestContext| prop(&view, id, "exports", cx);
        // A layer without settings shows PNG at 1×, which isn't in the file, and can't be removed.
        assert!(drawn(&mut cx, "design-exports-0-format"));
        assert!(!drawn(&mut cx, "design-exports-0-remove"));
        assert_eq!(settings(&cx), serde_json::Value::Null);
        click(&mut cx, "design-exports-0-scale");
        click(&mut cx, "design-exports-0-scale-3×");
        let held = settings(&cx);
        assert_eq!(held[0]["format"], json!("png"), "{held}");
        assert_eq!(held[0]["size"]["value"], json!(3.0), "{held}");
        click(&mut cx, "design-exports-0-format");
        click(&mut cx, "design-exports-0-format-SVG");
        assert_eq!(settings(&cx)[0]["format"], json!("svg"));
        click(&mut cx, "design-exports-add");
        assert_eq!(settings(&cx).as_array().map(Vec::len), Some(2));
        click(&mut cx, "design-exports-1-remove");
        click(&mut cx, "design-exports-0-remove");
        assert_eq!(settings(&cx), serde_json::Value::Null);
        assert!(drawn(&mut cx, "design-export"));
    }

    fn row(format: &str, scale: f64) -> ExportRow {
        ExportRow {
            format: format.into(),
            scale,
        }
    }

    /// Ruling P5-C12: a project canvas exports into the project's `.nyawit/exports/`, any other
    /// canvas into `exports/` in Sawit's own folder.
    #[test]
    fn exports_go_to_the_project_or_to_sawits_own_folder() {
        let app = || PathBuf::from("/app");
        assert_eq!(
            exports_folder(Path::new("/proj/.nyawit/canvas.sawit"), app),
            (
                PathBuf::from("/proj/.nyawit/exports"),
                Some(PathBuf::from("/proj"))
            )
        );
        // The canvas without a project, and a file opened from a project's tree.
        for board in ["/app/canvas.sawit", "/proj/designs/card.sawit"] {
            assert_eq!(
                exports_folder(Path::new(board), app),
                (PathBuf::from("/app/exports"), None),
                "{board}"
            );
        }
    }

    #[test]
    fn files_are_named_after_their_layer_and_scale() {
        assert_eq!(file_name("Card", &row("png", 1.0)), "Card.png");
        assert_eq!(file_name("Card", &row("png", 2.0)), "Card@2x.png");
        assert_eq!(file_name("Card", &row("jpg", 0.5)), "Card@0.5x.jpg");
        // A vector file is the same at any scale.
        assert_eq!(file_name("Card", &row("svg", 2.0)), "Card.svg");
        assert_eq!(file_name("Card", &row("pdf", 3.0)), "Card.pdf");
    }

    #[test]
    fn a_layers_name_is_made_fit_for_a_file() {
        assert_eq!(file_stem("Card", "Rectangle"), "Card");
        assert_eq!(
            file_stem("Icons/Arrow: left", "Vector"),
            "Icons-Arrow- left"
        );
        assert_eq!(file_stem(".hidden", "Frame"), "hidden");
        assert_eq!(file_stem("  ", "Frame"), "Frame");
        assert_eq!(file_stem("a\nb", "Text"), "a-b");
    }

    #[test]
    fn another_row_is_the_next_scale() {
        assert_eq!(with_another(Vec::new()), [row("png", 1.0)]);
        assert_eq!(
            with_another(vec![row("png", 1.0)]),
            [row("png", 1.0), row("png", 2.0)]
        );
        assert_eq!(
            with_another(vec![row("svg", 4.0)]),
            [row("svg", 4.0), row("svg", 1.0)]
        );
        assert_eq!(scale_name(0.5).as_ref(), "0.5×");
        assert_eq!(scale_name(2.0).as_ref(), "2×");
    }
}
