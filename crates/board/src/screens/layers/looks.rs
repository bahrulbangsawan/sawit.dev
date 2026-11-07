//! The looks a layer takes from its element (Convert to Layers): colours as hex, the font family
//! the canvas has, shadows as effects, a frame's fill, stroke, corners and effects, a text's type,
//! and a name for a frame the page gave none.

use figcraft_color::Color;
use serde_json::{Map, Value, json};

use super::plan::{Kind, Layer, Sizing};

/// `css` as `#rrggbb` (or `#rrggbbaa`), if it reads.
pub(super) fn hex(css: &str) -> Option<String> {
    let [r, g, b, a] = Color::parse_css(css)?.to_rgba8();
    Some(match a {
        255 => format!("#{r:02x}{g:02x}{b:02x}"),
        a => format!("#{r:02x}{g:02x}{b:02x}{a:02x}"),
    })
}

/// The family the canvas draws CSS's `font-family` stack in: the first family it has, the
/// system font for the system stack and sans-serif, Menlo for monospace, Georgia for serif.
pub(super) fn family(stack: &str) -> String {
    for name in stack.split(',') {
        let name = name.trim().trim_matches(['"', '\'']).trim();
        let generic = match name.to_ascii_lowercase().as_str() {
            "system-ui" | "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif"
            | "sans-serif" | "ui-rounded" => Some("SF Pro"),
            "ui-monospace" | "monospace" | "sfmono-regular" | "sf mono" => Some("Menlo"),
            "ui-serif" | "serif" => Some("Georgia"),
            _ => None,
        };
        if let Some(generic) = generic {
            return generic.to_string();
        }
        if !name.is_empty() && figcraft_text::has_family(name) {
            return name.to_string();
        }
    }
    "SF Pro".to_string()
}

/// The box-shadow's shadows as effects (`inset` ones as inner shadows).
pub(super) fn shadows(css: &str) -> Vec<Value> {
    let mut parts = Vec::new();
    let (mut depth, mut from) = (0, 0);
    for (i, c) in css.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&css[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&css[from..]);
    parts
        .into_iter()
        .filter_map(|part| {
            let part = part.trim();
            let inset = part.contains("inset");
            let (color, rest) = match part.find(')') {
                Some(end) if part.starts_with("rgb") => (&part[..=end], &part[end + 1..]),
                _ => ("rgba(0, 0, 0, 0.25)", part),
            };
            let n: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|w| w.trim_end_matches("px").parse().ok())
                .collect();
            let at = |i: usize| n.get(i).copied().unwrap_or(0.0);
            let kind = if inset { "innerShadow" } else { "dropShadow" };
            Some(
                json!({"type": kind, "color": hex(color)?, "offset": [at(0), at(1)],
                "radius": at(2), "spread": at(3)}),
            )
        })
        .collect()
}

/// The looks a frame (or an empty picture slot) takes from its element.
pub(super) fn frame_props(layer: &Layer) -> Map<String, Value> {
    let s = &layer.style;
    let mut p = Map::new();
    let fill = s.bg.as_deref().and_then(hex);
    p.insert(
        "fills".into(),
        match (&layer.kind, fill) {
            (_, Some(fill)) => json!([fill]),
            (Kind::Slot, None) => json!(["#8080801f"]),
            _ => json!([]),
        },
    );
    p.insert("clip".into(), json!(s.clip));
    if let Some(op) = s.op.filter(|o| *o < 0.99) {
        p.insert("opacity".into(), json!(op));
    }
    if let Some([tl, tr, br, bl]) = s.rd {
        match tl == tr && tr == br && br == bl {
            true => {
                p.insert("cornerRadius".into(), json!(tl));
            }
            false => {
                for (k, v) in [
                    ("topLeftRadius", tl),
                    ("topRightRadius", tr),
                    ("bottomRightRadius", br),
                    ("bottomLeftRadius", bl),
                ] {
                    p.insert(k.into(), json!(v));
                }
            }
        }
    }
    if let (Some([t, r, b, l]), Some(color)) = (s.bd, s.bc.as_deref().and_then(hex)) {
        p.insert("strokes".into(), json!([color]));
        p.insert("strokeAlign".into(), json!("inside"));
        match t == r && r == b && b == l {
            true => {
                p.insert("strokeWeight".into(), json!(t));
            }
            false => {
                p.insert("strokeWeight".into(), json!(t.max(r).max(b).max(l)));
                for (k, v) in [
                    ("strokeTopWeight", t),
                    ("strokeRightWeight", r),
                    ("strokeBottomWeight", b),
                    ("strokeLeftWeight", l),
                ] {
                    p.insert(k.into(), json!(v));
                }
            }
        }
    }
    if let Some(shadow) = &s.sh {
        let effects = shadows(shadow);
        if !effects.is_empty() {
            p.insert("effects".into(), Value::Array(effects));
        }
    }
    p
}

