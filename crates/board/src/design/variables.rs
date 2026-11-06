//! Variables and modes: the Variables table (each collection's modes across, its variables down,
//! each cell the variable's value in that mode), the picker that binds a layer's field to a
//! variable, and the mode a frame shows a collection in. The edits use the engine's variable
//! commands, each one undo step.

use figcraft_doc::{Document, NodeId, VariableType, VariableValue};
use serde_json::{Value, json};

use super::Edit;
use super::paints::{byte, hex_of, parse_hex, parse_rgb};

/// The variable types a new variable can have, as `variable.create` takes them.
pub const TYPES: [(&str, &str); 4] = [
    ("color", "Colour"),
    ("number", "Number"),
    ("string", "String"),
    ("boolean", "Boolean"),
];

/// One variable's row of the table.
#[derive(Clone, Debug, PartialEq)]
pub struct VariableRow {
    pub id: String,
    pub name: String,
    /// `color`, `number`, `string` or `boolean`.
    pub kind: &'static str,
    /// Its value in each of the collection's modes, as typed: `#RRGGBB`, `16`, `Hello`, `true`,
    /// or `→ other` for an alias.
    pub values: Vec<String>,
}

/// One collection of the table.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectionRow {
    pub id: String,
    pub name: String,
    /// Each mode's id and name; the first is the default.
    pub modes: Vec<(String, String)>,
    pub variables: Vec<VariableRow>,
}

fn kind_name(kind: VariableType) -> &'static str {
    match kind {
        VariableType::Color => "color",
        VariableType::Number => "number",
        VariableType::String => "string",
        VariableType::Boolean => "boolean",
    }
}

/// `value` as a cell shows it, `doc` naming the variables it reads.
fn shown(doc: &Document, value: Option<&VariableValue>) -> String {
    let name = |id: &str| {
        doc.variables
            .get_by_id(id)
            .map_or_else(|| id.to_string(), |v| v.name.clone())
    };
    match value {
        None => String::new(),
        Some(VariableValue::Color(c)) => {
            let rgb = hex_of([c.r, c.g, c.b].map(|v| byte(f64::from(v))));
            if c.a < 1.0 {
                format!("{rgb}{:02X}", byte(f64::from(c.a)))
            } else {
                rgb
            }
        }
        Some(VariableValue::Number(n)) => trimmed(*n),
        Some(VariableValue::String(s)) => s.clone(),
        Some(VariableValue::Boolean(b)) => b.to_string(),
        Some(VariableValue::Alias(id)) => format!("→ {}", name(id)),
        Some(VariableValue::ColorAlpha { variable, opacity }) => {
            format!("{} at {}%", name(variable), (opacity * 100.0).round())
        }
        Some(VariableValue::ColorMix { .. }) => "Mix".to_string(),
    }
}

