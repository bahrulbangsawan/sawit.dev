//! tokens.json: the tokens in the DTCG (Design Tokens Community Group) format.

use serde_json::{Map, Value, json};

use super::px;
use crate::site_design::color::STEPS;
use crate::site_design::tokens::{Tokens, parse_box_shadow};

fn token(ty: &str, value: Value) -> Value {
    json!({ "$type": ty, "$value": value })
}

fn put(root: &mut Map<String, Value>, group: &str, key: &str, v: Value) {
    let g = root
        .entry(group.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(m) = g {
        m.insert(key.to_string(), v);
    }
}

pub fn dtcg(t: &Tokens) -> Value {
    let mut root = Map::new();
    let mut color = Map::new();
    for r in &t.palette {
        let mut steps = Map::new();
        for (step, c) in STEPS.iter().zip(r.steps.iter()) {
            steps.insert(step.to_string(), token("color", json!(c.to_hex())));
        }
        color.insert(r.role.clone(), Value::Object(steps));
    }
    for (name, c) in &t.semantic {
        let tok = token("color", json!(c.to_hex()));
        match color.get_mut(name) {
            // A ramp of the same name is a group; DTCG keeps the group's own token in `$root`.
            Some(Value::Object(group)) => {
                group.insert("$root".into(), tok);
            }
            _ => {
                color.insert(name.clone(), tok);
            }
        }
    }
    if !color.is_empty() {
        root.insert("color".into(), Value::Object(color));
    }
    if !t.families.is_empty() || !t.type_scale.is_empty() {
        let mut font = Map::new();
        let family: Map<String, Value> = t
            .families
            .iter()
            .map(|(k, v)| (k.clone(), token("fontFamily", json!(v))))
            .collect();
        if !family.is_empty() {
            font.insert("family".into(), Value::Object(family));
        }
        let size: Map<String, Value> = t
            .type_scale
            .iter()
            .map(|(k, v)| (k.clone(), token("dimension", json!(px(*v)))))
            .collect();
        if !size.is_empty() {
            font.insert("size".into(), Value::Object(size));
        }
        root.insert("font".into(), Value::Object(font));
    }
    for (k, v) in &t.spacing {
        put(&mut root, "spacing", k, token("dimension", json!(px(*v))));
    }
    for (k, v) in &t.radius {
        put(&mut root, "radius", k, token("dimension", json!(v)));
    }
    for (k, raw) in &t.shadows {
        let Some(s) = parse_box_shadow(raw) else {
            continue;
        };
        let value = json!({
            "color": s.color.to_hex(),
            "offsetX": px(s.x),
            "offsetY": px(s.y),
            "blur": px(s.blur),
            "spread": px(s.spread),
        });
        put(&mut root, "shadow", k, token("shadow", value));
    }
    Value::Object(root)
}
