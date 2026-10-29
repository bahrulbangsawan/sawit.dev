//! The canvas document: a FigCraft [`Session`] holding one file, behind the few calls the rest of
//! the app needs. Make, load and save it; read it; run engine commands on it.

use std::sync::Arc;

use figcraft_doc::Document;
use figcraft_engine::{EngineError, Session, board};
use figcraft_format::FormatError;
use serde_json::{Value, json};
use thiserror::Error;

/// Why a canvas call failed. Every message is user-facing, so a toast or an `{ok:false,error}`
/// reply can show it as it is.
#[derive(Clone, Debug, PartialEq, Error)]
pub enum BoardError {
    /// The engine refused a command, or the session holds no document.
    #[error("{0}")]
    Engine(String),
    /// `.figcraft` bytes could not be read or written: corrupt, truncated or from a newer FigCraft.
    #[error("{0}")]
    Format(String),
    /// A file could not be read or written.
    #[error("{0}")]
    Io(String),
}

impl From<EngineError> for BoardError {
    fn from(e: EngineError) -> Self {
        BoardError::Engine(e.to_string())
    }
}

fn format_error(e: FormatError) -> BoardError {
    BoardError::Format(e.to_string())
}

/// The kind of file a canvas holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    /// Sticky notes, shapes with text, connectors, marker and stamps on a light page.
    Board,
    /// A design file: frames, auto layout, components.
    Design,
}

/// The canvas document: an engine session with one open file.
pub struct BoardDoc {
    pub session: Session,
}

impl BoardDoc {
    /// A new, empty board called `name`.
    pub fn new_board(name: &str) -> Result<Self, BoardError> {
        let mut session = Session::empty();
        session.execute("file.newBoard", &json!({ "name": name }))?;
        Ok(Self { session })
    }

