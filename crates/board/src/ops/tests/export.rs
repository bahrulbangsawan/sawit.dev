//! The export ops, File ▸ Export and Edit ▸ Copy as.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figcraft_doc::{ExportSetting, NodeId};
use figcraft_engine::cmd::file::export_area;
use serde_json::json;

use super::*;
use crate::design::export::ExportRow;

/// The width an SVG file gives itself, in its `<svg ...>` tag.
fn svg_width(svg: &str) -> f64 {
    let tag = &svg[..svg.find('>').expect("an <svg> tag")];
    let (_, after) = tag.split_once(" width=\"").expect("a width");
    let (width, _) = after.split_once('"').expect("a quoted width");
    width.parse().expect("a number")
}

/// Review Focus 3 and ruling P6-C13: the whole page is exported, whatever is selected.
#[test]
fn export_without_selection_exports_page() {
    let mut board = board();
    let left = square_at(&mut board, 0.0);
    square_at(&mut board, 1000.0);
    board.session.set_selection(vec![left]);
    let steps = board.session.undo_depth();
    let svg = String::from_utf8(export_page(&mut board, "svg", 1.0).unwrap()).unwrap();
    assert!(svg.starts_with("<svg"), "{svg}");
    // From the left square's left edge to the right one's right edge: both are in it.
    let width = svg_width(&svg);
    assert!(width >= 1100.0, "width {width}: {svg}");
    // The selected square alone is a tenth of that.
    let one = crate::design::export::export_bytes(
        &mut board,
        &[left],
        &ExportRow {
            format: "svg".into(),
            scale: 1.0,
        },
    );
    let one = svg_width(&String::from_utf8(one.unwrap()).unwrap());
    assert!(one < 200.0, "width {one}");
    // Exporting changes nothing: the selection stays, and there is no undo step.
    assert_eq!(board.session.selection(), [left]);
    assert_eq!(board.session.undo_depth(), steps);
    // At another scale and format: a PNG.
    let png = export_page(&mut board, "png", 0.5).unwrap();
    assert!(png.starts_with(b"\x89PNG"), "{:02x?}", &png[..8]);
}

#[test]
fn an_empty_page_or_an_unknown_format_exports_nothing() {
    let mut board = board();
    let error = export_page(&mut board, "png", 1.0).unwrap_err();
    assert!(error.to_string().contains("nothing to export"), "{error}");
    square_at(&mut board, 0.0);
    let error = export_page(&mut board, "gif", 1.0).unwrap_err();
    assert!(
        error.to_string().contains("not an export format"),
        "{error}"
    );
}

/// The text of an SVG file after its optional `<?xml ...?>` prolog.
fn after_prolog(svg: &str) -> &str {
    let svg = svg.trim_start();
    match svg.strip_prefix("<?xml") {
        Some(rest) => (rest.split_once("?>"))
            .map_or(rest, |(_, after)| after)
            .trim_start(),
        None => svg,
    }
}

#[test]
fn export_png_is_base64_png() {
    let mut board = board();
    square_at(&mut board, 0.0);
    let reply = ask(&mut board, json!({"op": "export_png"}));
    let encoded = reply["png_base64"].as_str().expect("png_base64");
    let png = STANDARD.decode(encoded).unwrap();
    assert!(
        png.starts_with(b"\x89PNG\r\n\x1a\n"),
        "{:02x?}",
        &png[..png.len().min(8)]
    );
    // At 1×, as the page is exported at that scale: not at another, which makes another size.
    let at = |board: &mut BoardDoc, scale| png_size(&export_page(board, "png", scale).unwrap());
    assert_eq!(png_size(&png), at(&mut board, 1.0));
    assert_ne!(png_size(&png), at(&mut board, 2.0));
}

#[test]
fn export_svg_has_svg_root() {
    let mut board = board();
    square_at(&mut board, 0.0);
    let reply = ask(&mut board, json!({"op": "export_svg"}));
    let svg = reply["svg"].as_str().expect("svg");
    assert!(after_prolog(svg).starts_with("<svg"), "{svg}");
    assert!(svg.trim_end().ends_with("</svg>"), "{svg}");
}

