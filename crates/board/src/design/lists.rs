//! What the paint and effect lists have in common. `fills`, `strokes` and `effects` are each one
//! property of a layer whose value is a list of rows, and the panel changes one row by sending the
//! whole list back through `node.set`. So the rows it doesn't touch have to go back as they were
//! read, and an edit that can't apply needs one answer, given here.

use serde_json::{Value, json};

use super::{DesignSelection, Edit, Mixed};

/// The largest whole number an `f64` holds exactly (2^53). Past it a float is no longer a count.
const MAX_WHOLE: f64 = 9_007_199_254_740_992.0;

/// The rows of list property `key` (`fills`, `strokes` or `effects`), when every layer holds the
/// same list. Each row is the JSON value as read, with its whole numbers made integers again
/// ([`whole_numbers`]), so the engine takes it back as it is.
///
/// `None` when the layers hold different lists: there are no rows to show, and no row to change.
/// A list with nothing in it is a list like any other (`Some` of no rows), and so is the list of an
/// empty selection.
pub(super) fn rows(sel: &DesignSelection, key: &str) -> Option<Vec<Value>> {
    match sel.get(key) {
        Mixed::Same(Value::Array(rows)) => Some(rows.into_iter().map(whole_numbers).collect()),
        Mixed::Absent => Some(Vec::new()),
        Mixed::Same(_) | Mixed::Mixed => None,
    }
}

/// The edit that makes `rows` the list `key` of every selected layer: one `node.set` with the whole
/// list, so one undo step however many layers there are.
pub(super) fn replace(sel: &DesignSelection, key: &str, rows: Vec<Value>) -> Edit {
    sel.set(key, Value::Array(rows))
}

/// The edit that removes row `index` of list `key`. Refused ([`refused`]) when there is no such
/// row, or the layers hold different lists.
pub(super) fn remove_row(sel: &DesignSelection, key: &str, index: usize) -> Edit {
    match rows(sel, key) {
        Some(mut list) if index < list.len() => {
            list.remove(index);
            replace(sel, key, list)
        }
        _ => refused(),
    }
}

/// The edit that makes row `index` of list `key` the row `change` gives for it, which sees the row
/// as read. Refused ([`refused`]) when there is no such row, the layers hold different lists, or
/// `change` gives none.
pub(super) fn change_row(
    sel: &DesignSelection,
    key: &str,
    index: usize,
    change: impl FnOnce(&Value) -> Option<Value>,
) -> Edit {
    let Some(mut list) = rows(sel, key) else {
        return refused();
    };
    let Some(slot) = list.get_mut(index) else {
        return refused();
    };
    let Some(row) = change(slot) else {
        return refused();
    };
    *slot = row;
    replace(sel, key, list)
}

/// The edit for a request that can't apply: a row that isn't in the list, or a list that differs
/// from layer to layer. It names no layers and sets nothing, so `node.set` refuses it twice over
/// and the document stays as it was, the way [`DesignSelection::set`] answers a text key when no
/// text is selected.
pub(super) fn refused() -> Edit {
    Edit {
        command: "node.set",
        params: json!({"ids": [], "props": {}}),
    }
}

/// `value` with each number that is whole held as an integer. The engine's integer fields (an image
/// paint's `rotation`, a pattern's `source`) refuse `0.0` and `3.0`, while its float fields take
/// `0` and `3` as well as `0.0` and `3.0`. [`DesignSelection::read`] holds every number as an `f64`
/// so that equal values compare equal, which turns those integers into floats; a row sent back as
/// it was read would be refused.
pub(super) fn whole_numbers(value: Value) -> Value {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(float) if number.is_f64() && float.fract() == 0.0 && float.abs() < MAX_WHOLE => {
                Value::from(float as i64)
            }
            _ => Value::Number(number),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(whole_numbers).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key, whole_numbers(value)))
                .collect(),
        ),
        other => other,
    }
}