fn trimmed(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// The document's collections, each with its modes and variables.
pub fn table(doc: &Document) -> Vec<CollectionRow> {
    doc.variables
        .collections
        .iter()
        .map(|c| CollectionRow {
            id: c.id.clone(),
            name: c.name.clone(),
            modes: c
                .modes
                .iter()
                .map(|m| (m.id.clone(), m.name.clone()))
                .collect(),
            variables: doc
                .variables
                .variables
                .iter()
                .filter(|v| v.collection == c.id)
                .map(|v| VariableRow {
                    id: v.id.clone(),
                    name: v.name.clone(),
                    kind: kind_name(v.kind),
                    values: c
                        .modes
                        .iter()
                        .map(|m| shown(doc, v.values.get(&m.id)))
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

/// The variables of type `kind` a field can bind to, as (id, `Collection/Name`).
pub fn bindable(doc: &Document, kind: &str) -> Vec<(String, String)> {
    doc.variables
        .variables
        .iter()
        .filter(|v| kind_name(v.kind) == kind)
        .map(|v| {
            let collection = doc
                .variables
                .collection_by_id(&v.collection)
                .map_or("", |c| c.name.as_str());
            (v.id.clone(), format!("{collection}/{}", v.name))
        })
        .collect()
}

/// The variable layer `id`'s `field` (`fills/0/color`, `opacity`, …) is bound to, as its id.
pub fn bound_to(doc: &Document, id: NodeId, field: &str) -> Option<String> {
    doc.node(id)?.bound.get(field).cloned()
}

/// What a typed value is for a variable of `kind`: a colour's hex, a number, a string or a
/// boolean; `→ name` aliases another variable.
fn parse_value(kind: &str, typed: &str) -> Option<Value> {
    let typed = typed.trim();
    if let Some(alias) = typed.strip_prefix('→').or_else(|| typed.strip_prefix("->")) {
        return Some(json!({"alias": alias.trim()}));
    }
    match kind {
        "color" => {
            let digits = typed.trim_start_matches('#');
            if digits.len() == 8 {
                let rgb = parse_rgb(&digits[..6])?;
                let a = u8::from_str_radix(&digits[6..], 16).ok()?;
                let unit = |c: u8| f64::from(c) / 255.0;
                return Some(
                    json!({"r": unit(rgb[0]), "g": unit(rgb[1]), "b": unit(rgb[2]), "a": unit(a)}),
                );
            }
            parse_hex(typed).map(Value::String)
        }
        "number" => typed
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(|n| json!(n)),
        "boolean" => match typed.to_ascii_lowercase().as_str() {
            "true" | "on" | "yes" => Some(json!(true)),
            "false" | "off" | "no" => Some(json!(false)),
            _ => None,
        },
        _ => Some(json!(typed)),
    }
}

/// The edit that gives variable `variable` (of `kind`) the value typed in mode `mode`; `None`
/// when what was typed is no value of that kind.
pub fn set_value(variable: &str, kind: &str, mode: &str, typed: &str) -> Option<Edit> {
    let value = parse_value(kind, typed)?;
    Some(Edit {
        command: "variable.set",
        params: json!({"variable": variable, "mode": mode, "value": value}),
    })
}

/// The edit that makes a variable of `kind` called `name` in collection `collection`, with that
/// kind's plain value.
pub fn create_variable(collection: &str, name: &str, kind: &str) -> Edit {
    let value = match kind {
        "color" => json!("#000000"),
        "number" => json!(0),
        "boolean" => json!(false),
        _ => json!(""),
    };
    Edit {
        command: "variable.create",
        params: json!({"collection": collection, "name": name, "type": kind, "value": value}),
    }
}

/// The edit that renames variable `variable`.
pub fn rename_variable(variable: &str, name: &str) -> Edit {
    Edit {
        command: "variable.update",
        params: json!({"variable": variable, "name": name}),
    }
}

/// The edit that deletes variable `variable`: bound layers keep their values.
pub fn delete_variable(variable: &str) -> Edit {
    Edit {
        command: "variable.delete",
        params: json!({"variable": variable}),
    }
}

/// The edit that makes a collection called `name`, with one mode.
pub fn create_collection(name: &str) -> Edit {
    Edit {
        command: "variable.createCollection",
        params: json!({"name": name}),
    }
}

/// The edit that renames collection `collection`.
pub fn rename_collection(collection: &str, name: &str) -> Edit {
    Edit {
        command: "variable.renameCollection",
        params: json!({"collection": collection, "name": name}),
    }
}

/// The edit that deletes collection `collection` and its variables.
pub fn delete_collection(collection: &str) -> Edit {
    Edit {
        command: "variable.deleteCollection",
        params: json!({"collection": collection}),
    }
}

/// The edit that adds a mode called `name` to `collection`, its values those of the default mode.
pub fn add_mode(collection: &str, name: &str) -> Edit {
    Edit {
        command: "variable.addMode",
        params: json!({"collection": collection, "name": name}),
    }
}

/// The edit that renames mode `mode` of `collection`.
pub fn rename_mode(collection: &str, mode: &str, name: &str) -> Edit {
    Edit {
        command: "variable.renameMode",
        params: json!({"collection": collection, "mode": mode, "name": name}),
    }
}

/// The edit that deletes mode `mode` of `collection` (a collection keeps one at least).
pub fn remove_mode(collection: &str, mode: &str) -> Edit {
    Edit {
        command: "variable.removeMode",
        params: json!({"collection": collection, "mode": mode}),
    }
}

/// The edit that binds `field` of layers `ids` to `variable`, or with `None` lets it go.
pub fn bind(ids: &[NodeId], field: &str, variable: Option<&str>) -> Edit {
    Edit {
        command: "variable.bind",
        params: json!({"ids": ids, "field": field, "variable": variable}),
    }
}

/// The edit that shows frames `ids` in mode `mode` of `collection`, or with `None` in the mode
/// they inherit (Auto).
pub fn set_mode(ids: &[NodeId], collection: &str, mode: Option<&str>) -> Edit {
    Edit {
        command: "variable.setMode",
        params: json!({"ids": ids, "collection": collection, "mode": mode}),
    }
}

/// The mode of `collection` layer `id` shows, set on it: `None` for Auto.
pub fn mode_of(doc: &Document, id: NodeId, collection: &str) -> Option<String> {
    doc.node(id)?.var_modes.get(collection).cloned()
}

/// A name for a new item that `taken` hasn't yet: `base`, `base 2`, `base 3`, …
pub fn fresh_name(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