/// Spec, "Agent wire contract": the ops answer the whole page, and exporting changes nothing
/// in it.
#[test]
fn the_export_ops_export_the_whole_page_and_change_nothing() {
    let mut board = board();
    let left = square_at(&mut board, 0.0);
    square_at(&mut board, 1000.0);
    board.session.set_selection(vec![left]);
    let (doc, steps) = (board.doc().unwrap(), board.session.undo_depth());

    // Both squares are in the picture, though one is selected.
    let svg = ask(&mut board, json!({"op": "export_svg"}));
    let width = svg_width(svg["svg"].as_str().unwrap());
    assert!(width >= 1100.0, "width {width}");
    let png = ask(&mut board, json!({"op": "export_png"}));
    let png = STANDARD
        .decode(png["png_base64"].as_str().unwrap())
        .unwrap();
    assert!(png_size(&png).0 >= 1100, "{:?}", png_size(&png));

    assert_eq!(board.session.selection(), [left]);
    assert_eq!(board.session.undo_depth(), steps);
    assert!(Arc::ptr_eq(&doc, &board.doc().unwrap()));
}

#[test]
fn the_export_ops_of_an_empty_page_are_errors_not_panics() {
    let mut board = board();
    for op in ["export_svg", "export_png"] {
        let error = refusal(&mut board, json!({"op": op}));
        assert!(error.contains("nothing to export"), "{op}: {error}");
    }
}

/// Review Focus 3: a page without frames says why it has no PDF.
#[test]
fn the_frames_of_the_page_make_a_pdf() {
    let mut board = board();
    let error = frames_pdf(&mut board).unwrap_err();
    assert!(
        error.to_string().contains("the page has no visible frames"),
        "{error}"
    );
    let mut design = design();
    for x in [0, 400] {
        let frame = json!({"type": "frame", "x": x, "y": 0, "width": 300, "height": 200});
        design.execute("node.create", &frame).unwrap();
    }
    let pdf = frames_pdf(&mut design).unwrap();
    assert!(pdf.starts_with(b"%PDF"), "{:02x?}", &pdf[..8]);
}

/// Review Focus 5 and ruling P6-C14: Copy as PNG makes no image longer than 4096 px.
#[test]
fn copy_png_is_capped() {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
    let wide = copy_png_scale((10000.0, 500.0), 2.0);
    assert!(close(wide, 4096.0 / 10000.0), "{wide}");
    let tall = copy_png_scale((500.0, 10000.0), 2.0);
    assert!(close(tall, 4096.0 / 10000.0), "{tall}");
    assert!(close(copy_png_scale((100.0, 50.0), 2.0), 2.0));
    // Exactly 4096 px long at the scale wanted, and nothing to measure: as wanted.
    assert!(close(copy_png_scale((2048.0, 10.0), 2.0), 2.0));
    assert!(close(copy_png_scale((0.0, 0.0), 2.0), 2.0));
}

/// The width and height of the PNG file `png`.
fn png_size(png: &[u8]) -> (u32, u32) {
    figcraft_render::images::dimensions(png).expect("a PNG")
}

/// Text, its own CSS or SVG, as Copy as gives it.
fn copied_text(board: &mut BoardDoc, id: &str) -> String {
    match copy_as(board, id).unwrap() {
        Copied::Text(text) => text,
        Copied::Png(_) => panic!("{id} copied a PNG"),
    }
}

/// A PNG file, as Copy as PNG gives it.
fn copied_png(board: &mut BoardDoc) -> Vec<u8> {
    match copy_as(board, "edit.copyAsPng").unwrap() {
        Copied::Png(png) => png,
        Copied::Text(text) => panic!("Copy as PNG copied text: {text}"),
    }
}

#[test]
fn copy_as_text_copies_the_text_of_each_selected_layer() {
    let mut board = board();
    let note = made(
        &mut board,
        "board.sticky",
        json!({"x": 0, "y": 0, "text": "Ship it"}),
    );
    let shape = json!({"shape": "ellipse", "x": 300, "y": 0, "text": "Review"});
    let shape = made(&mut board, "board.shape", shape);
    let empty = made(&mut board, "board.sticky", json!({"x": 600, "y": 0}));
    let text = json!({"type": "text", "text": "Notes", "x": 0, "y": 400});
    let text = made(&mut board, "node.create", text);
    // A line each, in the order they were selected; the empty sticky has no line.
    board.session.set_selection(vec![shape, note, empty, text]);
    assert_eq!(
        copied_text(&mut board, "edit.copyAsText"),
        "Review\nShip it\nNotes"
    );
    // A frame's text is the text of the text layers in it, each copied once.
    let mut design = design();
    let frame = json!({"type": "frame", "width": 300, "height": 200});
    let frame = made(&mut design, "node.create", frame);
    let texts: Vec<NodeId> = ["Title", "Body"]
        .into_iter()
        .map(|text| {
            let text = json!({"type": "text", "text": text, "parent": frame});
            made(&mut design, "node.create", text)
        })
        .collect();
    design.session.set_selection(vec![frame, texts[0]]);
    assert_eq!(copied_text(&mut design, "edit.copyAsText"), "Title\nBody");
}

