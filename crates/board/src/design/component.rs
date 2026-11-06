//! The Component section's model: what the one selected main component, component set or
//! instance is, and its properties: an instance's values (a variant's options, a boolean, a text,
//! the component a nested instance swaps to), a main's or a set's definitions with their
//! defaults. The edits use the engine's component commands, each one undo step:
//! `instance.setProperties`, `instance.swap`, `component.addProperty`, `component.editProperty`,
//! `component.deleteProperty` and `component.addVariant`.

use figcraft_doc::{Document, NodeId, NodeKind, PropDef, ordered_props};
use serde_json::{Value, json};

use super::Edit;

/// What the selected layer is.
#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    /// A main component, and the set it is a variant of.
    Main {
        set: Option<NodeId>,
    },
    Set,
    /// An instance of `main`.
    Instance {
        main: NodeId,
        main_name: String,
    },
}

/// What a property takes.
#[derive(Clone, Debug, PartialEq)]
pub enum PropKind {
    Boolean,
    Text,
    /// A nested instance's component: the components it can swap to, each its id and name.
    InstanceSwap(Vec<(NodeId, String)>),
    /// One of `options`.
    Variant(Vec<String>),
    /// A slot, which takes content of its own; it has no value to edit here.
    Slot,
}

/// One property, with the instance's value or the main's default.
#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub name: String,
    pub kind: PropKind,
    pub value: Value,
}

/// The section's model for the one selected layer.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentPanel {
    pub id: NodeId,
    pub role: Role,
    pub props: Vec<Prop>,
}

/// The types `component.addProperty` takes, as the panel offers them.
pub const NEW_PROPERTY_TYPES: [(&str, &str); 4] = [
    ("boolean", "Boolean"),
    ("text", "Text"),
    ("instanceSwap", "Instance swap"),
    ("variant", "Variant"),
];

/// The panel for `ids` when it is one main component, set or instance; `None` otherwise.
pub fn panel(doc: &Document, ids: &[NodeId]) -> Option<ComponentPanel> {
    let [id] = ids else { return None };
    let node = doc.node(*id)?;
    let swaps = swap_targets(doc);
    match &node.kind {
        NodeKind::Instance { component, props } => {
            let main = doc.node(*component)?;
            let set = set_of(doc, *component);
            let mut list = variant_props(doc, set, Some(&main.name));
            for prop in definitions(doc, *component, &swaps) {
                let value = props.get(&prop.name).cloned().unwrap_or(prop.value.clone());
                list.push(Prop { value, ..prop });
            }
            let main_name = match set.and_then(|s| doc.node(s)) {
                Some(set) => set.name.clone(),
                None => main.name.clone(),
            };
            Some(ComponentPanel {
                id: *id,
                role: Role::Instance {
                    main: *component,
                    main_name,
                },
                props: list,
            })
        }
        NodeKind::Component { .. } => {
            let set = set_of(doc, *id);
            let mut list = variant_props(doc, set, None);
            list.extend(definitions(doc, *id, &swaps));
            Some(ComponentPanel {
                id: *id,
                role: Role::Main { set },
                props: list,
            })
        }
        NodeKind::ComponentSet { .. } => {
            let mut list = variant_props(doc, Some(*id), None);
            if let Some(first) = node.children.first() {
                list.extend(definitions(doc, *first, &swaps));
            }
            Some(ComponentPanel {
                id: *id,
                role: Role::Set,
                props: list,
            })
        }
        _ => None,
    }
}

/// The set component `id` is a variant of.
fn set_of(doc: &Document, id: NodeId) -> Option<NodeId> {
    let parent = doc.node(id)?.parent?;
    matches!(doc.node(parent)?.kind, NodeKind::ComponentSet { .. }).then_some(parent)
}

/// A variant's name as its properties: `Size=Large, State=Hover`.
fn pairs(name: &str) -> Vec<(String, String)> {
    name.split(',')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// A set's variant properties, each with its options in the order the variants hold them; the
/// value is the variant `current`'s (an instance's main), else the first variant's.
fn variant_props(doc: &Document, set: Option<NodeId>, current: Option<&str>) -> Vec<Prop> {
    let Some(set) = set.and_then(|s| doc.node(s)) else {
        return Vec::new();
    };
    let mut keys: Vec<(String, Vec<String>)> = Vec::new();
    for child in &set.children {
        let Some(n) = doc.node(*child) else { continue };
        for (k, v) in pairs(&n.name) {
            match keys.iter_mut().find(|(key, _)| *key == k) {
                Some((_, values)) if !values.contains(&v) => values.push(v),
                Some(_) => {}
                None => keys.push((k, vec![v])),
            }
        }
    }
    let first = set
        .children
        .first()
        .and_then(|c| doc.node(*c))
        .map(|n| n.name.clone())
        .unwrap_or_default();
    let shown = pairs(current.unwrap_or(&first));
    keys.into_iter()
        .map(|(name, options)| {
            let value = shown
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.clone())
                .or_else(|| options.first().cloned())
                .unwrap_or_default();
            Prop {
                name,
                kind: PropKind::Variant(options),
                value: json!(value),
            }
        })
        .collect()
}

