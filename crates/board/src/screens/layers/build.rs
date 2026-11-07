//! Makes a [`Layer`] tree on the canvas, in one undo step: frames, text, images and SVG pictures
//! with the page's looks, auto layout where [`super::plan`] found it, and the page's colours and
//! type sizes as variables the layers are bound to (an existing variable of the same value is
//! used; a new one is named by its role: `text/1`, `bg/2`, `border/1`, `type/16`).

use std::collections::BTreeMap;

use base64::Engine as _;
use figcraft_doc::{NodeId, VariableValue};
use figcraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use super::looks::{frame_props, hex, role_name, sizing_name, text_props};
use super::plan::{Kind, Layer, Sizing};
use super::{Capture, Skipped};
use crate::doc::BoardDoc;

/// What a build made.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildReport {
    pub frame: NodeId,
    pub layers: usize,
    pub auto_layouts: usize,
    pub texts: usize,
    pub pictures: usize,
    pub variables: Vec<String>,
    pub skipped: Skipped,
}

/// The colours and sizes a build binds, by value: the variable each is bound to.
#[derive(Default)]
struct Tokens {
    colors: BTreeMap<String, String>,
    sizes: BTreeMap<u32, String>,
    made: Vec<String>,
}

fn run(s: &mut Session, id: &str, params: Value) -> Result<Value, EngineError> {
    s.execute(id, &params)
}

fn id_of(v: &Value) -> Result<NodeId, EngineError> {
    figcraft_engine::params::id_of(&v["id"])
        .ok_or_else(|| EngineError::Other("the engine made no layer".into()))
}

/// Builds `root` (a page `capture`'s plan) as a frame named `name` with its top-left at page point
/// `at` on page `page`, and renames a layer if `rename` says so, in one undo step.
pub fn build(
    board: &mut BoardDoc,
    root: &Layer,
    capture: &Capture,
    page: NodeId,
    at: (f64, f64),
    name: &str,
    rename: Option<(NodeId, String)>,
) -> Result<BuildReport, String> {
    let mut report = BuildReport {
        frame: page,
        layers: 0,
        auto_layouts: 0,
        texts: 0,
        pictures: 0,
        variables: Vec::new(),
        skipped: capture.skipped.clone(),
    };
    let made = crate::doc::grouped(&mut board.session, "Convert to Layers", |s| {
        let mut tokens = Tokens::default();
        prepare_tokens(s, root, &mut tokens)?;
        let frame = make(
            s,
            root,
            page,
            [at.0, at.1],
            Some(name),
            &tokens,
            &mut report,
        )?;
        if let Some((id, new_name)) = &rename {
            run(
                s,
                "node.set",
                json!({"id": id, "props": {"name": new_name}}),
            )?;
        }
        report.variables = tokens.made;
        Ok(frame)
    })
    .map_err(|e| e.to_string())?;
    report.frame = made;
    Ok(report)
}