    /// The file in `bytes` (a `.figcraft` archive). What the loader repaired or left out is only
    /// logged, so a caller that may save back to the file should use
    /// [`BoardDoc::from_bytes_with_issues`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BoardError> {
        let (board, issues) = Self::from_bytes_with_issues(bytes)?;
        for issue in issues {
            log::warn!("canvas file loaded with an issue: {issue}");
        }
        Ok(board)
    }

    /// Like [`BoardDoc::from_bytes`], with the loader's notes on what it repaired or left out, one
    /// sentence each (none for a healthy file). Some mean data loss: an image, version history,
    /// published library or branch it could not read is missing from the returned document, so
    /// saving that document over the same file drops it.
    pub fn from_bytes_with_issues(bytes: &[u8]) -> Result<(Self, Vec<String>), BoardError> {
        let (doc, issues) = figcraft_format::load_bytes(bytes).map_err(format_error)?;
        let board = Self {
            session: Session::with_document(doc),
        };
        Ok((board, issues))
    }

    /// The document as a `.figcraft` archive, without a thumbnail.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BoardError> {
        let doc = self.doc()?;
        figcraft_format::save_bytes(&doc, None).map_err(format_error)
    }

    /// The current document. It is a snapshot: later edits make a new one.
    pub fn doc(&self) -> Result<Arc<Document>, BoardError> {
        Ok(self.session.ds()?.doc.clone())
    }

    /// Whether this is a board or a design file. A session without a document is not a board.
    pub fn kind(&self) -> DocKind {
        match self.session.doc() {
            Ok(doc) if board::is_board(doc) => DocKind::Board,
            _ => DocKind::Design,
        }
    }

    /// The engine's change counter for the open file (0 without one). It goes up on every
    /// successful command and every selection change, so a higher value means something changed,
    /// not always the content that gets saved.
    pub fn revision(&self) -> u64 {
        self.session.ds().map_or(0, |ds| ds.revision)
    }

    /// Run engine command `id`. A refused command leaves the document as it was.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, BoardError> {
        Ok(self.session.execute(id, params)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_board_is_a_board_with_an_empty_page() {
        let b = BoardDoc::new_board("Canvas").unwrap();
        assert_eq!(b.kind(), DocKind::Board);
        let doc = b.doc().unwrap();
        let page = doc.first_page().unwrap();
        assert!(doc.node(page).unwrap().children.is_empty());
    }

    #[test]
    fn bytes_round_trip_keeps_a_sticky() {
        let mut b = BoardDoc::new_board("Canvas").unwrap();
        b.execute("board.sticky", &json!({"x": 0, "y": 0, "text": "hi"}))
            .unwrap();
        let before = b.doc().unwrap().nodes().count();
        let back = BoardDoc::from_bytes(&b.to_bytes().unwrap()).unwrap();
        let doc = back.doc().unwrap();
        assert_eq!(doc.nodes().count(), before);
        assert!(
            doc.nodes()
                .any(|n| matches!(n.board, Some(figcraft_doc::BoardItem::Sticky { .. })))
        );
    }

    #[test]
    fn unknown_command_is_an_engine_error() {
        let mut b = BoardDoc::new_board("Canvas").unwrap();
        assert!(matches!(
            b.execute("no.such", &json!({})),
            Err(BoardError::Engine(_))
        ));
    }

    #[test]
    fn revision_moves_on_edit() {
        let mut b = BoardDoc::new_board("Canvas").unwrap();
        let r0 = b.revision();
        b.execute("board.sticky", &json!({"x": 0, "y": 0})).unwrap();
        assert!(b.revision() > r0);
    }

    #[test]
    fn corrupt_or_truncated_bytes_are_a_format_error() {
        let is_format_error =
            |bytes: &[u8]| matches!(BoardDoc::from_bytes(bytes), Err(BoardError::Format(_)));
        assert!(is_format_error(b"not a figcraft file"));
        assert!(is_format_error(&[]));
        let mut b = BoardDoc::new_board("Canvas").unwrap();
        b.execute("board.sticky", &json!({"x": 0, "y": 0, "text": "hi"}))
            .unwrap();
        let bytes = b.to_bytes().unwrap();
        assert!(is_format_error(&bytes[..bytes.len() / 2]));
    }

    #[test]
    fn a_damaged_image_loads_with_issues_that_name_what_was_lost() {
        // A 1x1 PNG.
        const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let mut b = BoardDoc::new_board("Canvas").unwrap();
        let placed = b
            .execute("image.place", &json!({"data": PNG, "x": 0, "y": 0}))
            .unwrap();
        let hash = placed["image"].as_str().unwrap().to_string();
        let png = b.doc().unwrap().images[&hash].bytes.clone();
        let bytes = b.to_bytes().unwrap();

        // The undamaged file loads without a word.
        let (back, issues) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert!(back.doc().unwrap().images.contains_key(&hash));

        // Flip a byte of the stored image: the file still opens, without the image, and says so.
        let mut damaged = bytes.clone();
        let at = damaged
            .windows(png.len())
            .position(|w| w == png.as_slice())
            .unwrap();
        damaged[at + png.len() / 2] ^= 0xFF;
        let (back, issues) = BoardDoc::from_bytes_with_issues(&damaged).unwrap();
        assert!(issues.iter().any(|i| i.contains(&hash)), "{issues:?}");
        assert!(!back.doc().unwrap().images.contains_key(&hash));
        // `from_bytes` opens it too, and only logs the issues.
        assert!(BoardDoc::from_bytes(&damaged).is_ok());
    }

    #[test]
    fn kind_survives_a_save_and_reload() {
        let board = BoardDoc::new_board("Canvas").unwrap();
        let board = BoardDoc::from_bytes(&board.to_bytes().unwrap()).unwrap();
        assert_eq!(board.kind(), DocKind::Board);
        let design = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
        let design = BoardDoc::from_bytes(&design).unwrap();
        assert_eq!(design.kind(), DocKind::Design);
    }

    #[test]
    fn without_a_document_there_is_nothing_to_read() {
        let b = BoardDoc {
            session: Session::empty(),
        };
        assert_eq!(b.revision(), 0);
        assert_eq!(b.kind(), DocKind::Design);
        assert!(matches!(b.doc(), Err(BoardError::Engine(_))));
        assert!(matches!(b.to_bytes(), Err(BoardError::Engine(_))));
    }
}
