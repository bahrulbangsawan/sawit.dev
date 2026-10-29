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

    /// The file in `bytes` (a `.figcraft` archive). The notes on what the loader repaired or left
    /// out, and on what saving would drop, are only logged, so a caller that may save back to the
    /// file should use [`BoardDoc::from_bytes_with_issues`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BoardError> {
        let (board, issues) = Self::from_bytes_with_issues(bytes)?;
        for issue in issues {
            log::warn!("canvas file loaded with an issue: {issue}");
        }
        Ok(board)
    }

    /// Like [`BoardDoc::from_bytes`], with notes on what saving the returned document over the same
    /// file would lose, one sentence each. An empty list means a save writes back everything the
    /// loader read, bar a stored thumbnail (a preview, not content). The list holds:
    /// - the loader's own notes: what it repaired, and what it could not read and left out (an
    ///   image, version, branch or published library that is damaged or past its limits);
    /// - a note for each kind of content that was read fine but that [`BoardDoc::to_bytes`] never
    ///   writes: a version history, branches, and the images past the loader's limits (those stay
    ///   in the file, unread).
    pub fn from_bytes_with_issues(bytes: &[u8]) -> Result<(Self, Vec<String>), BoardError> {
        let loaded = figcraft_format::load_bytes_with_versions(bytes).map_err(format_error)?;
        let mut issues = loaded.issues;
        if !loaded.versions.is_empty() {
            issues.push(
                "the file has a version history, which the canvas doesn't keep: \
                 saving this file again drops it"
                    .into(),
            );
        }
        if !loaded.branches.is_empty() {
            issues.push(
                "the file has branches, which the canvas doesn't keep: \
                 saving this file again drops them"
                    .into(),
            );
        }
        if loaded.images_past_budget > 0 {
            issues.push(format!(
                "images past the limit of what can be loaded stay in the file ({} of them): \
                 saving this file again drops them",
                loaded.images_past_budget
            ));
        }
        let board = Self {
            session: Session::with_document(loaded.doc),
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
    fn a_version_history_is_noted_because_saving_drops_it() {
        let board = BoardDoc::new_board("Canvas").unwrap();
        let doc = board.doc().unwrap();
        let history = [figcraft_format::Version::new(
            "v1",
            "First",
            "",
            1,
            doc.clone(),
        )];
        let bytes = figcraft_format::save_bytes_with_versions(&doc, None, &history).unwrap();

        let (board, issues) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("version history"), "{issues:?}");
        // The note is true: the saved file has no history, and says nothing once it has none.
        let saved = board.to_bytes().unwrap();
        let loaded = figcraft_format::load_bytes_with_versions(&saved).unwrap();
        assert!(loaded.versions.is_empty());
        assert!(
            BoardDoc::from_bytes_with_issues(&saved)
                .unwrap()
                .1
                .is_empty()
        );
    }

    #[test]
    fn branches_are_noted_because_saving_drops_them() {
        let board = BoardDoc::new_board("Canvas").unwrap();
        let doc = board.doc().unwrap();
        let branch = figcraft_format::Branch {
            id: "b1".into(),
            name: "Explore".into(),
            time: 1,
            client: 1,
            state: figcraft_format::BranchState::Active,
            base: doc.clone(),
            head: doc.clone(),
            versions: Vec::new(),
        };
        let bytes = figcraft_format::save_bytes_with_history(&doc, None, &[], &[branch]).unwrap();

        let (board, issues) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("branches"), "{issues:?}");
        let loaded = figcraft_format::load_bytes_with_versions(&board.to_bytes().unwrap()).unwrap();
        assert!(loaded.branches.is_empty());
    }

    #[test]
    fn images_past_the_loaders_limit_are_noted_because_saving_drops_them() {
        // FigCraft's IMAGE_BUDGET reads at most 20 000 images from a file; the rest stay in the
        // file, unread. Building 20 001 archive members takes most of a second.
        let document = Document::new("File");
        let document = serde_json::to_vec(&figcraft_format::to_json(&document).unwrap()).unwrap();
        let names: Vec<String> = (0..=20_000).map(|n| format!("images/i{n}")).collect();
        let members = std::iter::once(("document.json", document.as_slice()))
            .chain(names.iter().map(|name| (name.as_str(), &b"x"[..])));
        let bytes = figcraft_format::zip_files(members).unwrap();

        let (_, issues) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
        let note = |i: &&String| i.contains("past the limit");
        assert_eq!(issues.iter().filter(note).count(), 1, "{issues:?}");
        assert!(
            issues.iter().any(|i| i.contains("(1 of them)")),
            "{issues:?}"
        );
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
