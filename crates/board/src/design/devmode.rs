//! Dev Mode's Inspect panel: what the one selected layer is to a developer. Its properties as
//! FigCraft's inspector lists them (layout, auto layout, typography, fills, strokes, effects, each
//! value with the style or variable it comes from), its code in one of FigCraft's languages
//! (CSS, HTML, React, Leptos, Vue, Tailwind, SwiftUI, Compose, ArkUI, Flutter; components take
//! their properties as parameters), its annotations, and its ready-for-dev status. Everything is
//! read from the document directly (`figcraft_engine::devinspect`, `figcraft_engine::codegen`),
//! so the panel runs no command to show it; its edits are one undo step each.

use figcraft_doc::{DevStatus, Document, NodeId};
use figcraft_engine::codegen::{CodeLang, code_for};
use figcraft_engine::devinspect::properties;
use serde_json::{Value, json};

use super::Edit;

/// One row of a section of the inspector.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectRow {
    pub label: String,
    pub value: String,
    /// A colour swatch for it, as `#RRGGBB` or `#RRGGBBAA`.
    pub color: Option<String>,
    /// The style or variable the value comes from.
    pub token: Option<String>,
}

/// One section of the inspector.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectSection {
    pub title: String,
    pub rows: Vec<InspectRow>,
}

/// The languages code comes in, as (id, label).
pub fn languages() -> Vec<(&'static str, &'static str)> {
    CodeLang::ALL.iter().map(|l| (l.id(), l.label())).collect()
}

/// Layer `id`'s properties, section by section.
pub fn inspect(doc: &Document, id: NodeId) -> Vec<InspectSection> {
    if doc.node(id).is_none() {
        return Vec::new();
    }
    properties(doc, id)
        .into_iter()
        .map(|section| InspectSection {
            title: section.title.to_string(),
            rows: section
                .rows
                .into_iter()
                .map(|p| InspectRow {
                    label: p.label,
                    value: p.value,
                    color: p.color,
                    token: p.token,
                })
                .collect(),
        })
        .collect()
}

/// Layer `id`'s code in language `language` (one of [`languages`]; CSS when it is none of them).
pub fn code(doc: &Document, id: NodeId, language: &str) -> String {
    if doc.node(id).is_none() {
        return String::new();
    }
    let lang = CodeLang::from_id(language).unwrap_or(CodeLang::Css);
    code_for(doc, id, lang)
}

/// Layer `id`'s annotations' notes, in order.
pub fn annotations(doc: &Document, id: NodeId) -> Vec<String> {
    doc.node(id)
        .map(|n| {
            n.annotations
                .iter()
                .map(|a| match (a.label.is_empty(), a.properties.is_empty()) {
                    (false, _) => a.label.clone(),
                    (true, false) => a.properties.join(", "),
                    (true, true) => "Annotation".to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether layer `id` is marked ready for development.
pub fn ready(doc: &Document, id: NodeId) -> bool {
    doc.node(id)
        .is_some_and(|n| n.dev_status == DevStatus::ReadyForDev)
}

/// The edit that adds an annotation saying `text` to layer `id`.
pub fn add_annotation(id: NodeId, text: &str) -> Edit {
    Edit {
        command: "annotation.add",
        params: json!({"id": id, "label": text}),
    }
}

/// The edit that removes annotation `index` of layer `id`.
pub fn remove_annotation(id: NodeId, index: usize) -> Edit {
    Edit {
        command: "annotation.remove",
        params: json!({"id": id, "index": index}),
    }
}

/// The edit that marks layer `id` ready for development, or takes the mark off.
pub fn set_ready(id: NodeId, ready: bool) -> Edit {
    let status = if ready {
        json!("readyForDev")
    } else {
        Value::Null
    };
    Edit {
        command: "node.set",
        params: json!({"ids": [id], "props": {"devStatus": status}}),
    }
}

#[cfg(test)]
mod tests;
