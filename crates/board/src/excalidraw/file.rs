//! A scene read from an `.excalidraw` file, or a blank one from a file with nothing in it.

use std::path::Path;

use serde_json::{Value, json};

use super::BYTE_ORDER_MARK;
use crate::doc::BoardError;

/// The scene in the `.excalidraw` file at `path`. A file that can't be read, or isn't JSON, is an
/// error that names it. A file with nothing in it isn't JSON, so it is one too: a load that
/// somebody asked for (the `replace` op) has no use for it. Opening a canvas takes it as a blank
/// one ([`read_file_or_blank`]).
pub fn read_file(path: &Path) -> Result<Value, BoardError> {
    parse(path, &read_text(path)?)
}

/// The scene in the `.excalidraw` file at `path`, as [`read_file`] reads it, except that a file
/// with nothing in it is a scene with no elements: a blank canvas. Nothing in it is empty, or only
/// white space, or only a byte order mark (which some editors write). Today's canvas opens such a
/// file blank, and there is nothing in it to protect.
///
/// It is for opening a canvas, as the migration of the old project canvas does
/// ([`store::open_with_issues`](crate::store::open_with_issues)). Everything else is as
/// [`read_file`]'s: a file with anything in it that isn't a scene is an error that names it.
pub fn read_file_or_blank(path: &Path) -> Result<Value, BoardError> {
    let text = read_text(path)?;
    if text
        .chars()
        .all(|c| c.is_whitespace() || c == BYTE_ORDER_MARK)
    {
        return Ok(json!({"type": "excalidraw", "elements": []}));
    }
    parse(path, &text)
}

/// The most an Excalidraw file may hold to be read: 1 GiB of JSON, images in it included.
pub const MAX_SCENE: u64 = 1024 * 1024 * 1024;

/// The text of the file at `path`. Only a regular file of at most [`MAX_SCENE`] bytes is read
/// ([`sawit_core::files::read_regular`]): a named pipe or a device is refused at once
/// (CANVAS-CORE-6).
fn read_text(path: &Path) -> Result<String, BoardError> {
    let couldnt_read = |e: &dyn std::fmt::Display| {
        BoardError::Io(format!("couldn't read {}: {e}", path.display()))
    };
    let bytes = sawit_core::files::read_regular(path, MAX_SCENE).map_err(|e| couldnt_read(&e))?;
    String::from_utf8(bytes).map_err(|_| couldnt_read(&"stream did not contain valid UTF-8"))
}

/// The JSON in `text`, the text of the file at `path`, without the byte order mark in front.
fn parse(path: &Path, text: &str) -> Result<Value, BoardError> {
    serde_json::from_str(text.trim_start_matches(BYTE_ORDER_MARK))
        .map_err(|e| BoardError::Format(format!("{} isn't valid JSON: {e}", path.display())))
}
