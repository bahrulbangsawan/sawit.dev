//! Typing on the canvas. While it edits a text, [`BoardView`] is the window's text input (its
//! paint registers it, [`handle_input`]), so what the keys type and what an input method composes
//! reach FigCraft's text editor (`figcraft_engine::textedit`): plain keys, accents held down, and
//! the compositions of Chinese, Japanese and Korean input methods. The keys the canvas leaves
//! alone while it edits ([`BoardView::on_key`]) come back here as text.
//!
//! The input method sees the text being edited as a window of it around the selection, at most
//! [`textedit::MAX_IME_SURROUNDING_BYTES`] long (`textedit::ime_surrounding`), with its
//! composition in it at the caret, as a text view holds its marked text. It counts in UTF-16 code
//! units, where the editor counts characters. A composition is shown, not typed: the document gets
//! it once the input method commits it, and an edit that ends while composing takes it along.
//!
//! Every call into the text editor goes through `guard::run` (spec D15); an error the engine
//! returns shows as a toast.

use std::ops::Range;

use figcraft_engine::textedit::{self, ImeEditOwner};
use figcraft_engine::{Session, TextEdit, guard};
use gpui_kit::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, Pixels, Point, UTF16Selection,
    WeakEntity, Window, point, px,
};
use sawit_board::overlay::{self, Composition};
use sawit_board::{BoardDoc, BoardError};

use super::BoardView;

/// While the canvas of `view` edits a text, makes it the window's text input for the frame being
/// painted, the canvas being at `bounds`: what the keys type, and what an input method composes,
/// then come to it. Called from the canvas's paint, as GPUI asks. The window takes it only while
/// the canvas has the keyboard.
pub(super) fn handle_input(
    view: &WeakEntity<BoardView>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(view) = view.upgrade() else { return };
    let focus = {
        let canvas = view.read(cx);
        if !canvas.editing_text() {
            return;
        }
        canvas.focus.clone()
    };
    window.handle_input(&focus, ElementInputHandler::new(bounds, view), cx);
}