/// The variables the layers bind: the file's colour and number variables, by value, and a new
/// one for each colour and type size the page has that none holds.
fn prepare_tokens(s: &mut Session, root: &Layer, tokens: &mut Tokens) -> Result<(), EngineError> {
    let mut colors: Vec<(&'static str, String)> = Vec::new();
    let mut sizes: Vec<u32> = Vec::new();
    collect(root, &mut colors, &mut sizes);
    let doc = s.doc()?;
    let mut taken: Vec<String> = doc
        .variables
        .variables
        .iter()
        .map(|v| v.name.clone())
        .collect();
    for v in &doc.variables.variables {
        match v.values.values().next() {
            Some(VariableValue::Color(c)) => {
                let [r, g, b, a] = c.to_rgba8();
                let key = match a {
                    255 => format!("#{r:02x}{g:02x}{b:02x}"),
                    a => format!("#{r:02x}{g:02x}{b:02x}{a:02x}"),
                };
                tokens.colors.entry(key).or_insert_with(|| v.name.clone());
            }
            Some(VariableValue::Number(n)) if v.name.starts_with("type/") => {
                tokens
                    .sizes
                    .entry(n.round() as u32)
                    .or_insert_with(|| v.name.clone());
            }
            _ => {}
        }
    }
    let has = |s: &Session, name: &str| {
        s.doc()
            .map(|d| d.variables.collections.iter().any(|c| c.name == name))
            .unwrap_or(false)
    };
    let next = |role: &str, taken: &mut Vec<String>| {
        let name = (1..)
            .map(|n| format!("{role}/{n}"))
            .find(|n| !taken.contains(n))
            .unwrap_or_default();
        taken.push(name.clone());
        name
    };
    for (role, color) in colors {
        if tokens.colors.contains_key(&color) {
            continue;
        }
        if !has(s, "Color") {
            run(s, "variable.createCollection", json!({"name": "Color"}))?;
        }
        let name = next(role, &mut taken);
        run(
            s,
            "variable.create",
            json!({"collection": "Color", "name": name, "type": "color", "value": color}),
        )?;
        tokens.made.push(name.clone());
        tokens.colors.insert(color, name);
    }
    for size in sizes {
        if tokens.sizes.contains_key(&size) {
            continue;
        }
        if !has(s, "Scale") {
            run(s, "variable.createCollection", json!({"name": "Scale"}))?;
        }
        let name = format!("type/{size}");
        if !taken.contains(&name) {
            run(
                s,
                "variable.create",
                json!({"collection": "Scale", "name": name, "type": "number", "value": size}),
            )?;
            tokens.made.push(name.clone());
            taken.push(name.clone());
        }
        tokens.sizes.insert(size, name);
    }
    Ok(())
}

/// The colours `layer`'s tree paints, with their role, most used first within a role, and its
/// type sizes.
fn collect(layer: &Layer, colors: &mut Vec<(&'static str, String)>, sizes: &mut Vec<u32>) {
    let mut push = |role: &'static str, css: &Option<String>| {
        if let Some(h) = css.as_deref().and_then(hex)
            && !colors.iter().any(|(_, c)| *c == h)
        {
            colors.push((role, h));
        }
    };
    match &layer.kind {
        Kind::Text(_) => {
            push("text", &layer.style.c);
            let size = layer.style.fs.round() as u32;
            if size > 0 && !sizes.contains(&size) {
                sizes.push(size);
            }
        }
        Kind::Frame | Kind::Slot => {
            push("bg", &layer.style.bg);
            if layer.style.bd.is_some() {
                push("border", &layer.style.bc);
            }
        }
        _ => {}
    }
    for kid in &layer.kids {
        collect(kid, colors, sizes);
    }
}

/// Binds `field` of `id` to the variable holding `css`'s colour.
fn bind_color(
    s: &mut Session,
    id: NodeId,
    field: &str,
    css: &Option<String>,
    tokens: &Tokens,
) -> Result<(), EngineError> {
    let Some(variable) = css
        .as_deref()
        .and_then(hex)
        .and_then(|h| tokens.colors.get(&h))
    else {
        return Ok(());
    };
    run(
        s,
        "variable.bind",
        json!({"ids": [id], "field": field, "variable": variable}),
    )
    .map(drop)
}

/// Makes `layer` in `parent`, whose top-left is page point `origin`, and its children; answers its
/// id.
fn make(
    s: &mut Session,
    layer: &Layer,
    parent: NodeId,
    origin: [f64; 2],
    name: Option<&str>,
    tokens: &Tokens,
    report: &mut BuildReport,
) -> Result<NodeId, EngineError> {
    let [x, y, w, h] = layer.rect;
    // The root goes where `origin` says; the rest relative to their parent's top-left.
    let (rx, ry) = match name {
        Some(_) => (origin[0], origin[1]),
        None => (x - origin[0], y - origin[1]),
    };
    let named = name.map(str::to_string).or_else(|| layer.name.clone());
    report.layers += 1;
    let id = match &layer.kind {
        Kind::Text(text) => {
            report.texts += 1;
            let mut params = json!({"text": text, "x": rx, "y": ry, "parent": parent,
                "select": false, "props": text_props(layer)});
            let fits = layer.one_line() && layer.sizing.0 != Sizing::Fill;
            if !fits {
                params["width"] = json!(w.max(1.0));
            }
            let id = id_of(&run(s, "text.create", params)?)?;
            bind_color(s, id, "fills/0/color", &layer.style.c, tokens)?;
            if let Some(var) = tokens.sizes.get(&(layer.style.fs.round() as u32)) {
                run(
                    s,
                    "variable.bind",
                    json!({"ids": [id], "field": "fontSize", "variable": var}),
                )?;
            }
            id
        }
        Kind::Image(data) => {
            report.pictures += 1;
            id_of(&run(
                s,
                "file.placeImage",
                json!({"data": data, "x": rx, "y": ry, "width": w.max(1.0), "height": h.max(1.0),
                    "parent": parent, "name": named.clone().unwrap_or_else(|| "Image".into())}),
            )?)?
        }
        Kind::Svg(svg) => {
            report.pictures += 1;
            let data = base64::engine::general_purpose::STANDARD.encode(svg);
            let title = named.clone().unwrap_or_else(|| "Icon".into());
            let id = id_of(&run(
                s,
                "file.import",
                json!({"data": data, "name": format!("{title}.svg"), "x": 0, "y": 0}),
            )?)?;
            run(s, "node.reparent", json!({"ids": [id], "parent": parent}))?;
            run(
                s,
                "node.set",
                json!({"id": id, "props": {"x": rx, "y": ry, "name": title}}),
            )?;
            id
        }
        Kind::Frame | Kind::Slot => {
            let frame_name = match (&named, &layer.kind) {
                (Some(n), Kind::Slot) => format!("Image slot / {n}"),
                (None, Kind::Slot) => "Image slot".to_string(),
                (Some(n), _) => n.clone(),
                (None, _) => role_name(&layer.style, layer.tag.as_deref()),
            };
            let id = id_of(&run(
                s,
                "frame.create",
                json!({"x": rx, "y": ry, "width": w.max(1.0), "height": h.max(1.0),
                    "parent": parent, "name": frame_name, "select": false,
                    "props": frame_props(layer)}),
            )?)?;
            bind_color(s, id, "fills/0/color", &layer.style.bg, tokens)?;
            if layer.style.bd.is_some() {
                bind_color(s, id, "strokes/0/color", &layer.style.bc, tokens)?;
            }
            id
        }
    };
    let mut kids = Vec::new();
    for kid in &layer.kids {
        kids.push(make(s, kid, id, [x, y], None, tokens, report)?);
    }
    if let Some(l) = &layer.layout {
        report.auto_layouts += 1;
        let [t, r, b, lft] = l.padding;
        run(
            s,
            "node.set",
            json!({"id": id, "props": {
                "layout": if l.vertical { "vertical" } else { "horizontal" },
                "gap": l.gap, "paddingTop": t, "paddingRight": r, "paddingBottom": b,
                "paddingLeft": lft, "mainAlign": l.main, "crossAlign": l.cross,
                "sizingH": "fixed", "sizingV": "fixed",
            }}),
        )?;
        for (kid, kid_id) in layer.kids.iter().zip(&kids) {
            let props = match kid.absolute {
                true => json!({"absolute": true, "x": kid.rect[0] - x, "y": kid.rect[1] - y}),
                false => json!({"sizingH": sizing_name(kid.sizing.0),
                                "sizingV": sizing_name(kid.sizing.1)}),
            };
            run(s, "node.set", json!({"id": kid_id, "props": props}))?;
        }
    }
    Ok(id)
}
