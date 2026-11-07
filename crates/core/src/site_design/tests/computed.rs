//! The colour reading, against the page's JavaScript it replaced (`tests/color_math.js`, run in
//! QuickJS): the same answer for every colour, edge cases included.

use super::*;

/// The page's colour math as it was before it moved to Rust.
const OLD_JS: &str = include_str!("color_math.js");

/// `[r, g, b, a, chroma]` per colour from the old JavaScript; `None` where it read nothing.
fn old_js(colors: &[String]) -> Vec<Option<[Option<f64>; 5]>> {
    let source = format!(
        "(() => {{ {OLD_JS}\n const n = (x) => (Number.isNaN(x) ? null : x);\n\
         return JSON.stringify({}.map((c) => {{ const p = readColor(c); return p ? [n(p[0]), n(p[1]), n(p[2]), n(p[3]), n(chromaOf(p))] : null; }})); }})()",
        serde_json::to_string(colors).unwrap()
    );
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let json: String = ctx.with(|ctx| ctx.eval::<String, _>(source).unwrap());
    serde_json::from_str(&json).unwrap()
}

fn same(a: Option<f64>, b: f64) -> bool {
    match a {
        None => b.is_nan(),
        Some(a) => a == b || (a - b).abs() <= 1e-9 * a.abs().max(1.0),
    }
}