impl EntityInputHandler for BoardView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let ime = ImeText::of(&self.board()?.session)?;
        let from = byte_floor(&ime.text, range.start);
        let to = byte_ceil(&ime.text, range.end).max(from);
        *adjusted = Some(utf16_at(&ime.text, from)..utf16_at(&ime.text, to));
        Some(ime.text[from..to].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let ime = ImeText::of(&self.board()?.session)?;
        let caret = utf16_at(&ime.text, ime.caret);
        let anchor = utf16_at(&ime.text, ime.anchor);
        Some(UTF16Selection {
            range: caret.min(anchor)..caret.max(anchor),
            reversed: caret < anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let session = &self.board()?.session;
        // Asked before each key reaches the canvas: no text is gathered while nothing is composed.
        text_edit(session)?.preedit.as_ref()?;
        let ime = ImeText::of(session)?;
        let marked = ime.marked?;
        Some(utf16_at(&ime.text, marked.start)..utf16_at(&ime.text, marked.end))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let owner = self.ime_owner.take();
        let Some(board) = self.board_mut() else {
            return;
        };
        let done = commit_composition(board, owner);
        self.after_text_input(done, cx);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime_owner = None;
        let Some(board) = self.board_mut() else {
            return;
        };
        let done = replace(board, range, text);
        self.after_text_input(done, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(board) = self.board_mut() else {
            return;
        };
        let done = compose(board, range, text, selected);
        self.ime_owner = composer(&board.session);
        self.after_text_input(done, cx);
        // The input method's candidates follow the composition.
        window.invalidate_character_coordinates();
    }

    /// Where the caret is before the range's first character, in the text as it shows: an input
    /// method shows its candidates there.
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let board = self.board.as_ref().ok()?;
        Composition::refresh(&mut self.composition, &board.session);
        let ime = ImeText::of(&board.session)?;
        let index = ime.shown(byte_floor(&ime.text, range.start));
        let composition = self.composition.as_ref();
        let caret = guard::run("ui.imeBounds", || {
            Ok(overlay::caret_at(&board.session, composition, index))
        });
        let ((x0, y0), (x1, y1)) = caret.ok().flatten()?;
        let at = |x: f64, y: f64| element_bounds.origin + point(px(x as f32), px(y as f32));
        Some(Bounds::from_corners(
            at(x0.min(x1), y0.min(y1)),
            at(x0.max(x1), y0.max(y1)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl BoardView {
    /// After the text input changed the text being edited, as after every input: the canvas
    /// carries out what the engine asked of it, the engine's error shows as a toast, and the
    /// canvas redraws (and saves, once it has been still).
    fn after_text_input(&mut self, done: Result<(), BoardError>, cx: &mut Context<Self>) {
        self.handle_ui_requests(Vec::new(), cx);
        // After the requests' toast, so that the error's is the one that shows.
        if let Err(error) = done {
            self.show_toast(error.to_string(), cx);
        }
        cx.notify();
    }
}

/// The text edit under way in `s`.
fn text_edit(s: &Session) -> Option<&TextEdit> {
    s.ds().ok()?.text_edit.as_ref()
}

/// The edit under way in `s`, as an input method's composition is owned by one.
fn edit_owner(s: &Session) -> Option<ImeEditOwner> {
    let ds = s.ds().ok()?;
    let edit = ds.text_edit.as_ref()?;
    Some(ImeEditOwner {
        document: ds.uid,
        node: edit.id,
        editing_session: edit.editing_session,
    })
}

/// The edit a composition is under way in, if one is.
fn composer(s: &Session) -> Option<ImeEditOwner> {
    text_edit(s)?.preedit.as_ref()?;
    edit_owner(s)
}

/// Puts `text` in place of what the input method replaces: `range` of the text as it sees it,
/// when it names one; else the composition, or the selection. What a key types, and what an input
/// method commits ([`textedit::insert_ime_commit`] while it composes). Nothing without a text
/// edit.
fn replace(
    board: &mut BoardDoc,
    range: Option<Range<usize>>,
    text: &str,
) -> Result<(), BoardError> {
    let Some(edit) = text_edit(&board.session) else {
        return Ok(());
    };
    let composing = edit.preedit.is_some();
    if let Some(range) = range {
        make_room(board, range)?;
    }
    let session = &mut board.session;
    if composing {
        guard::run("ui.imeCommit", || {
            textedit::insert_ime_commit(session, text)
        })?;
    } else {
        guard::run("ui.type", || textedit::insert(session, text))?;
    }
    Ok(())
}

/// Shows `text` as the input method's composition at the caret, in place of `range` of the text as
/// it sees it when it names one, with its cursor where `selected` starts (UTF-16 in `text`), or at
/// its end. An empty `text` ends the composition, leaving no text. Nothing without a text edit.
fn compose(
    board: &mut BoardDoc,
    range: Option<Range<usize>>,
    text: &str,
    selected: Option<Range<usize>>,
) -> Result<(), BoardError> {
    if text_edit(&board.session).is_none() {
        return Ok(());
    }
    if let Some(range) = range {
        make_room(board, range)?;
    }
    let cursor = selected.map(|selected| text[..byte_floor(text, selected.start)].chars().count());
    let session = &mut board.session;
    guard::run("ui.preedit", || textedit::preedit(session, text, cursor))?;
    Ok(())
}

/// Readies `range` of the text as the input method sees it to be replaced by the next insertion,
/// which replaces the selection: what it covers before and after the selection is deleted
/// ([`textedit::delete_surrounding_chars`]); a range apart from the selection is selected instead.
/// In the composition, it stands for the composition, which the insertion replaces anyway.
fn make_room(board: &mut BoardDoc, range: Range<usize>) -> Result<(), BoardError> {
    let session = &board.session;
    let (Some(ime), Some(edit)) = (ImeText::of(session), text_edit(session)) else {
        return Ok(());
    };
    let from = ime.committed(byte_floor(&ime.text, range.start));
    let to = ime.committed(byte_ceil(&ime.text, range.end)).max(from);
    let (start, end) = (edit.caret.min(edit.anchor), edit.caret.max(edit.anchor));
    let session = &mut board.session;
    if from <= start && to >= end {
        let (before, after) = (start - from, to - end);
        if before > 0 || after > 0 {
            guard::run("ui.imeReplace", || {
                textedit::delete_surrounding_chars(session, before, after)
            })?;
        }
    } else {
        guard::run("ui.imeReplace", || {
            textedit::set_caret(session, from, false)?;
            textedit::set_caret(session, to, true)
        })?;
    }
    Ok(())
}

/// Commits the composition as it shows, while it is still under way in the edit `owner` it was
/// composed in (ruling P2-C9). An edit that ended took its composition along: nothing then, nor in
/// another edit.
fn commit_composition(board: &mut BoardDoc, owner: Option<ImeEditOwner>) -> Result<(), BoardError> {
    let session = &board.session;
    let Some(preedit) = text_edit(session).and_then(|edit| edit.preedit.clone()) else {
        return Ok(());
    };
    if owner.is_none() || edit_owner(session) != owner {
        return Ok(());
    }
    let session = &mut board.session;
    guard::run("ui.imeCommit", || {
        textedit::insert_ime_commit(session, &preedit.text)
    })?;
    Ok(())
}

/// The text being edited as the input method sees it. Offsets are UTF-8 bytes of `text`.
struct ImeText {
    /// A window of the text around the selection, with the composition in it at the caret.
    text: String,
    /// How many characters of the text come before the window.
    first: usize,
    /// The selection: where the caret is, the input method's cursor while it composes, and where
    /// the selection was anchored.
    caret: usize,
    anchor: usize,
    /// The composition, while one is under way.
    marked: Option<Range<usize>>,
}

impl ImeText {
    /// The text `s` edits. `None` without a text edit, or when the selection alone is too long for
    /// the window.
    fn of(s: &Session) -> Option<Self> {
        let seen = guard::run("ui.imeText", || Ok(textedit::ime_surrounding(s).ok()));
        let seen = seen.ok().flatten()?;
        let edit = text_edit(s)?;
        let first = edit
            .caret
            .saturating_sub(seen.text[..seen.cursor].chars().count());
        let mut text = seen.text;
        let Some(preedit) = &edit.preedit else {
            return Some(Self {
                text,
                first,
                caret: seen.cursor,
                anchor: seen.anchor,
                marked: None,
            });
        };
        // Composing, the selection is the caret, and the composition shows there.
        let at = seen.cursor;
        text.insert_str(at, &preedit.text);
        let cursor: usize = (preedit.text.chars().take(preedit.cursor))
            .map(char::len_utf8)
            .sum();
        Some(Self {
            text,
            first,
            caret: at + cursor,
            anchor: at + cursor,
            marked: Some(at..at + preedit.text.len()),
        })
    }

    /// The character of the text as it shows, with the composition in it, at byte `byte`.
    fn shown(&self, byte: usize) -> usize {
        self.first + self.chars(0..byte)
    }

    /// The character of the text the document has, without the composition, at byte `byte`: in
    /// the composition, the caret it goes in at.
    fn committed(&self, byte: usize) -> usize {
        let before = match &self.marked {
            Some(marked) if marked.contains(&byte) => self.chars(0..marked.start),
            Some(marked) if byte >= marked.end => self.chars(0..byte) - self.chars(marked.clone()),
            _ => self.chars(0..byte),
        };
        self.first + before
    }

    /// How many characters bytes `range` of the text hold.
    fn chars(&self, range: Range<usize>) -> usize {
        self.text.get(range).map_or(0, |text| text.chars().count())
    }
}

/// The UTF-16 offset of byte `byte` of `text`, the start of a character.
fn utf16_at(text: &str, byte: usize) -> usize {
    (text.char_indices())
        .take_while(|(at, _)| *at < byte)
        .map(|(_, c)| c.len_utf16())
        .sum()
}

/// The byte of `text` at UTF-16 offset `utf16`: the start of the character it falls in (the second
/// half of a surrogate pair is in its character), or the end of the text past it.
fn byte_floor(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        units += c.len_utf16();
        if units > utf16 {
            return at;
        }
    }
    text.len()
}

/// [`byte_floor`], rounding an offset inside a character up to the character's end.
fn byte_ceil(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        if units >= utf16 {
            return at;
        }
        units += c.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use figcraft_doc::NodeId;
    use figcraft_engine::textedit;
    use serde_json::json;

    use super::*;

    #[test]
    fn utf16_offsets_round_to_whole_characters() {
        // The emoji is 2 UTF-16 units and 4 bytes, the é 1 unit and 2 bytes.
        let text = "a😀bé";
        let starts = [(0, 0), (1, 1), (3, 5), (4, 6), (5, 8), (9, 8)];
        for (utf16, byte) in starts {
            assert_eq!(byte_floor(text, utf16), byte, "floor {utf16}");
            assert_eq!(byte_ceil(text, utf16), byte, "ceil {utf16}");
        }
        // Inside the emoji: its start rounding down, its end rounding up.
        assert_eq!((byte_floor(text, 2), byte_ceil(text, 2)), (1, 5));
        for (utf16, byte) in [(0, 0), (1, 1), (3, 5), (4, 6), (5, 8)] {
            assert_eq!(utf16_at(text, byte), utf16, "byte {byte}");
        }
    }

    /// A board with a sticky being typed in, reading `text` with the caret at its end.
    fn typing(text: &str) -> (BoardDoc, NodeId) {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let made = board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": text}))
            .unwrap();
        let note = figcraft_engine::params::id_of(&made["id"]).unwrap();
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        textedit::key(&mut board.session, "End", false, false, true).unwrap();
        (board, note)
    }

    #[test]
    fn a_composition_shows_at_the_caret_and_maps_back_to_it() {
        let (mut board, _) = typing("hi");
        textedit::key(&mut board.session, "Left", false, false, false).unwrap();
        textedit::preedit(&mut board.session, "ni", Some(1)).unwrap();
        let ime = ImeText::of(&board.session).unwrap();
        // "h", the composition with the input method's cursor after its n, then "i".
        assert_eq!(ime.text, "hnii");
        assert_eq!(
            (ime.marked.clone(), ime.caret, ime.anchor),
            (Some(1..3), 2, 2)
        );
        // As it shows, character for character; in the document, the composition is at the caret.
        assert_eq!(
            (0..=4).map(|b| ime.shown(b)).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
        let committed: Vec<usize> = (0..=4).map(|b| ime.committed(b)).collect();
        assert_eq!(committed, [0, 1, 1, 1, 2]);
    }

    #[test]
    fn a_long_text_is_seen_through_a_window_around_the_caret() {
        // 5000 characters, the caret at the end: the input method sees the last 4000 of them.
        let (mut board, note) = typing(&"x".repeat(5000));
        let ime = ImeText::of(&board.session).unwrap();
        assert_eq!((ime.text.len(), ime.first, ime.caret), (4000, 1000, 4000));
        assert_eq!(ime.committed(3999), 4999);
        // Replacing the window's last character replaces the text's.
        replace(&mut board, Some(3999..4000), "y").unwrap();
        let doc = board.doc().unwrap();
        let text = figcraft_engine::board::text_of(&doc, note).unwrap();
        let reads = &doc
            .node(text)
            .and_then(|n| n.kind.text())
            .unwrap()
            .characters;
        assert_eq!(reads.chars().count(), 5000);
        assert!(reads.ends_with("xy"), "{}", &reads[4990..]);
    }

    #[test]
    fn nothing_is_typed_or_composed_without_a_text_edit() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        assert_eq!(replace(&mut board, None, "hi"), Ok(()));
        assert_eq!(compose(&mut board, None, "ni", None), Ok(()));
        assert_eq!(commit_composition(&mut board, None), Ok(()));
        assert!(ImeText::of(&board.session).is_none());
        let page = board.session.page().unwrap();
        assert!(board.doc().unwrap().node(page).unwrap().children.is_empty());
    }
}
