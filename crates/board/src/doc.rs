//! The canvas document: a FigCraft [`Session`] holding one file, behind the few calls the rest of
//! the app needs. Make, load and save it; read it; run engine commands on it.

use std::path::Path;
use std::sync::Arc;

use figcraft_doc::Document;
use figcraft_engine::{DocState, EngineError, Session, board, guard};
use figcraft_format::FormatError;
use serde_json::{Value, json};
use thiserror::Error;

/// Why a canvas call failed. The message is the one an agent's `{ok:false,error}` reply carries,
/// as the engine words it, with its advice to agents (`pass ids`). What a toast shows the user is
/// [`BoardError::for_user`], and a key that has nothing to act on shows nothing
/// ([`BoardError::for_key`]).
#[derive(Clone, Debug, PartialEq, Error)]
pub enum BoardError {
    /// The engine refused a command, or the session holds no document.
    #[error("{0}")]
    Engine(String),
    /// The command doesn't apply right now: nothing is selected, there is nothing to undo, no
    /// properties were copied, a board's tool on a design file. The engine's `Disabled` refusal,
    /// in its words.
    #[error("command `{command}` is not available right now: {reason}")]
    Unavailable { command: String, reason: String },
    /// Canvas file bytes could not be read or written: corrupt, truncated or from a newer Sawit.
    #[error("{0}")]
    Format(String),
    /// A file could not be read or written.
    #[error("{0}")]
    Io(String),
}

/// Why a board's command does nothing on a design file, in Sawit's words. FigCraft's
/// ([`board::NEEDS_BOARD`]) sends the user to File ▸ New board, which the canvas hides: a canvas
/// keeps the file it opened, and nothing in Sawit makes a design file a board (SMOKE-3).
pub const BOARDS_ONLY: &str = "this is for boards, and this canvas is a design file";

/// What the engine's file format says of a file it can't read, in its own name, and the same in
/// Sawit's: to the user and to an agent the canvas file is Sawit's, whatever engine reads it.
const FILE_WORDS: [(&str, &str); 2] = [
    ("not a valid FigCraft file", "not a valid canvas file"),
    (
        "saved by a newer FigCraft",
        "saved by a newer version of Sawit",
    ),
];

/// `message`, from the engine or its file format, with Sawit's words for what it says in
/// FigCraft's ([`BOARDS_ONLY`], [`FILE_WORDS`]), wherever in it they are.
fn in_sawits_words(message: &str) -> String {
    let message = message.replace(board::NEEDS_BOARD, BOARDS_ONLY);
    (FILE_WORDS.iter()).fold(message, |message, (engine, sawit)| {
        message.replace(engine, sawit)
    })
}

impl From<EngineError> for BoardError {
    /// The engine's message, with Sawit's words for what it says in FigCraft's
    /// ([`in_sawits_words`]): the same for a toast and for an agent's reply, wherever in it the
    /// engine's words are. A command that doesn't apply right now is [`BoardError::Unavailable`].
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::Disabled(command, reason) => BoardError::Unavailable {
                command,
                reason: in_sawits_words(&reason),
            },
            e => BoardError::Engine(in_sawits_words(&e.to_string())),
        }
    }
}

impl BoardError {
    /// Whether the command doesn't apply right now ([`BoardError::Unavailable`]): it changed
    /// nothing, and there was nothing for it to act on.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, BoardError::Unavailable { .. })
    }

    /// What a toast tells the user when a command they ran from a menu, a button or the palette
    /// fails: the reason in plain words, without the engine's advice to agents. "command
    /// `object.frameSelection` is not available right now: nothing is selected (pass `ids`)"
    /// reads "Nothing is selected"; any other message loses its advice the same way.
    pub fn for_user(&self) -> String {
        match self {
            // The engine's short reasons, which start in lower case: "nothing is selected".
            BoardError::Unavailable { reason, .. } => sentence(&plain_words(reason)),
            BoardError::Engine(message) => match given_what_it_cant_use(message) {
                Some(what) => sentence(&plain_words(what)),
                // A message may start with a file's name ("notes.txt: …"): it stays as it is.
                None => plain_words(message),
            },
            BoardError::Format(message) | BoardError::Io(message) => plain_words(message),
        }
    }

    /// What a toast tells the user when a key's command fails, if anything. A key with nothing to
    /// act on does nothing and says nothing, as in FigCraft's own keyboard and in Figma (⌥⌘G with
    /// nothing selected, ⌥⌘V with no properties copied, ⌃⌥⌘K with no instance selected). A
    /// board's tool on a design file says why ([`BOARDS_ONLY`], SMOKE-3): there it is the canvas,
    /// not the selection, that leaves the key nothing to do. Any other failure says why, as
    /// [`BoardError::for_user`] words it.
    pub fn for_key(&self) -> Option<String> {
        match self {
            BoardError::Unavailable { reason, .. } if reason != BOARDS_ONLY => None,
            // A key's command runs without parameters: what it lacks is the selection it needs
            // ("select an instance") or a value only a dialog would ask for.
            BoardError::Engine(message) if given_what_it_cant_use(message).is_some() => None,
            error => Some(error.for_user()),
        }
    }
}