fn corpus() -> Vec<String> {
    let mut out: Vec<String> = [
        "oklch(0.623 0.214 259.815)",
        "oklch(0.985 0 0)",
        "oklab(0.628 0.225 0.126)",
        "lab(54.29 80.8 69.89)",
        "lch(54.29 106.84 40.85 / 0.5)",
        "color(display-p3 1 0 0)",
        "color(srgb 0 0.5 1)",
        "color(srgb-linear 0.5 0.1 0.1)",
        "color(srgb 1 0)",
        "color(srgb)",
        "color(display-p3 1 0)",
        "color(rec2020 1 0 0)",
        "color()",
        "rgb(0 113 227 / 50%)",
        "rgba(0, 0, 0, 0)",
        "rgba(0, 0, 0, 0.5)",
        "rgb(10, 20, 30)",
        "rgb(10, 20)",
        "rgb(10 20 30 40)",
        "rgb(10 20 30 / )",
        "rgb(1e2 5e-1 .5)",
        "rgb(-20 300 Infinity)",
        "rgb(10% 20% 30%)",
        "RGB( 1 , 2 , 3 )",
        "  #0F7FFF  ",
        "#0f7fff",
        "#abc",
        "#abcd",
        "#abcde",
        "#11223344",
        "#1122334455",
        "#zzz",
        "transparent",
        "TRANSPARENT",
        "",
        "   ",
        "red",
        "hsl(0 0% 0%)",
        "oklch(0.7 0.1 none)",
        "oklch(0.7 0.1 0.5turn)",
        "oklch(0.7 0.1 1rad)",
        "oklch(0.7 0.1 100grad)",
        "oklch(70% 0.1 30deg)",
        "oklch(0.7 40% 30)",
        "oklch(0.7)",
        "oklab(1)",
        "lab(50% 40% -40%)",
        "lch(50 30 none / none)",
        "rgb(1 2 3 / 0.5 / 0.2)",
        "rgb(calc(1) 2 3)",
        "rgb(1 2 3))",
        "-x(1 2 3)",
        "rgb(+5 -.5e1 5.)",
        "rgb(5abc 6 7)",
        "lab(0 0 0)",
        "lab(100 0 0)",
        "lab(5 0 0)",
        "oklch(0.5 0.4 -720)",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // A deterministic spread of the syntaxes WebKit computes.
    let mut seed: u64 = 0x5eed;
    let mut next = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for _ in 0..400 {
        let f = |k: u64| k as f64 / 1000.0;
        let c = match next(7) {
            0 => format!("rgb({}, {}, {})", next(256), next(256), next(256)),
            1 => format!(
                "rgba({}, {}, {}, {})",
                next(256),
                next(256),
                next(256),
                f(next(1001))
            ),
            2 => format!(
                "oklch({} {} {} / {})",
                f(next(1001)),
                f(next(400)),
                f(next(360_000)),
                f(next(1001))
            ),
            3 => format!(
                "oklab({} {} {})",
                f(next(1001)),
                f(next(800)) - 0.4,
                f(next(800)) - 0.4
            ),
            4 => format!(
                "lab({} {} {})",
                f(next(100_000)),
                f(next(250_000)) - 125.0,
                f(next(250_000)) - 125.0
            ),
            5 => format!(
                "color(display-p3 {} {} {})",
                f(next(1001)),
                f(next(1001)),
                f(next(1001))
            ),
            _ => format!("#{:06x}", next(0x1000000)),
        };
        out.push(c);
    }
    out
}

#[test]
fn reads_every_colour_as_the_page_did() {
    let colors = corpus();
    let old = old_js(&colors);
    for (c, want) in colors.iter().zip(old) {
        let got = read(c);
        match (want, got) {
            (None, None) => {}
            (Some(w), Some(g)) => {
                for k in 0..4 {
                    assert!(same(w[k], g[k]), "{c:?} part {k}: {w:?} vs {g:?}");
                }
                assert!(same(w[4], chroma_of(g)), "{c:?} chroma: {w:?}");
            }
            (w, g) => panic!("{c:?}: the page read {w:?}, Rust {g:?}"),
        }
    }
}

#[test]
fn parse_float_reads_like_javascript() {
    for (t, want) in [
        ("50%", 50.0),
        (".5", 0.5),
        ("5.", 5.0),
        ("1e3", 1000.0),
        ("1e", 1.0),
        ("-.5e1", -5.0),
        ("+2", 2.0),
        ("  7px", 7.0),
    ] {
        assert_eq!(parse_float(t), want, "{t}");
    }
    assert!(parse_float("abc").is_nan());
    assert!(parse_float(".").is_nan());
    assert_eq!(parse_float("-Infinity"), f64::NEG_INFINITY);
}

#[test]
fn transparent_and_chroma() {
    let none = Fallbacks::new();
    assert!(transparent(None, &none));
    assert!(transparent(Some(""), &none));
    assert!(transparent(Some("rgba(0, 0, 0, 0)"), &none));
    assert!(transparent(Some("oklch(0.5 0.1 20 / 0)"), &none));
    assert!(!transparent(Some("rgb(0, 0, 0)"), &none));
    // Unreadable and no reading from the browser: not transparent.
    assert!(!transparent(Some("hsl(0 0% 0% / 0)"), &none));
    let read_by_browser = Fallbacks::from([(
        "hsl(0 0% 0% / 0)".to_string(),
        Some("rgba(0, 0, 0, 0)".to_string()),
    )]);
    assert!(transparent(Some("hsl(0 0% 0% / 0)"), &read_by_browser));
    assert!(chroma("oklch(0.623 0.214 259.815)", &none) >= 0.03);
    assert!(chroma("oklch(0.985 0 0)", &none) < 0.03);
    assert_eq!(chroma("rgba(255, 0, 0, 0)", &none), 0.0);
    assert_eq!(chroma("nonsense", &none), 0.0);
}

#[test]
fn effective_background_composites_translucent_layers() {
    let none = Fallbacks::new();
    let layers = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(effective_background(&[], &none), "rgb(255, 255, 255)");
    assert_eq!(
        effective_background(
            &layers(&["rgba(0, 0, 0, 0)", "rgb(10, 20, 30)", "rgb(1, 1, 1)"]),
            &none
        ),
        "rgb(10, 20, 30)"
    );
    // Half black over white.
    assert_eq!(
        effective_background(&layers(&["rgba(0, 0, 0, 0.5)"]), &none),
        "rgb(128, 128, 128)"
    );
    // Two half-black layers and an unreadable one over an opaque base.
    assert_eq!(
        effective_background(
            &layers(&[
                "rgba(0, 0, 0, 0.5)",
                "rgba(0, 0, 0, 0.5)",
                "nonsense",
                "rgb(200, 100, 0)"
            ]),
            &none
        ),
        "rgb(50, 25, 0)"
    );
    // A part left out: NaN, written as JavaScript wrote it.
    assert_eq!(
        effective_background(&layers(&["color(srgb 1 0)"]), &none),
        "rgb(255, 0, NaN)"
    );
}

#[test]
fn js_round_rounds_halves_up() {
    assert_eq!(js_round(2.5), 3.0);
    assert_eq!(js_round(-2.5), -2.0);
    assert_eq!(js_round(0.49999999999999994), 0.0);
    assert!(js_round(-0.2).is_sign_positive());
}