/// The type a text takes from its element.
pub(super) fn text_props(layer: &Layer) -> Map<String, Value> {
    let s = &layer.style;
    let mut p = Map::new();
    p.insert(
        "fontFamily".into(),
        json!(family(s.ff.as_deref().unwrap_or(""))),
    );
    p.insert("fontSize".into(), json!(s.fs.max(1.0)));
    p.insert(
        "fontWeight".into(),
        json!(if s.fw > 0.0 { s.fw } else { 400.0 }),
    );
    if let Some(lh) = s.lh {
        p.insert("lineHeight".into(), json!(lh));
    }
    if s.ls.abs() > 0.01 {
        p.insert("letterSpacing".into(), json!(s.ls));
    }
    let align = match s.ta.as_deref() {
        Some("center") => "center",
        Some("right" | "end") => "right",
        Some("justify") => "justified",
        _ => "left",
    };
    p.insert("textAlign".into(), json!(align));
    match s.tt.as_deref() {
        Some("uppercase") => {
            p.insert("textCase".into(), json!("upper"));
        }
        Some("lowercase") => {
            p.insert("textCase".into(), json!("lower"));
        }
        Some("capitalize") => {
            p.insert("textCase".into(), json!("title"));
        }
        _ => {}
    }
    match s.td.as_deref() {
        Some(d) if d.contains("underline") => {
            p.insert("textDecoration".into(), json!("underline"));
        }
        Some(d) if d.contains("line-through") => {
            p.insert("textDecoration".into(), json!("strikethrough"));
        }
        _ => {}
    }
    if let Some(color) = s.c.as_deref().and_then(hex) {
        p.insert("fills".into(), json!([color]));
    }
    p
}

pub(super) fn sizing_name(s: Sizing) -> &'static str {
    match s {
        Sizing::Fixed => "fixed",
        Sizing::Fill => "fill",
        Sizing::Hug => "hug",
    }
}

/// A name for a frame the page gave none, by its element's role (cnv.8: never a bare "Frame").
pub(super) fn role_name(style: &super::Style, tag: Option<&str>) -> String {
    let by_tag = match tag.unwrap_or("") {
        "header" => "Header",
        "footer" => "Footer",
        "nav" => "Navigation",
        "main" => "Main",
        "aside" => "Sidebar",
        "section" => "Section",
        "article" => "Article",
        "form" => "Form",
        "button" => "Button",
        "a" => "Link",
        "ul" | "ol" => "List",
        "li" => "List item",
        "input" | "textarea" | "select" => "Field",
        "label" => "Label",
        "table" => "Table",
        "tr" => "Row",
        "td" | "th" => "Cell",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "span" => "Text box",
        _ => "",
    };
    match (by_tag, style.d.as_deref()) {
        ("", Some(d)) if d.contains("flex") && style.fd.as_deref() == Some("column") => {
            "Column".into()
        }
        ("", Some(d)) if d.contains("flex") => "Row".into(),
        ("", _) if style.bg.is_some() || style.bd.is_some() => "Box".into(),
        ("", _) => "Group".into(),
        (name, _) => name.into(),
    }
}
