//! The Export section's settings, and the files an export writes and where.

use std::path::{Path, PathBuf};

use gpui_kit::TestAppContext;
use sawit_board::design::export::ExportRow;
use serde_json::json;

// Not `super::*`: gpui-kit's `test` attribute would come with it, in place of the standard one.
use super::super::tests::{Design, Scratch, click, drawn, open, open_in_window, prop, undo_steps};
use super::files::{exports_folder, file_name, file_stem};
use super::{scale_name, with_another};

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
