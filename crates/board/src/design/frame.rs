//! The Design panel's frame and placement sections: a layer's constraints (how it follows its
//! frame when the frame is resized), its minimum and maximum sizes and whether it ignores its
//! auto layout, a grid layout's columns and rows, and a frame's layout grids (columns, rows or a
//! square grid drawn over it).
//!
//! Constraints are one `constraints` object of `node.set` (`{horizontal, vertical}`): an edit of
//! one axis sends the other as it was. Layout grids are the `grids` list; a grid is added with
//! `grid.add`, removed with `grid.remove`, and changed by sending the list back with that grid
//! changed.

use serde_json::{Map, Value, json};

use super::lists::{refused, rows};
use super::{DesignSelection, Edit, Mixed};

/// The horizontal constraints, as `node.set` takes them and as the panel names them.
pub const HORIZONTAL: [(&str, &str); 5] = [
    ("left", "Left"),
    ("right", "Right"),
    ("leftRight", "Left and right"),
    ("center", "Center"),
    ("scale", "Scale"),
];

/// The vertical constraints.
pub const VERTICAL: [(&str, &str); 5] = [
    ("top", "Top"),
    ("bottom", "Bottom"),
    ("topBottom", "Top and bottom"),
    ("center", "Center"),
    ("scale", "Scale"),
];

/// The sizes a layer can be held between: `minWidth`, `maxWidth`, `minHeight` and `maxHeight`.
pub const LIMITS: [(&str, &str); 4] = [
    ("minWidth", "Min W"),
    ("maxWidth", "Max W"),
    ("minHeight", "Min H"),
    ("maxHeight", "Max H"),
];

/// The layout grid kinds.
pub const GRID_KINDS: [(&str, &str); 3] =
    [("grid", "Grid"), ("columns", "Columns"), ("rows", "Rows")];

/// How columns or rows sit in the frame.
pub const GRID_ALIGNS: [(&str, &str); 4] = [
    ("stretch", "Stretch"),
    ("min", "Left / Top"),
    ("center", "Center"),
    ("max", "Right / Bottom"),
];

/// The selection's constraint on `axis` (`horizontal` or `vertical`): `None` when the layers
/// differ or none has constraints (a layer has them only inside a frame, outside auto layout).
pub fn constraint(sel: &DesignSelection, axis: &str) -> Option<String> {
    match sel.get("constraints") {
        Mixed::Same(Value::Object(c)) => c.get(axis).and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

/// Whether the selection's layers take constraints: every one of them holds some.
pub fn has_constraints(sel: &DesignSelection) -> bool {
    !matches!(sel.get("constraints"), Mixed::Absent)
}

/// The edit that sets constraint `axis` of every selected layer to `value`, keeping each layer's
/// other axis.
pub fn set_constraint(sel: &DesignSelection, axis: &str, value: &str) -> Edit {
    let known = match axis {
        "horizontal" => HORIZONTAL.iter().any(|(v, _)| *v == value),
        "vertical" => VERTICAL.iter().any(|(v, _)| *v == value),
        _ => false,
    };
    if !known {
        return refused();
    }
    let other = if axis == "horizontal" {
        "vertical"
    } else {
        "horizontal"
    };
    let mut constraints = Map::new();
    constraints.insert(axis.into(), json!(value));
    if let Some(kept) = constraint(sel, other) {
        constraints.insert(other.into(), json!(kept));
    }
    sel.set("constraints", Value::Object(constraints))
}

/// The selection's layout grids, when every layer holds the same list.
pub fn grids(sel: &DesignSelection) -> Option<Vec<GridRow>> {
    Some(rows(sel, "grids")?.iter().map(grid_row).collect())
}

/// One layout grid as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct GridRow {
    pub kind: String,
    /// A square grid's cell size, or fixed columns' or rows' width.
    pub size: f64,
    /// How many columns or rows (0: as many as fit).
    pub count: f64,
    pub gutter: f64,
    pub offset: f64,
    pub align: String,
    pub visible: bool,
}

fn grid_row(grid: &Value) -> GridRow {
    let n = |k: &str, or: f64| grid.get(k).and_then(Value::as_f64).unwrap_or(or);
    let s = |k: &str, or: &str| {
        grid.get(k)
            .and_then(Value::as_str)
            .unwrap_or(or)
            .to_string()
    };
    GridRow {
        kind: s("kind", "grid"),
        size: n("size", 10.0),
        count: n("count", 5.0),
        gutter: n("gutter", 20.0),
        offset: n("offset", 0.0),
        align: s("align", "stretch"),
        visible: grid.get("visible").and_then(Value::as_bool).unwrap_or(true),
    }
}

/// The edit that adds a layout grid of `kind` with the engine's defaults.
pub fn add_grid(sel: &DesignSelection, kind: &str) -> Edit {
    if !GRID_KINDS.iter().any(|(k, _)| *k == kind) {
        return refused();
    }
    Edit {
        command: "grid.add",
        params: json!({"ids": sel.ids, "grid": {"kind": kind}}),
    }
}

/// The edit that removes layout grid `index`.
pub fn remove_grid(sel: &DesignSelection, index: usize) -> Edit {
    match rows(sel, "grids") {
        Some(list) if index < list.len() => Edit {
            command: "grid.remove",
            params: json!({"ids": sel.ids, "index": index}),
        },
        _ => refused(),
    }
}

/// The edit that sets `field` of layout grid `index` to `value`: `kind` and `align` take one of
/// [`GRID_KINDS`] and [`GRID_ALIGNS`]; `size`, `count`, `gutter` and `offset` a number (`count` a
/// whole one, 0 meaning as many as fit; `size` above 0; the others not below 0, but `offset`);
/// `visible` true or false.
pub fn set_grid(sel: &DesignSelection, index: usize, field: &str, value: Value) -> Edit {
    let Some(mut list) = rows(sel, "grids") else {
        return refused();
    };
    let Some(grid) = list.get_mut(index).and_then(Value::as_object_mut) else {
        return refused();
    };
    let ok = match field {
        "kind" => value
            .as_str()
            .is_some_and(|v| GRID_KINDS.iter().any(|(k, _)| *k == v)),
        "align" => value
            .as_str()
            .is_some_and(|v| GRID_ALIGNS.iter().any(|(k, _)| *k == v)),
        "visible" => value.is_boolean(),
        "size" => value.as_f64().is_some_and(|n| n.is_finite() && n > 0.0),
        "count" => value
            .as_f64()
            .is_some_and(|n| n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= 1000.0),
        "gutter" => value.as_f64().is_some_and(|n| n.is_finite() && n >= 0.0),
        "offset" => value.as_f64().is_some_and(f64::is_finite),
        _ => false,
    };
    if !ok {
        return refused();
    }
    let value = match (field, value.as_f64()) {
        ("count", Some(n)) => json!(n as u64),
        _ => value,
    };
    grid.insert(field.into(), value);
    sel.set("grids", Value::Array(list))
}

#[cfg(test)]
mod tests;
