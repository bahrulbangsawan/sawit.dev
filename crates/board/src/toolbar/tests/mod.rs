//! The toolbar's tests: the files they run on, and the kinds of file with their button counts.

use figcraft_doc::Document;
use figcraft_engine::Session;

use super::*;
use crate::DocKind;

mod buttons;
mod flyouts;
mod place;

/// A new, empty board.
fn board() -> BoardDoc {
    BoardDoc::new_board("Canvas").unwrap()
}

/// A design file with nothing on its page.
fn design() -> BoardDoc {
    BoardDoc {
        session: Session::with_document(Document::new("Design")),
    }
}

/// Both kinds of file, with how many tool buttons the toolbar of each has. The tests that go
/// over every button check the count too, so that an empty toolbar fails them.
const KINDS: [(DocKind, usize); 2] = [(DocKind::Board, 22), (DocKind::Design, 16)];

/// A file of `kind` with nothing on its page.
fn empty(kind: DocKind) -> BoardDoc {
    match kind {
        DocKind::Board => board(),
        DocKind::Design => design(),
    }
}