/// What the paint and effect tests share: a design file to edit, and the calls that read it and
/// run edits the way the panel does.
#[cfg(test)]
pub(super) mod testing {
    use figcraft_doc::{Document, NodeId};
    use figcraft_engine::Session;
    use serde_json::{Value, json};

    use crate::design::{DesignSelection, Edit};
    use crate::doc::BoardDoc;

    /// A design file with nothing on its page.
    pub fn design_file() -> BoardDoc {
        BoardDoc {
            session: Session::with_document(Document::new("Design")),
        }
    }

    /// A 100 x 100 rectangle called `name` at (`x`, 0), with the engine's default fill and nothing
    /// else.
    pub fn rectangle(board: &mut BoardDoc, name: &str, x: f64) -> NodeId {
        let params =
            json!({"type": "rectangle", "name": name, "x": x, "y": 0, "width": 100, "height": 100});
        let made = board.execute("node.create", &params).unwrap();
        NodeId(made["id"].as_u64().unwrap())
    }

    /// What layers `ids` hold now.
    pub fn read(board: &BoardDoc, ids: &[NodeId]) -> DesignSelection {
        DesignSelection::read(&board.doc().unwrap(), ids)
    }

    /// Runs `edit` as the panel does; the engine must take it.
    pub fn apply(board: &mut BoardDoc, edit: &Edit) {
        board.execute(edit.command, &edit.params).unwrap();
    }

    /// Makes an edit from what layers `ids` hold now and runs it, the way the panel does.
    pub fn edit(board: &mut BoardDoc, ids: &[NodeId], make: impl FnOnce(&DesignSelection) -> Edit) {
        let edit = make(&read(board, ids));
        apply(board, &edit);
    }

    /// `node.set` on `ids`, without the panel's model.
    pub fn node_set(board: &mut BoardDoc, ids: &[NodeId], props: Value) {
        board
            .execute("node.set", &json!({"ids": ids, "props": props}))
            .unwrap();
    }

    /// How many undo steps the file has.
    pub fn undo_steps(board: &BoardDoc) -> usize {
        board.session.undo_depth().0
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{design_file, read, rectangle};
    use super::*;
    use crate::doc::BoardError;

    /// An edit that can't apply is refused for having no layers, and would be refused for having
    /// nothing to set if the engine ever took an empty `ids` for the selection.
    #[test]
    fn a_refused_edit_names_no_layers_and_sets_nothing() {
        let edit = refused();
        assert_eq!(edit.command, "node.set");
        assert_eq!(edit.params, json!({"ids": [], "props": {}}));

        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        let before = read(&board, &[r]);
        let revision = board.revision();
        let result = board.execute(edit.command, &edit.params);
        assert!(matches!(result, Err(BoardError::Engine(_))), "{result:?}");
        // The same edit with a layer named: nothing to set.
        let named = json!({"ids": [r], "props": {}});
        let result = board.execute(edit.command, &named);
        assert!(matches!(result, Err(BoardError::Engine(_))), "{result:?}");
        assert_eq!(board.revision(), revision);
        assert_eq!(read(&board, &[r]), before);
    }

    #[test]
    fn whole_numbers_become_integers_and_nothing_else_changes() {
        let rows = json!([{
            "rotation": 0.0,
            "source": 3.0,
            "scale": 1.5,
            "crop": null,
            "name": "1.0",
            "visible": true,
            "stops": [{"position": 1.0}, {"position": 0.25}],
            "count": 7,
            "negative": -2.0,
            "minus zero": -0.0,
            "huge": 1e300,
        }]);
        assert_eq!(
            whole_numbers(rows),
            json!([{
                "rotation": 0,
                "source": 3,
                "scale": 1.5,
                "crop": null,
                "name": "1.0",
                "visible": true,
                "stops": [{"position": 1}, {"position": 0.25}],
                "count": 7,
                "negative": -2,
                "minus zero": 0,
                "huge": 1e300,
            }])
        );
    }
}