/// What the engine says a command was given that it can't use, when `message` is its refusal of
/// a command's parameters (`EngineError::BadParams`): "select an instance" of "invalid parameters
/// for `component.goToMain`: select an instance".
fn given_what_it_cant_use(message: &str) -> Option<&str> {
    // The engine's own words around the command, so that they can't drift from what it writes.
    let words = EngineError::BadParams {
        cmd: "\0".into(),
        msg: String::new(),
    }
    .to_string();
    let (before, after) = words.split_once('\0')?;
    let (_, what) = message.strip_prefix(before)?.split_once(after)?;
    Some(what)
}

/// `message` as a user reads it: without the advice the engine gives agents, in parentheses or
/// after a colon or a semicolon ("(pass `ids`)", ": pass `path`", "; use page.duplicate"), and
/// without the backticks around a parameter's name.
fn plain_words(message: &str) -> String {
    let mut text = message.to_string();
    // Advice in parentheses: "(pass `ids`)", "(use component.restoreMain)".
    while let Some((open, close)) = advice_in_parentheses(&text) {
        text.replace_range(open..=close, "");
    }
    // Advice after a colon or a semicolon: "the file has no path yet: pass `path`".
    for separator in [": ", "; "] {
        if let Some(at) = text.rfind(separator)
            && is_advice(&text[at + separator.len()..])
        {
            text.truncate(at);
        }
    }
    let text = text.replace('`', "");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` with a capital first letter, as a toast's sentence starts.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Where the first parenthesised advice to agents in `text` opens and closes, as byte offsets.
fn advice_in_parentheses(text: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(open) = text[from..].find('(').map(|at| from + at) {
        let close = open + text[open..].find(')')?;
        if is_advice(&text[open + 1..close]) {
            return Some((open, close));
        }
        from = close + 1;
    }
    None
}

/// Whether `text` is advice to an agent: it names a parameter or a command in backticks, or it
/// tells what to pass, use or add.
fn is_advice(text: &str) -> bool {
    let text = text.trim();
    text.contains('`')
        || ["pass ", "use ", "add one with "]
            .iter()
            .any(|verb| text.starts_with(verb))
}

fn format_error(e: FormatError) -> BoardError {
    BoardError::Format(in_sawits_words(&e.to_string()))
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

    /// The canvas file in `bytes` (the engine's archive), with its version history and branches.
    /// The notes on what the loader repaired or left out are only logged, so a caller that may save
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

    /// The canvas file at `path`, whose bytes are `bytes`, read as
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
    ///
    /// The engine's own `file.open` makes the document's state (repairing what it must, linking
    /// styles, binding paints) inside its guard against a panic; it is made here in one too
    /// ([`guard::run`], spec D15, CANVAS-CORE-9): a file another app wrote that makes it panic is
    /// an error the canvas shows, where the panic unwound into the window's callbacks and aborted
    /// the app with every tab.
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
        let ds = guard::run("file.open", || {
            let mut ds = DocState::new(loaded.doc, None)
                .with_versions(loaded.versions)
                .with_branches(loaded.branches);
            if let Some(file) = file {
                ds = ds.with_images_left_in_file(&file.to_string_lossy(), left_in_file);
            }
            Ok(ds)
        })?;
        let mut session = Session::empty();
        session.docs.push(ds);
        Ok((Self { session }, issues))
    }

    /// The document as a canvas file's archive, with its version history and branches, and without a
    /// thumbnail (a preview, not content). It is the document itself
    /// ([`BoardDoc::document_itself`]), never a version shown in its place. It records the page on
    /// show, as FigCraft's own save does (`FileSnapshot::encode`), so the file opens on that page
    /// again, here and in FigCraft (CANVAS-CORE-12).
    pub fn to_bytes(&self) -> Result<Vec<u8>, BoardError> {
        let ds = self.session.ds()?;
        let itself = ds.document_itself();
        let on_show = Some(ds.page).filter(|page| itself.pages.contains(page));
        let encode = |doc: &Document| {
            figcraft_format::save_bytes_with_history(doc, None, &ds.versions, &ds.branches)
                .map_err(format_error)
        };
        match on_show {
            Some(page) if itself.current_page != Some(page) => {
                let mut doc = (**itself).clone();
                doc.current_page = Some(page);
                encode(&doc)
            }
            _ => encode(itself),
        }
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
                 the canvas reads from one file), and saving over it would delete them. Save it \
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

