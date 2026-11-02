//! Why a canvas call failed, in the words an agent reads and in the plain words a toast shows the
//! user.

use figcraft_engine::{EngineError, board};
use figcraft_format::FormatError;
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

pub(super) fn format_error(e: FormatError) -> BoardError {
    BoardError::Format(in_sawits_words(&e.to_string()))
}
