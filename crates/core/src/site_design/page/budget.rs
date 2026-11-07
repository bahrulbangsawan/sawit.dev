//! The scan's answer within its budget: [`fit`] halves its heaviest list until the answer, as
//! the page's `JSON.stringify` writes it ([`js_json_len`]), fits.

use serde_json::Value;

use crate::site_design::scan::Scan;

/// Over `max` bytes (as the page's `JSON.stringify` wrote the scan), halves the heaviest list
/// until it fits, at most 40 times, and marks the scan truncated.
pub fn fit(scan: &mut Scan, max: usize) {
    const LISTS: [&str; 9] = [
        "assets",
        "components",
        "texts",
        "colors",
        "variables",
        "font_faces",
        "shadows",
        "radii",
        "spacing",
    ];
    let size = |scan: &Scan| js_json_len(&serde_json::to_value(scan).unwrap_or_default());
    let mut bytes = size(scan);
    for _ in 0..40 {
        if bytes <= max {
            break;
        }
        let value = serde_json::to_value(&*scan).unwrap_or_default();
        let mut heaviest = None;
        let mut weight = 0;
        for name in LISTS {
            let list = &value[name];
            let w = if list.as_array().is_some_and(|a| !a.is_empty()) {
                js_json_len(list)
            } else {
                0
            };
            if w > weight {
                weight = w;
                heaviest = Some(name);
            }
        }
        let Some(name) = heaviest else { break };
        fn halve<T>(v: &mut Vec<T>) {
            v.truncate(v.len() / 2);
        }
        match name {
            "assets" => halve(&mut scan.assets),
            "components" => halve(&mut scan.components),
            "texts" => halve(&mut scan.texts),
            "colors" => halve(&mut scan.colors),
            "variables" => halve(&mut scan.variables),
            "font_faces" => halve(&mut scan.font_faces),
            "shadows" => halve(&mut scan.shadows),
            "radii" => halve(&mut scan.radii),
            _ => halve(&mut scan.spacing),
        }
        scan.truncated = true;
        bytes = size(scan);
    }
}

/// The length in UTF-8 bytes of `JSON.stringify(v)` in a browser: numbers as JavaScript writes
/// them (`16`, not `16.0`).
pub fn js_json_len(v: &Value) -> usize {
    match v {
        Value::Null => 4,
        Value::Bool(b) => {
            if *b {
                4
            } else {
                5
            }
        }
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => js_number(f).len(),
            _ => n.to_string().len(),
        },
        Value::String(s) => js_string_len(s),
        Value::Array(a) => 2 + a.len().saturating_sub(1) + a.iter().map(js_json_len).sum::<usize>(),
        Value::Object(m) => {
            2 + m.len().saturating_sub(1)
                + m.iter()
                    .map(|(k, v)| js_string_len(k) + 1 + js_json_len(v))
                    .sum::<usize>()
        }
    }
}

fn js_string_len(s: &str) -> usize {
    2 + s
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\u{8}' | '\u{c}' | '\n' | '\r' | '\t' => 2,
            c if (c as u32) < 0x20 => 6,
            c => c.len_utf8(),
        })
        .sum::<usize>()
}

/// A number as JavaScript's `String(n)` writes it.
pub fn js_number(f: f64) -> String {
    if !f.is_finite() {
        // JSON.stringify writes null.
        return "null".into();
    }
    if f == 0.0 {
        return "0".into();
    }
    let a = f.abs();
    if (1e-6..1e21).contains(&a) {
        if f.fract() == 0.0 {
            format!("{}", f as i128)
        } else {
            format!("{f}")
        }
    } else {
        let e = format!("{f:e}");
        match e.split_once('e') {
            Some((m, x)) if !x.starts_with('-') => format!("{m}e+{x}"),
            _ => e,
        }
    }
}
