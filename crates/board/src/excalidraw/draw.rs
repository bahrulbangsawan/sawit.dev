//! A scene the engine refuses part of, drawn one element at a time with a note for each refused.

use crate::doc::{BoardDoc, BoardError};
use crate::skeleton::{self, Skeleton};

/// Draws `items` one at a time, in the order [`skeleton::apply`] draws a list in, so that an
/// element the engine refuses is left out with a note and the others stay. `refused` is why the
/// list as a whole wasn't drawn, and is the error when not one element could be drawn.
///
/// An element finds what it joins, or the section it goes in, by the agent id of what was drawn
/// before it, so one at a time draws the same as all at once.
pub(super) fn draw_apart(
    board: &mut BoardDoc,
    items: &[Skeleton],
    refused: BoardError,
    notes: &mut Vec<String>,
) -> Result<(), BoardError> {
    let mut in_order: Vec<&Skeleton> = items.iter().collect();
    in_order.sort_by_key(|item| stage(&item.kind));
    let mut later = Vec::new();
    let mut drawn = 0;
    for item in in_order {
        let only = std::slice::from_ref(item);
        match skeleton::apply(board, only, (0.0, 0.0)) {
            Ok(applied) => {
                drawn += 1;
                later.extend(applied.notes.iter().map(|note| named(note, only)));
            }
            Err(error) => later.push(format!("skipped {}: {}", describe(item), reason(&error))),
        }
    }
    if drawn == 0 && !items.is_empty() {
        return Err(refused);
    }
    notes.extend(later);
    Ok(())
}

/// When [`skeleton::apply`] draws a skeleton of `kind`: sections, then shapes and pictures, then
/// text, then what joins them.
fn stage(kind: &str) -> usize {
    match kind {
        "frame" => 0,
        "rectangle" | "ellipse" | "diamond" | "image" => 1,
        "text" => 2,
        _ => 3,
    }
}

/// `note`, as [`skeleton::apply`] words it about "elements[3]", with each of those named by the
/// element it is about, which is how a person knows it: "arrow `Ab3x`". `items` are the skeletons
/// it was given.
pub(super) fn named(note: &str, items: &[Skeleton]) -> String {
    const OPEN: &str = "elements[";
    let mut out = String::with_capacity(note.len());
    let mut rest = note;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        rest = &rest[at + OPEN.len()..];
        let item = rest.split_once(']').and_then(|(index, tail)| {
            let item = items.get(index.parse::<usize>().ok()?)?;
            Some((item, tail))
        });
        match item {
            Some((item, tail)) => {
                out.push_str(&describe(item));
                rest = tail;
            }
            None => out.push_str(OPEN),
        }
    }
    out.push_str(rest);
    out
}

/// What a skeleton is, for a note: its type, and the id it came with.
fn describe(item: &Skeleton) -> String {
    match item.id.as_deref() {
        Some(id) => format!("{} `{id}`", item.kind),
        None => item.kind.clone(),
    }
}

/// Why `apply` refused an element it was given alone: the engine's words, without the
/// "elements[0] (arrow): " in front.
fn reason(error: &BoardError) -> String {
    let message = error.to_string();
    match message.split_once("): ") {
        Some((_, reason)) => reason.to_string(),
        None => message,
    }
}
