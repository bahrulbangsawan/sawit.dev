//! The design module: the contract's fields, and its color math.

use serde_json::Value;

use crate::browser::DESIGN_JS;

/// Every field of the design contract fixtures is written by the design module.
#[test]
fn design_module_writes_the_contract_fields() {
    fn keys(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, v) in m {
                    out.push(k.clone());
                    keys(v, out);
                }
            }
            Value::Array(a) => a.iter().for_each(|v| keys(v, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    for fixture in [
        include_str!("../../site_design/fixtures/page_scan.json"),
        include_str!("../../site_design/fixtures/page_rules.json"),
        include_str!("../../site_design/fixtures/page_style.json"),
    ] {
        keys(&serde_json::from_str(fixture).unwrap(), &mut all);
    }
    for key in all {
        let written = [
            format!("{key}:"),
            format!("{{ {key},"),
            format!(" {key},"),
            format!(" {key} }}"),
        ];
        assert!(
            written.iter().any(|w| DESIGN_JS.contains(w.as_str())),
            "DESIGN_JS never writes `{key}`"
        );
    }
}

/// The design module's color reader handles the syntaxes WebKit computes (Tailwind v4's
/// palette is oklch), so chromatic buttons and translucent layers are read on such pages.
#[test]
fn design_color_math_reads_modern_syntax() {
    let start = DESIGN_JS.find("// ---- Color math").unwrap();
    let end = DESIGN_JS.find("// ---- End of color math").unwrap();
    let source = format!(
        "(() => {{ {}\n const out = (c) => {{ const p = readColor(c); return p ? [Math.round(p[0]), Math.round(p[1]), Math.round(p[2]), p[3], Math.round(chromaOf(p) * 1000) / 1000] : null; }};\n\
             return JSON.stringify([\"oklch(0.623 0.214 259.815)\", \"oklch(0.985 0 0)\", \"oklab(0.628 0.225 0.126)\", \"lab(54.29 80.8 69.89)\", \"lch(54.29 106.84 40.85 / 0.5)\", \"color(display-p3 1 0 0)\", \"color(srgb 0 0.5 1)\", \"rgb(0 113 227 / 50%)\", \"rgba(0, 0, 0, 0)\", \"#0f7fff\", \"oklch(0.7 0.1 none)\", \"hsl(0 0% 0%)\"].map(out)); }})()",
        &DESIGN_JS[start..end]
    );
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let json: String = ctx.with(|ctx| ctx.eval::<String, _>(source).unwrap());
    let got: Vec<Option<[f64; 5]>> = serde_json::from_str(&json).unwrap();
    let near = |i: usize, rgb: [f64; 3], alpha: f64| {
        let c = got[i].unwrap_or_else(|| panic!("color {i} unread"));
        for k in 0..3 {
            assert!((c[k] - rgb[k]).abs() <= 2.0, "color {i}: {c:?} vs {rgb:?}");
        }
        assert!((c[3] - alpha).abs() < 1e-6, "color {i} alpha: {c:?}");
        c[4]
    };
    // Tailwind v4 blue-500 (just outside sRGB, clipped): chromatic.
    assert!(near(0, [43.0, 127.0, 255.0], 1.0) >= 0.03);
    // Tailwind neutral-50: not.
    assert!(near(1, [250.0, 250.0, 250.0], 1.0) < 0.03);
    near(2, [255.0, 0.0, 0.0], 1.0);
    near(3, [255.0, 0.0, 0.0], 1.0);
    near(4, [255.0, 0.0, 0.0], 0.5);
    // Out of sRGB: clamped.
    near(5, [255.0, 0.0, 0.0], 1.0);
    near(6, [0.0, 128.0, 255.0], 1.0);
    near(7, [0.0, 113.0, 227.0], 0.5);
    near(8, [0.0, 0.0, 0.0], 0.0);
    near(9, [15.0, 127.0, 255.0], 1.0);
    assert!(got[10].is_some());
    // Left to the browser's canvas in the page.
    assert!(got[11].is_none());
}