#[test]
fn copy_as_text_and_css_need_something_selected_to_copy() {
    let mut board = board();
    for id in ["edit.copyAsText", "edit.copyAsCss"] {
        let error = copy_as(&mut board, id).unwrap_err();
        assert!(error.is_unavailable(), "{id}: {error:?}");
        assert_eq!(error.for_user(), "Nothing is selected", "{id}");
    }
    let square = square_at(&mut board, 0.0);
    board.session.set_selection(vec![square]);
    let error = copy_as(&mut board, "edit.copyAsText").unwrap_err();
    assert_eq!(error.to_string(), "the selection has no text");
    let error = copy_as(&mut board, "edit.copy").unwrap_err();
    assert_eq!(error.to_string(), "`edit.copy` is no Copy as command");
}

#[test]
fn copy_as_css_is_the_first_selected_layers() {
    let mut design = design();
    let rectangle = json!({"type": "rectangle", "width": 120, "height": 80});
    let first = made(&mut design, "node.create", rectangle);
    let ellipse = json!({"type": "ellipse", "width": 30, "height": 30});
    let second = made(&mut design, "node.create", ellipse);
    design.session.set_selection(vec![first, second]);
    let css = copied_text(&mut design, "edit.copyAsCss");
    assert!(css.contains("width: 120px"), "{css}");
    assert!(!css.contains("width: 30px"), "{css}");
}

/// Copy as SVG and PNG copy the selection together, and with nothing selected, the page, as
/// File ▸ Export does. Nothing in the document changes.
#[test]
fn copy_as_svg_and_png_copy_the_selection_or_the_page() {
    let mut board = board();
    let left = square_at(&mut board, 0.0);
    square_at(&mut board, 1000.0);
    board.session.set_selection(vec![left]);
    let steps = board.session.undo_depth();
    let svg = copied_text(&mut board, "edit.copyAsSvg");
    assert!(svg.starts_with("<svg"), "{svg}");
    assert!(svg_width(&svg) < 200.0, "{svg}");
    // At 2×, as FigCraft's own app copies.
    let doc = board.doc().unwrap();
    let area = export_area(&doc, &[left], &ExportSetting::default()).unwrap();
    let at_2x = |side: f64| (side * 2.0).ceil() as u32;
    assert_eq!(
        png_size(&copied_png(&mut board)),
        (at_2x(area.width()), at_2x(area.height()))
    );
    // Nothing selected: the page, both squares.
    board.session.set_selection(Vec::new());
    assert!(svg_width(&copied_text(&mut board, "edit.copyAsSvg")) >= 1100.0);
    assert!(png_size(&copied_png(&mut board)).0 >= 2200);
    assert_eq!(board.session.undo_depth(), steps);
    assert!(board.session.selection().is_empty());
}

/// Review Focus 5 and ruling P6-C14: the PNG's longest side is at most 4096 px, and an area
/// too long for that at the engine's least scale is refused instead of copied longer.
#[test]
fn copy_as_png_caps_the_longest_side() {
    let mut design = design();
    let wide = json!({"type": "rectangle", "width": 10000, "height": 500});
    let wide = made(&mut design, "node.create", wide);
    design.session.set_selection(vec![wide]);
    let (width, height) = png_size(&copied_png(&mut design));
    assert_eq!(width, 4096);
    // 500 at 4096 / 10000, on whole pixels.
    assert_eq!(height, 205);
    let longer = json!({"ids": [wide], "props": {"width": 500_000}});
    design.execute("node.set", &longer).unwrap();
    let error = copy_as(&mut design, "edit.copyAsPng").unwrap_err();
    assert_eq!(
        error.to_string(),
        "The selection is too large to copy as PNG"
    );
    design.session.set_selection(Vec::new());
    let error = copy_as(&mut design, "edit.copyAsPng").unwrap_err();
    assert_eq!(error.to_string(), "The page is too large to copy as PNG");
}
