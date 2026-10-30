//! Copy and paste on the canvas: across its tabs, and with other apps.
//!
//! Each canvas tab has a FigCraft session of its own, and so a clipboard of its own. A copy or a
//! cut also keeps that clipboard in one place for the whole app ([`after_copy`]), and a paste
//! first takes the layers kept there into its session ([`before_paste`]): layers copied in one tab
//! paste in any other.
//!
//! The system clipboard can't hold FigCraft's layers, so a copy puts a marker there instead, which
//! [`after_copy`] returns and [`marker`] gives back. While the system clipboard still holds that
//! marker, a paste pastes the copied layers. An image, SVG or text copied in another app since is
//! newer, and pastes instead ([`paste_source`], [`paste`]), as in FigCraft's own app.
//!
//! While text is edited, copy, cut and paste act on its characters ([`copy_text`], [`cut_text`],
//! [`paste_text`]).

use std::sync::{Mutex, MutexGuard, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figcraft_doc::NodeId;
use figcraft_engine::cmd::create::PLACED_IMAGE_MAX_SIDE;
use figcraft_engine::params::id_of;
use figcraft_engine::{Clipboard, guard, looks_like_svg, textedit};
use serde_json::{Value, json};

use crate::{BoardDoc, BoardError};

/// What a paste pastes, as [`paste_source`] decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteSource {
    /// The layers copied last on the canvas, in this tab or another.
    Canvas,
    /// Text another app copied: it becomes a text layer.
    Text(String),
    /// An image another app copied, in a format FigCraft reads (PNG, JPEG, …): it becomes an
    /// image layer.
    Image(Vec<u8>),
    /// SVG another app copied as text: its shapes come in, in a frame.
    Svg(String),
}

/// The newest copy in the app.
struct Slot {
    /// How many copies the canvas has made: the number in the newest one's marker.
    generation: u64,
    /// The newest copy: the clipboard of the session that made it, as it was then.
    copied: Option<Clipboard>,
}

/// The app's one copy, shared by every canvas tab.
static SLOT: Mutex<Slot> = Mutex::new(Slot {
    generation: 0,
    copied: None,
});

/// The slot. A panic elsewhere while it was held leaves it whole: each change to it is one
/// statement.
fn slot() -> MutexGuard<'static, Slot> {
    SLOT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What every marker starts with; the copy's number follows.
const MARKER: &str = "sawit-canvas:";

/// The marker the system clipboard holds for the copy numbered `generation`.
fn marker_of(generation: u64) -> String {
    format!("{MARKER}{generation}")
}