/// Component `id`'s own property definitions, each with its default.
fn definitions(doc: &Document, id: NodeId, swaps: &[(NodeId, String)]) -> Vec<Prop> {
    let Some(NodeKind::Component {
        props, prop_order, ..
    }) = doc.node(id).map(|n| &n.kind)
    else {
        return Vec::new();
    };
    ordered_props(props, prop_order)
        .into_iter()
        .map(|(name, def)| {
            let (kind, value) = match def {
                PropDef::Boolean { default } => (PropKind::Boolean, json!(default)),
                PropDef::Text { default } => (PropKind::Text, json!(default)),
                PropDef::InstanceSwap { default, .. } => {
                    (PropKind::InstanceSwap(swaps.to_vec()), json!(default))
                }
                PropDef::Variant { options, default } => {
                    (PropKind::Variant(options.clone()), json!(default))
                }
                _ => (PropKind::Slot, Value::Null),
            };
            Prop {
                name: name.clone(),
                kind,
                value,
            }
        })
        .collect()
}

/// Every component an instance can swap to: the sets, and the components in none, by name.
pub fn swap_targets(doc: &Document) -> Vec<(NodeId, String)> {
    let mut out: Vec<(NodeId, String)> = doc
        .nodes()
        .filter(|n| match n.kind {
            NodeKind::ComponentSet { .. } => true,
            NodeKind::Component { .. } => set_of(doc, n.id).is_none(),
            _ => false,
        })
        .map(|n| (n.id, n.name.clone()))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    out
}

/// The edit that gives property `name` of instance `id` the value `value`: a variant's option, a
/// boolean, a text, or the id of the component a nested instance swaps to.
pub fn set_value(id: NodeId, name: &str, value: Value) -> Edit {
    Edit {
        command: "instance.setProperties",
        params: json!({"ids": [id], "properties": {name: value}}),
    }
}

/// The edit that swaps instance `id` for an instance of `component`.
pub fn swap(id: NodeId, component: NodeId) -> Edit {
    Edit {
        command: "instance.swap",
        params: json!({"ids": [id], "component": component}),
    }
}

/// The edit that adds a property of `kind` (one of [`NEW_PROPERTY_TYPES`]) named `name` to main
/// component or set `id`.
pub fn add_property(id: NodeId, name: &str, kind: &str) -> Edit {
    Edit {
        command: "component.addProperty",
        params: json!({"id": id, "name": name, "type": kind}),
    }
}

/// The edit that renames property `name` of main component or set `id` to `new_name`.
pub fn rename_property(id: NodeId, name: &str, new_name: &str) -> Edit {
    Edit {
        command: "component.editProperty",
        params: json!({"id": id, "name": name, "newName": new_name}),
    }
}

/// The edit that makes `value` property `name`'s default on main component or set `id`.
pub fn set_default(id: NodeId, name: &str, value: Value) -> Edit {
    Edit {
        command: "component.editProperty",
        params: json!({"id": id, "name": name, "default": value}),
    }
}

/// The edit that deletes property `name` of main component or set `id`.
pub fn delete_property(id: NodeId, name: &str) -> Edit {
    Edit {
        command: "component.deleteProperty",
        params: json!({"id": id, "name": name}),
    }
}

/// The edit that adds a variant to main component or set `id` (a main not in a set yet becomes a
/// set of itself and the new variant).
pub fn add_variant(id: NodeId) -> Edit {
    Edit {
        command: "component.addVariant",
        params: json!({"id": id}),
    }
}

/// A name for a new property of `kind` that `panel` hasn't yet: `Property 1`, `Property 2`, …
pub fn new_property_name(panel: &ComponentPanel) -> String {
    (1..)
        .map(|n| format!("Property {n}"))
        .find(|name| panel.props.iter().all(|p| p.name != *name))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
