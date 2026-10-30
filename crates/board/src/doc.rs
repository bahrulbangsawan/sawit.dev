//! The canvas document: a FigCraft [`Session`] holding one file, behind the few calls the rest of
//! the app needs. Make, load and save it; read it; run engine commands on it.

use std::path::Path;
use std::sync::Arc;

use figcraft_doc::Document;
use figcraft_engine::{DocState, EngineError, Session, board};
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

    /// The file in `bytes` (a `.figcraft` archive), with its version history and branches. The
    /// notes on what the loader repaired or left out are only logged, so a caller that may save
    /// back to a file should use [`BoardDoc::from_file_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BoardError> {
        let (board, issues) = Self::from_bytes_with_issues(bytes)?;
        for issue in issues {
            log::warn!("canvas file loaded with an issue: {issue}");
        }
        Ok(board)
    }

    /// Like [`BoardDoc::from_bytes`], with notes on what saving the returned document over the same
    /// file would lose, one sentence each. An empty list means a save writes back everything the
    /// loader read ([`BoardDoc::to_bytes`]: the document, its version history and its branches),
    /// bar a stored thumbnail (a preview, not content). The list holds:
    /// - the loader's own notes: what it repaired, and what it could not read and left out (an
    ///   image, version, branch or published library that is damaged or past its limits);
    /// - a note on the images past the loader's limits, which stay in the file, unread: a save over
    ///   the file would drop them, so [`BoardDoc::from_file_bytes`] keeps a save from doing that.
    pub fn from_bytes_with_issues(bytes: &[u8]) -> Result<(Self, Vec<String>), BoardError> {
        Self::load(bytes, None)
    }

    /// The `.figcraft` file at `path`, whose bytes are `bytes`, read as
    /// [`BoardDoc::from_bytes_with_issues`] reads it, and with what FigCraft's own `file.open` notes
    /// of the file: the images it holds past the loader's limits, which a save over it would delete
    /// ([`BoardDoc::refuse_dropping_images`]). The session keeps no path, so nothing in it can
    /// write or reload the file behind the canvas's store.
    pub fn from_file_bytes(bytes: &[u8], path: &Path) -> Result<(Self, Vec<String>), BoardError> {
        Self::load(bytes, Some(path))
    }

    /// The board in `bytes`, with its version history and branches, and when it is read from the
    /// file at `file`, the images left in that file. The notes are
    /// [`BoardDoc::from_bytes_with_issues`]'s.
    fn load(bytes: &[u8], file: Option<&Path>) -> Result<(Self, Vec<String>), BoardError> {
        let loaded = figcraft_format::load_bytes_with_versions(bytes).map_err(format_error)?;
        let mut issues = loaded.issues;
        let left_in_file = loaded.images_past_budget;
        if left_in_file > 0 {
            issues.push(format!(
                "images past the limit of what can be loaded stay in the file ({left_in_file} of \
                 them): saving over the file would drop them"
            ));
        }
        let mut ds = DocState::new(loaded.doc, None)
            .with_versions(loaded.versions)
            .with_branches(loaded.branches);
        if let Some(file) = file {
            ds = ds.with_images_left_in_file(&file.to_string_lossy(), left_in_file);
        }
        let mut session = Session::empty();
        session.docs.push(ds);
        Ok((Self { session }, issues))
    }

    /// The document as a `.figcraft` archive, with its version history and branches, and without a
    /// thumbnail (a preview, not content). It is the document itself
    /// ([`BoardDoc::document_itself`]), never a version shown in its place.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BoardError> {
        let ds = self.session.ds()?;
        figcraft_format::save_bytes_with_history(
            ds.document_itself(),
            None,
            &ds.versions,
            &ds.branches,
        )
        .map_err(format_error)
    }

    /// Refuses a save over the file at `path` that would delete images the loader left in it,
    /// past its limits ([`BoardDoc::from_file_bytes`]), as FigCraft's own `file.save` refuses it,
    /// and in its words. A save anywhere else loses nothing of that file, and neither does one of a
    /// document that wasn't read from it.
    pub fn refuse_dropping_images(&self, path: &Path) -> Result<(), BoardError> {
        match self
            .session
            .ds()?
            .images_lost_saving_to(&path.to_string_lossy())
        {
            Some(count) => Err(BoardError::Engine(format!(
                "{} wasn't saved: {count} of its images weren't loaded (together they pass what \
                 FigCraft reads from one file), and saving over it would delete them. Save it \
                 under another name; this file keeps them.",
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("Untitled")
            ))),
            None => Ok(()),
        }
    }

    /// The current document. It is a snapshot: later edits make a new one. While the engine shows a
    /// version, or a live text preview, in place of the document, this is what it shows.
    pub fn doc(&self) -> Result<Arc<Document>, BoardError> {
        Ok(self.session.ds()?.doc.clone())
    }

    /// The document itself, which a save writes: [`BoardDoc::doc`], except while the engine shows
    /// a version of it, or a live text preview, in its place.
    pub fn document_itself(&self) -> Result<Arc<Document>, BoardError> {
        Ok(self.session.ds()?.document_itself().clone())
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

/// A `.figcraft` file of a board with a sticky "kept", and an image whose bytes are damaged: the
/// loader leaves the image out, and says that a save drops it.
#[cfg(test)]
pub(crate) fn file_with_a_damaged_image() -> Vec<u8> {
    // A 1 × 1 PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "kept"}))
        .unwrap();
    let placed = board
        .execute("image.place", &json!({"data": PNG, "x": 0, "y": 300}))
        .unwrap();
    let hash = placed["image"].as_str().unwrap().to_string();
    let png = board.doc().unwrap().images[&hash].bytes.clone();
    let mut bytes = board.to_bytes().unwrap();
    let at = bytes
        .windows(png.len())
        .position(|w| w == png.as_slice())
        .unwrap();
    bytes[at + png.len() / 2] ^= 0xFF;
    bytes
}

/// A `.figcraft` file of a blank design with one image more than FigCraft reads from a file (its
/// `IMAGE_BUDGET` is 20 000): that image stays in the file, unread. Building its 20 001 archive
/// members takes most of a second.
#[cfg(test)]
pub(crate) fn file_with_an_image_past_the_limit() -> Vec<u8> {
    let document = Document::new("File");
    let document = serde_json::to_vec(&figcraft_format::to_json(&document).unwrap()).unwrap();
    let names: Vec<String> = (0..=20_000).map(|n| format!("images/i{n}")).collect();
    let members = std::iter::once(("document.json", document.as_slice()))
        .chain(names.iter().map(|name| (name.as_str(), &b"x"[..])));
    figcraft_format::zip_files(members).unwrap()
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

    /// Sticky notes on the document, whatever it shows.
    fn stickies(doc: &Document) -> usize {
        doc.nodes()
            .filter(|n| matches!(n.board, Some(figcraft_doc::BoardItem::Sticky { .. })))
            .count()
    }

    #[test]
    fn a_version_history_is_kept_by_a_save() {
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

        // It is read with the file, so there is nothing to say about it.
        let (mut board, issues) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        // A save writes it back, after an edit too.
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "after"}))
            .unwrap();
        let saved = board.to_bytes().unwrap();
        let loaded = figcraft_format::load_bytes_with_versions(&saved).unwrap();
        let names: Vec<&str> = loaded.versions.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["First"]);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        assert_eq!(stickies(&loaded.doc), 1);
    }

    #[test]
    fn branches_are_kept_by_a_save() {
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

        let (mut board, issues) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "after"}))
            .unwrap();
        let loaded = figcraft_format::load_bytes_with_versions(&board.to_bytes().unwrap()).unwrap();
        let names: Vec<&str> = loaded.branches.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Explore"]);
        assert_eq!(stickies(&loaded.doc), 1);
    }

    #[test]
    fn a_save_writes_the_document_not_a_version_shown_in_its_place() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let sticky = |board: &mut BoardDoc, text: &str| {
            let at = json!({"x": stickies(&board.doc().unwrap()) * 300, "y": 0, "text": text});
            board.execute("board.sticky", &at).unwrap();
        };
        sticky(&mut board, "then");
        board
            .execute("file.saveVersion", &json!({"name": "Then"}))
            .unwrap();
        sticky(&mut board, "now");
        let now = board.doc().unwrap();

        // The canvas shows the version in place of the document...
        board
            .execute("version.preview", &json!({"index": 0}))
            .unwrap();
        assert_eq!(stickies(&board.doc().unwrap()), 1);
        // ...and the document is still what is saved, with its history.
        assert!(Arc::ptr_eq(&board.document_itself().unwrap(), &now));
        let loaded = figcraft_format::load_bytes_with_versions(&board.to_bytes().unwrap()).unwrap();
        assert_eq!(stickies(&loaded.doc), 2);
        assert_eq!(loaded.versions.len(), 1);
    }

    #[test]
    fn images_past_the_loaders_limit_make_saving_over_their_file_refused() {
        let bytes = file_with_an_image_past_the_limit();
        let path = Path::new("/work/design.figcraft");
        let (board, issues) = BoardDoc::from_file_bytes(&bytes, path).unwrap();
        let note = |i: &&String| i.contains("past the limit");
        assert_eq!(issues.iter().filter(note).count(), 1, "{issues:?}");
        assert!(
            issues.iter().any(|i| i.contains("(1 of them)")),
            "{issues:?}"
        );
        // Saved over, the file would lose the image it holds unread. That is refused, in
        // FigCraft's own words; saved anywhere else, nothing is lost.
        let refused = board.refuse_dropping_images(path).unwrap_err();
        assert_eq!(
            refused.to_string(),
            "design wasn't saved: 1 of its images weren't loaded (together they pass what \
             FigCraft reads from one file), and saving over it would delete them. Save it under \
             another name; this file keeps them."
        );
        assert!(
            board
                .refuse_dropping_images(Path::new("/work/copy.figcraft"))
                .is_ok()
        );
        // Read with no file, it has no file to save over.
        let (board, _) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
        assert!(board.refuse_dropping_images(path).is_ok());
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