/// Whether `text` is a marker: this run's, or one that Sawit left on the system clipboard before
/// a restart, or another instance of it.
fn is_marker(text: &str) -> bool {
    text.strip_prefix(MARKER)
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// After `edit.copy` or `edit.cut` ran on `board`: keeps what its session copied as the app's
/// copy, for every canvas tab to paste ([`before_paste`]), and returns the marker for the system
/// clipboard, `sawit-canvas:<generation>`, a new one for each copy.
pub fn after_copy(board: &BoardDoc) -> String {
    let mut slot = slot();
    slot.generation += 1;
    slot.copied = Some(board.session.clipboard.clone());
    marker_of(slot.generation)
}

/// The marker of the app's newest copy ([`after_copy`]), or `None` before the first.
pub fn marker() -> Option<String> {
    let slot = slot();
    slot.copied.as_ref().map(|_| marker_of(slot.generation))
}

/// Before a paste on `board`: its session takes the layers the app copied last, in whichever tab
/// ([`after_copy`]), so `edit.paste` pastes them. What it copied of its own is kept otherwise: the
/// properties, fill, stroke or effect copied with their own commands.
///
/// When the newest copy is still this session's own, the session keeps it as it is, because
/// FigCraft keeps it up to date there: an undo that removes a copied component also takes it off
/// the components a paste makes instances of. Without a copy in the app it does nothing.
pub fn before_paste(board: &mut BoardDoc) {
    let slot = slot();
    let Some(copied) = &slot.copied else {
        return;
    };
    let own = board
        .session
        .ds()
        .is_ok_and(|ds| ds.uid == copied.source_doc);
    let clipboard = &mut board.session.clipboard;
    if own
        && clipboard.source_doc == copied.source_doc
        && clipboard.layers_generation == copied.layers_generation
    {
        return;
    }
    clipboard.nodes = copied.nodes.clone();
    clipboard.images = copied.images.clone();
    clipboard.bounds = copied.bounds;
    clipboard.source_doc = copied.source_doc;
    clipboard.source = copied.source.clone();
    clipboard.source_path = copied.source_path.clone();
    clipboard.copied_components = copied.copied_components.clone();
    // New layers to paste, as FigCraft counts them when it takes in layers from another app.
    clipboard.layers_generation = clipboard.layers_generation.wrapping_add(1);
}

/// What a paste pastes, from what the system clipboard holds (`system_text`, and `system_png`,
/// an image in a format FigCraft reads) and the marker of the canvas's newest copy:
///
/// 1. An image: [`PasteSource::Image`].
/// 2. Text that is the marker, so nothing was copied since the canvas's copy:
///    [`PasteSource::Canvas`]. So is a marker an earlier run of Sawit or another instance left:
///    it stands for layers too, never for text, and this run pastes its own copy, if it has one.
/// 3. SVG, also after an XML prolog, a comment or a doctype: [`PasteSource::Svg`].
/// 4. Any other text: [`PasteSource::Text`].
/// 5. Nothing: [`PasteSource::Canvas`].
pub fn paste_source(
    system_text: Option<&str>,
    system_png: Option<&[u8]>,
    marker: Option<&str>,
) -> PasteSource {
    if let Some(png) = system_png.filter(|png| !png.is_empty()) {
        return PasteSource::Image(png.to_vec());
    }
    match system_text {
        Some(text) if Some(text) == marker || is_marker(text) => PasteSource::Canvas,
        Some(text) if looks_like_svg(text) => PasteSource::Svg(text.to_string()),
        Some(text) if !text.is_empty() => PasteSource::Text(text.to_string()),
        _ => PasteSource::Canvas,
    }
}

/// Pastes `src` on `board`, and returns the layers it made, which are then selected.
///
/// - [`PasteSource::Canvas`]: `edit.paste`, which pastes the session's copied layers where they
///   were copied, or into the frame selected (call [`before_paste`] first, for the app's newest
///   copy). With nothing copied it is the engine's refusal: a [`BoardError::Engine`].
/// - [`PasteSource::Text`]: a text layer, `node.create`.
/// - [`PasteSource::Image`] and [`PasteSource::Svg`]: `file.import`, as `pasted.png` or
///   `pasted.svg`.
///
/// `at` is the page point to paste at (Paste here): the pasted layers' top left corner goes there.
/// Without it, as in FigCraft's own paste, text starts in the middle of the view, and an image or
/// SVG is centred there, so that a screenshot shows where it was pasted.
pub fn paste(
    board: &mut BoardDoc,
    src: PasteSource,
    at: Option<(f64, f64)>,
) -> Result<Vec<NodeId>, BoardError> {
    let mut point = at.map_or_else(|| json!({}), |(x, y)| json!({"x": x, "y": y}));
    match src {
        PasteSource::Canvas => {
            let pasted = board.execute("edit.paste", &point)?;
            let ids = pasted["ids"].as_array().into_iter().flatten();
            Ok(ids.filter_map(id_of).collect())
        }
        PasteSource::Text(text) => {
            let (x, y) = match at {
                Some(at) => at,
                None => {
                    let view = &board.session.ds()?.view;
                    (view.cx, view.cy)
                }
            };
            let text = json!({"type": "text", "text": text, "x": x, "y": y});
            let made = board.execute("node.create", &text)?;
            Ok(id_of(&made["id"]).into_iter().collect())
        }
        PasteSource::Image(png) => {
            if at.is_none() {
                point = centred(board, &png)?;
            }
            import(board, &png, "pasted.png", point)
        }
        PasteSource::Svg(svg) => {
            // The import centres its frame on the middle of the view.
            if at.is_none() {
                point["center"] = json!(true);
            }
            import(board, svg.as_bytes(), "pasted.svg", point)
        }
    }
}

/// Where the image `bytes` goes to be centred in the view, as FigCraft pastes one: its top left
/// corner, on whole units, from the size `file.import` places it at (its pixels over its density,
/// at most 4096 units a side). An image whose size can't be read gets no point: `file.import` puts
/// its corner in the middle of the view, or refuses it.
fn centred(board: &BoardDoc, bytes: &[u8]) -> Result<Value, BoardError> {
    let view = &board.session.ds()?.view;
    let size = guard::run("ui.pasteImage", || {
        Ok(
            figcraft_render::images::dimensions(bytes).map(|(width, height)| {
                let density = figcraft_format::pixel_density(bytes);
                figcraft_format::placed_size(width, height, density, PLACED_IMAGE_MAX_SIDE)
            }),
        )
    })?;
    Ok(match size {
        Some((width, height)) => json!({
            "x": (view.cx - width / 2.0).round(),
            "y": (view.cy - height / 2.0).round(),
        }),
        None => json!({}),
    })
}

/// Imports the file `bytes`, called `name`, its top left corner at `point` when that has `x` and
/// `y` (`file.import`), and returns the layer it made.
fn import(
    board: &mut BoardDoc,
    bytes: &[u8],
    name: &str,
    mut point: Value,
) -> Result<Vec<NodeId>, BoardError> {
    point["data"] = json!(STANDARD.encode(bytes));
    point["name"] = json!(name);
    let made = board.execute("file.import", &point)?;
    Ok(id_of(&made["id"]).into_iter().collect())
}

/// The characters selected in the text being edited, for the system clipboard: `None` when there
/// is only a caret, or no text is edited. FigCraft's panics are caught ([`guard::run`]).
pub fn copy_text(board: &BoardDoc) -> Result<Option<String>, BoardError> {
    let session = &board.session;
    let text = guard::run("ui.copyText", || Ok(textedit::selected_text(session)))?;
    Ok(text.filter(|text| !text.is_empty()))
}

/// Cuts the characters selected in the text being edited: [`copy_text`]'s, which then leave the
/// text, as Delete takes them out. `None`, and no change, when none are selected.
pub fn cut_text(board: &mut BoardDoc) -> Result<Option<String>, BoardError> {
    let text = copy_text(board)?;
    if text.is_some() {
        let session = &mut board.session;
        guard::run("ui.cutText", || {
            textedit::key(session, "Delete", false, false, false)
        })?;
    }
    Ok(text)
}

/// Types `text` over the selection of the text being edited, as a paste does: line endings become
/// `\n`, and other control characters are dropped. An error when no text is edited.
pub fn paste_text(board: &mut BoardDoc, text: &str) -> Result<(), BoardError> {
    let session = &mut board.session;
    guard::run("ui.pasteText", || textedit::insert(session, text))?;
    Ok(())
}

/// Forgets the app's copy, for the tests that start from nothing copied.
#[cfg(test)]
fn forget_the_copy() {
    slot().copied = None;
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use figcraft_doc::{BoardItem, Document, NodeId, NodeKind};
    use figcraft_engine::{Session, textedit};
    use serde_json::json;

    use super::*;

    /// One red rectangle, as another app copies SVG.
    const RECT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><rect width="100" height="80" fill="#f00"/></svg>"##;

    /// A 1 × 1 PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    /// A 160 × 90 PNG, at 72 dpi: it is placed at its size in pixels.
    const PNG_160_BY_90: &str = "iVBORw0KGgoAAAANSUhEUgAAAKAAAABaCAIAAACwpMoFAAAAoklEQVR42u3RAQ0AAAjDsEtDGtKQhg9oMgVrqkeHiwWABViABViABViAAQuwAAuwAAuwAAMWYAEWYAEWYAEWYMACLMACLMACLMCABViABViABViAAQuwAAuwAAuwAAswYAEWYAEWYAEWYMACLMACLMACLMCAXQAswAIswAIswAIMWIAFWIAFWIAFGLAAC7AAC7AAC7AAAxZgARZgARZgAf7VAoGLZO1iB8DyAAAAAElFTkSuQmCC";

    /// The same PNG at 144 dpi, as a screenshot on a Retina screen is: it is placed at half its
    /// size in pixels, 80 × 45.
    const PNG_160_BY_90_RETINA: &str = "iVBORw0KGgoAAAANSUhEUgAAAKAAAABaCAIAAACwpMoFAAAACXBIWXMAABYlAAAWJQFJUiTwAAAAoklEQVR42u3RAQ0AAAjDsEtDGtKQhg9oMgVrqkeHiwWABViABViABViAAQuwAAuwAAuwAAMWYAEWYAEWYAEWYMACLMACLMACLMCABViABViABViAAQuwAAuwAAuwAAswYAEWYAEWYAEWYMACLMACLMACLMCAXQAswAIswAIswAIMWIAFWIAFWIAFGLAAC7AAC7AAC7AAAxZgARZgARZgAf7VAoGLZO1iB8DyAAAAAElFTkSuQmCC";

    /// The tests that copy, or paste what was copied, share the app's one copy: they take turns,
    /// each starting with nothing copied.
    static TURN: Mutex<()> = Mutex::new(());

    fn turn() -> MutexGuard<'static, ()> {
        let turn = TURN.lock().unwrap_or_else(PoisonError::into_inner);
        forget_the_copy();
        turn
    }

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// A new, empty design file.
    fn design() -> BoardDoc {
        BoardDoc {
            session: Session::with_document(Document::new("Design")),
        }
    }

    /// The id in a command's answer.
    fn made(answer: &serde_json::Value) -> NodeId {
        figcraft_engine::params::id_of(&answer["id"]).unwrap()
    }

    /// A new sticky reading `text`, and only it selected.
    fn sticky(board: &mut BoardDoc, text: &str) -> NodeId {
        let note = made(
            &board
                .execute("board.sticky", &json!({"x": 0, "y": 0, "text": text}))
                .unwrap(),
        );
        select(board, note);
        note
    }

    fn select(board: &mut BoardDoc, id: NodeId) {
        board.execute("select.set", &json!({"ids": [id]})).unwrap();
    }

    /// What sticky `id` reads.
    fn sticky_text(board: &BoardDoc, id: NodeId) -> String {
        let doc = board.doc().unwrap();
        let text = figcraft_engine::board::text_of(&doc, id).unwrap();
        let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
        text.characters.clone()
    }

    /// The top-level layers of the page on show.
    fn layers(board: &BoardDoc) -> Vec<NodeId> {
        let page = board.session.page().unwrap();
        board.doc().unwrap().node(page).unwrap().children.clone()
    }

    fn is_sticky(board: &BoardDoc, id: NodeId) -> bool {
        let doc = board.doc().unwrap();
        doc.node(id)
            .is_some_and(|n| matches!(n.board, Some(BoardItem::Sticky { .. })))
    }

    /// Where layer `id`'s top-left corner is on the page.
    fn top_left(board: &BoardDoc, id: NodeId) -> (f64, f64) {
        let bounds = board.doc().unwrap().world_bounds(id).unwrap();
        (bounds.x0, bounds.y0)
    }

    /// Copies the selection as ⌘C does: the engine's copy, then the app's.
    fn copy(board: &mut BoardDoc) -> String {
        board.execute("edit.copy", &json!({})).unwrap();
        after_copy(board)
    }

    #[test]
    fn copy_in_one_tab_pastes_in_another() {
        let _turn = turn();
        let (mut one, mut other) = (board(), board());
        sticky(&mut one, "hi");
        copy(&mut one);
        before_paste(&mut other);
        let pasted = paste(&mut other, PasteSource::Canvas, None).unwrap();
        let [note] = layers(&other)[..] else {
            panic!("layers {:?}", layers(&other))
        };
        assert!(is_sticky(&other, note));
        assert_eq!(sticky_text(&other, note), "hi");
        assert_eq!(pasted, [note]);
        // What was pasted is selected, as after FigCraft's own paste.
        assert_eq!(other.session.selection(), [note]);
    }

    #[test]
    fn paste_prefers_newer_system_text() {
        assert_eq!(
            paste_source(Some("hello"), None, Some("sawit-canvas:3")),
            PasteSource::Text("hello".into())
        );
    }

    #[test]
    fn marker_means_canvas() {
        assert_eq!(
            paste_source(Some("sawit-canvas:3"), None, Some("sawit-canvas:3")),
            PasteSource::Canvas
        );
    }

    #[test]
    fn a_marker_from_another_run_is_no_text() {
        // Left on the system clipboard by Sawit before a restart, or by another instance: it
        // stands for layers, never for text to paste.
        for marker in [Some("sawit-canvas:3"), None] {
            assert_eq!(
                paste_source(Some("sawit-canvas:7"), None, marker),
                PasteSource::Canvas,
                "{marker:?}"
            );
        }
        // Text that only looks a little like one is text.
        for text in ["sawit-canvas:", "sawit-canvas:7 and more", "sawit-canvas:x"] {
            assert_eq!(
                paste_source(Some(text), None, None),
                PasteSource::Text(text.into())
            );
        }
    }

    #[test]
    fn an_image_comes_first_and_nothing_newer_means_the_canvas() {
        let png = [1u8, 2, 3];
        // An image another app copied is newer than the canvas's copy, whatever the text says.
        for text in [Some("sawit-canvas:3"), Some("hello"), None] {
            assert_eq!(
                paste_source(text, Some(&png), Some("sawit-canvas:3")),
                PasteSource::Image(png.to_vec()),
                "{text:?}"
            );
        }
        // No text and no image, or empty text: the canvas's own copy, if any.
        assert_eq!(paste_source(None, None, None), PasteSource::Canvas);
        assert_eq!(
            paste_source(Some(""), None, Some("sawit-canvas:3")),
            PasteSource::Canvas
        );
        // Text from another app before anything was copied on the canvas.
        assert_eq!(
            paste_source(Some("hello"), None, None),
            PasteSource::Text("hello".into())
        );
    }

    #[test]
    fn svg_text_imports() {
        // As SVG is copied from an editor, and with the XML prolog most tools write.
        let prolog = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{RECT_SVG}");
        for svg in [RECT_SVG, prolog.as_str()] {
            let mut board = board();
            let source = paste_source(Some(svg), None, Some("sawit-canvas:3"));
            assert_eq!(source, PasteSource::Svg(svg.into()));
            let pasted = paste(&mut board, source, None).unwrap();
            assert_eq!(layers(&board).len(), 1, "{svg}");
            assert_eq!(pasted, layers(&board));
        }
    }

    #[test]
    fn empty_canvas_clipboard_is_a_quiet_error() {
        let _turn = turn();
        let mut board = board();
        before_paste(&mut board);
        let pasted = paste(&mut board, PasteSource::Canvas, None);
        assert!(matches!(pasted, Err(BoardError::Engine(_))), "{pasted:?}");
        assert!(layers(&board).is_empty());
    }

    #[test]
    fn each_copy_has_a_marker_of_its_own() {
        let _turn = turn();
        assert_eq!(marker(), None);
        let mut board = board();
        sticky(&mut board, "hi");
        let first = copy(&mut board);
        assert!(first.starts_with("sawit-canvas:"), "{first}");
        assert_eq!(marker().as_deref(), Some(first.as_str()));
        let second = copy(&mut board);
        assert_ne!(second, first);
        assert_eq!(marker(), Some(second));
    }

    #[test]
    fn a_canvas_paste_goes_to_the_point_given() {
        let _turn = turn();
        let mut board = board();
        sticky(&mut board, "hi");
        copy(&mut board);
        before_paste(&mut board);
        let pasted = paste(&mut board, PasteSource::Canvas, Some((300.0, 200.0))).unwrap();
        let [note] = pasted[..] else {
            panic!("pasted {pasted:?}")
        };
        assert_eq!(top_left(&board, note), (300.0, 200.0));
        assert_eq!(layers(&board).len(), 2);
    }

    #[test]
    fn text_pastes_as_a_text_layer_at_the_point_or_in_the_middle_of_the_view() {
        let mut board = board();
        let pasted = paste(
            &mut board,
            PasteSource::Text("hello".into()),
            Some((40.0, 30.0)),
        )
        .unwrap();
        let [text] = pasted[..] else {
            panic!("pasted {pasted:?}")
        };
        let doc = board.doc().unwrap();
        let characters = doc.node(text).and_then(|n| n.kind.text());
        assert_eq!(characters.map(|t| t.characters.as_str()), Some("hello"));
        assert_eq!(top_left(&board, text), (40.0, 30.0));
        assert_eq!(board.session.selection(), [text]);

        let view = board.session.ds().unwrap().view.clone();
        let pasted = paste(&mut board, PasteSource::Text("there".into()), None).unwrap();
        assert_eq!(top_left(&board, pasted[0]), (view.cx, view.cy));
        assert_eq!(layers(&board).len(), 2);
    }

    #[test]
    fn an_image_pastes_as_an_image_layer() {
        let mut board = board();
        let png = STANDARD.decode(PNG).unwrap();
        let pasted = paste(&mut board, PasteSource::Image(png), Some((10.0, 20.0))).unwrap();
        assert_eq!(pasted, layers(&board));
        let doc = board.doc().unwrap();
        let image = doc.node(pasted[0]).unwrap();
        assert!(
            matches!(image.kind, NodeKind::Rectangle),
            "{:?}",
            image.kind
        );
        assert_eq!(doc.images.len(), 1);
        assert_eq!(top_left(&board, pasted[0]), (10.0, 20.0));
    }

    /// Layer `id`'s size on the page.
    fn size(board: &BoardDoc, id: NodeId) -> (f64, f64) {
        let bounds = board.doc().unwrap().world_bounds(id).unwrap();
        (bounds.width(), bounds.height())
    }

    /// A new board whose view shows page point (300.5, 200) in its middle.
    fn board_seen_about_its_point() -> BoardDoc {
        let mut board = board();
        let view = json!({"zoom": 1, "vw": 1000, "vh": 800, "cx": 300.5, "cy": 200});
        board.execute("view.set", &view).unwrap();
        board
    }

    /// As FigCraft's own paste places them (ui-egui menus/clipboard.rs `paste`, `paste_text`): a
    /// screenshot pasted with its corner in the middle of the view would lie mostly off it.
    #[test]
    fn a_pasted_image_is_centred_in_the_view() {
        for (png, (width, height)) in [
            (PNG_160_BY_90, (160.0, 90.0)),
            (PNG_160_BY_90_RETINA, (80.0, 45.0)),
        ] {
            let mut board = board_seen_about_its_point();
            let png = STANDARD.decode(png).unwrap();
            let pasted = paste(&mut board, PasteSource::Image(png), None).unwrap();
            let [image] = pasted[..] else {
                panic!("pasted {pasted:?}")
            };
            // At the size it is placed at, its middle on the view's, its corner on whole units.
            assert_eq!(size(&board, image), (width, height));
            let corner = (
                (300.5 - width / 2.0).round(),
                (200.0 - height / 2.0).round(),
            );
            assert_eq!(top_left(&board, image), corner, "{width} × {height}");
        }
    }

    #[test]
    fn a_pasted_svg_is_centred_in_the_view() {
        let mut board = board_seen_about_its_point();
        let pasted = paste(&mut board, PasteSource::Svg(RECT_SVG.into()), None).unwrap();
        let [frame] = pasted[..] else {
            panic!("pasted {pasted:?}")
        };
        assert_eq!(size(&board, frame), (100.0, 80.0));
        assert_eq!(top_left(&board, frame), (251.0, 160.0));
    }

    #[test]
    fn a_pasted_svg_goes_to_the_point_given() {
        // Paste here: its top-left corner at the point, as for an image and for layers.
        let mut board = board_seen_about_its_point();
        let source = PasteSource::Svg(RECT_SVG.into());
        let pasted = paste(&mut board, source, Some((10.0, 20.0))).unwrap();
        assert_eq!(top_left(&board, pasted[0]), (10.0, 20.0));
    }

    #[test]
    fn a_paste_keeps_the_properties_a_tab_copied() {
        let _turn = turn();
        let (mut one, mut other) = (board(), board());
        // The other tab copied a sticky's properties (Copy properties, ⌥⌘C).
        sticky(&mut other, "props");
        other.execute("edit.copyProperties", &json!({})).unwrap();
        let props = other.session.clipboard.props.clone();
        assert!(props.is_some());
        sticky(&mut one, "layers");
        copy(&mut one);
        before_paste(&mut other);
        // It takes the copied layers, and keeps its copied properties.
        assert_eq!(other.session.clipboard.props, props);
        let pasted = paste(&mut other, PasteSource::Canvas, None).unwrap();
        assert_eq!(sticky_text(&other, pasted[0]), "layers");
    }

    #[test]
    fn a_tab_pastes_what_it_copied_after_an_undo() {
        let _turn = turn();
        let mut board = design();
        let component = json!({"type": "component", "x": 0, "y": 0, "width": 50, "height": 50});
        let first = made(&board.execute("node.create", &component).unwrap());
        copy(&mut board);
        // Undone, the component is gone, and the next one takes its id: FigCraft then pastes the
        // copy as a component of its own, not as an instance of the new one.
        board.execute("edit.undo", &json!({})).unwrap();
        let second = made(&board.execute("node.create", &component).unwrap());
        assert_eq!(second, first);
        before_paste(&mut board);
        let pasted = paste(&mut board, PasteSource::Canvas, None).unwrap();
        let doc = board.doc().unwrap();
        let kind = &doc.node(pasted[0]).unwrap().kind;
        assert!(matches!(kind, NodeKind::Component { .. }), "{kind:?}");
    }

    #[test]
    fn text_copy_cut_and_paste_while_editing() {
        let mut board = board();
        let note = sticky(&mut board, "hello");
        // Editing starts with all of "hello" selected.
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        assert_eq!(copy_text(&board), Ok(Some("hello".into())));
        assert_eq!(sticky_text(&board, note), "hello");
        paste_text(&mut board, "x").unwrap();
        assert_eq!(sticky_text(&board, note), "x");
        // Only a caret: nothing to copy or cut, and nothing changes.
        assert_eq!(copy_text(&board), Ok(None));
        assert_eq!(cut_text(&mut board), Ok(None));
        assert_eq!(sticky_text(&board, note), "x");
        // Cut takes the selected characters out of the text.
        textedit::key(&mut board.session, "SelectAll", false, false, false).unwrap();
        assert_eq!(cut_text(&mut board), Ok(Some("x".into())));
        assert_eq!(sticky_text(&board, note), "");
        assert!(board.session.ds().unwrap().text_edit.is_some());
    }
}