/// Runs `f` on `session` as one undo step called `label` ([`Session::grouped`]), with a panic in
/// `f` caught as the engine catches one in a command ([`guard::run`], spec D15, CANVAS-CORE-9):
/// the group fails, so what `f` did is rolled back and the document is as it was. Uncaught, the
/// panic unwound through the group without rolling it back, and into the window's callbacks,
/// which aborts the app with every tab.
pub(crate) fn grouped<T>(
    session: &mut Session,
    label: &str,
    f: impl FnOnce(&mut Session) -> Result<T, EngineError>,
) -> Result<T, EngineError> {
    session.grouped(label, |session| guard::run(label, || f(session)))
}

/// A canvas file of a board with a sticky "kept", and an image whose bytes are damaged: the
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

/// A canvas file of a blank design with one image more than FigCraft reads from a file (its
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

    /// The toast in the user's screenshot: ⌥⌘G with nothing selected. An agent still reads the
    /// engine's words, with what to pass; the user reads plain words, and a key reads nothing.
    #[test]
    fn a_command_that_does_not_apply_speaks_plainly_to_the_user() {
        let mut b = BoardDoc::new_board("Canvas").unwrap();
        let refused = b.execute("object.frameSelection", &json!({})).unwrap_err();
        assert!(refused.is_unavailable(), "{refused:?}");
        assert_eq!(
            refused.to_string(),
            "command `object.frameSelection` is not available right now: nothing is selected \
             (pass `ids`)"
        );
        assert_eq!(refused.for_user(), "Nothing is selected");
        assert_eq!(refused.for_key(), None);
        // No properties copied to paste: the same.
        let paste = b.execute("edit.pasteProperties", &json!({})).unwrap_err();
        assert!(paste.is_unavailable(), "{paste:?}");
        assert_eq!(paste.for_user(), "No copied properties");
        assert_eq!(paste.for_key(), None);
    }

    #[test]
    fn a_boards_tool_on_a_design_file_says_why_even_from_a_key() {
        let mut design = BoardDoc {
            session: Session::with_document(Document::new("Design")),
        };
        let refused = design.execute("tool.sticky", &json!({})).unwrap_err();
        assert!(refused.is_unavailable(), "{refused:?}");
        let said = "This is for boards, and this canvas is a design file";
        assert_eq!(refused.for_user(), said);
        assert_eq!(refused.for_key().as_deref(), Some(said));
    }

    #[test]
    fn plain_words_drop_the_advice_to_agents() {
        for (engine, user) in [
            ("nothing is selected (pass `ids`)", "Nothing is selected"),
            (
                "the main component was deleted (use component.restoreMain)",
                "The main component was deleted",
            ),
            (
                "no export settings (add one with export.add)",
                "No export settings",
            ),
            (
                "the document has unsaved changes (pass force: true to discard)",
                "The document has unsaved changes",
            ),
            (
                "the file has no path yet: pass `path`",
                "The file has no path yet",
            ),
            (
                "the saved path ends in .fig: choose Save as or pass `path`",
                "The saved path ends in .fig",
            ),
            (
                "pages can't be copied; use page.duplicate",
                "Pages can't be copied",
            ),
            // What is no advice stays.
            (
                "Frame 1 isn't on a page (it is a library component)",
                "Frame 1 isn't on a page (it is a library component)",
            ),
            ("no copied properties", "No copied properties"),
        ] {
            let error = BoardError::Unavailable {
                command: "some.command".into(),
                reason: engine.into(),
            };
            assert_eq!(error.for_user(), user, "{engine}");
            assert!(!error.for_user().contains('`'), "{engine}");
        }
        // A command given what it can't use says what it lacks, and nothing from a key.
        let mut b = BoardDoc::new_board("Canvas").unwrap();
        let refused = b.execute("component.goToMain", &json!({})).unwrap_err();
        assert_eq!(refused.for_user(), "Select an instance");
        assert_eq!(refused.for_key(), None);
        let refused = b.execute("text.link", &json!({})).unwrap_err();
        assert!(!refused.for_user().contains('`'), "{refused:?}");
        assert_eq!(refused.for_key(), None);
        // Other errors lose their advice the same way, and say why from a key too.
        // A message of its own keeps its first letter: it may start with a file's name.
        let other = BoardError::Engine("notes.txt: no image in it (pass `data`)".into());
        assert_eq!(other.for_user(), "notes.txt: no image in it");
        assert_eq!(
            other.for_key().as_deref(),
            Some("notes.txt: no image in it")
        );
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
        assert!(is_format_error(b"not a canvas file"));
        assert!(is_format_error(&[]));
        let mut b = BoardDoc::new_board("Canvas").unwrap();
        b.execute("board.sticky", &json!({"x": 0, "y": 0, "text": "hi"}))
            .unwrap();
        let bytes = b.to_bytes().unwrap();
        assert!(is_format_error(&bytes[..bytes.len() / 2]));
    }

    /// A file the canvas can't read is said to be no canvas file, or one from a newer Sawit: the
    /// user and the agent read Sawit's name, not the engine's.
    #[test]
    fn a_file_that_cant_be_read_is_told_in_sawits_words() {
        let error = BoardDoc::from_bytes(b"{").err().unwrap().to_string();
        assert!(error.starts_with("not a valid canvas file: "), "{error}");
        let newer = br#"{"version":99,"pages":[]}"#;
        let error = BoardDoc::from_bytes(newer).err().unwrap().to_string();
        assert_eq!(
            error,
            "this file was saved by a newer version of Sawit (format version 99)"
        );
        assert!(!error.contains("FigCraft"));
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
        let path = Path::new("/work/design.sawit");
        let (board, issues) = BoardDoc::from_file_bytes(&bytes, path).unwrap();
        let note = |i: &&String| i.contains("past the limit");
        assert_eq!(issues.iter().filter(note).count(), 1, "{issues:?}");
        assert!(
            issues.iter().any(|i| i.contains("(1 of them)")),
            "{issues:?}"
        );
        // Saved over, the file would lose the image it holds unread. That is refused, in
        // FigCraft's own words, the canvas in its place; saved anywhere else, nothing is lost.
        let refused = board.refuse_dropping_images(path).unwrap_err();
        assert_eq!(
            refused.to_string(),
            "design wasn't saved: 1 of its images weren't loaded (together they pass what \
             the canvas reads from one file), and saving over it would delete them. Save it under \
             another name; this file keeps them."
        );
        assert!(
            board
                .refuse_dropping_images(Path::new("/work/copy.sawit"))
                .is_ok()
        );
        // Read with no file, it has no file to save over.
        let (board, _) = BoardDoc::from_bytes_with_issues(&bytes).unwrap();
        assert!(board.refuse_dropping_images(path).is_ok());
    }

    /// CANVAS-CORE-12: a save records the page on show, as FigCraft's own does, so the file
    /// opens on it again, here and in FigCraft. It opened on the page FigCraft last saved.
    #[test]
    fn a_save_keeps_the_page_on_show() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let first = board.session.ds().unwrap().page;
        board.execute("page.add", &json!({})).unwrap();
        let second = board.session.ds().unwrap().page;
        assert_ne!(first, second, "page.add shows the new page");
        let page_on_open = |board: &BoardDoc| {
            let back = BoardDoc::from_bytes(&board.to_bytes().unwrap()).unwrap();
            back.session.ds().unwrap().page
        };
        assert_eq!(page_on_open(&board), second);
        board.execute("page.select", &json!({"id": first})).unwrap();
        assert_eq!(page_on_open(&board), first);
    }

    /// CANVAS-CORE-9: a panic in what an op does inside its group (a bug in Sawit's code between
    /// the engine's commands) is caught there: the group fails and rolls back, and the next edit
    /// is the one undo step. Caught outside the group, the sticky made before the panic stayed, a
    /// half-done op, and was an undo step too.
    #[test]
    fn a_panic_in_a_group_rolls_it_back_and_leaves_undo_whole() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let before = board.doc().unwrap();
        let result: Result<(), EngineError> =
            grouped(&mut board.session, "Add elements", |session| {
                session.execute("board.sticky", &json!({"x": 0, "y": 0}))?;
                panic!("a bug in the op");
            });
        let error = result.unwrap_err().to_string();
        assert!(error.contains("a bug in the op"), "{error}");
        assert!(
            *board.doc().unwrap() == *before,
            "the sticky made before stayed"
        );
        board
            .execute("board.sticky", &json!({"x": 300, "y": 0}))
            .unwrap();
        assert_eq!(board.session.undo_depth(), (1, 0));
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
