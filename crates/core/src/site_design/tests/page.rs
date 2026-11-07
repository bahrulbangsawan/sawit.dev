//! The page's answers to the design ops, and what Rust makes of them.

use serde_json::Value;

use super::*;
use crate::site_design::assert_wire_exact;
use crate::site_design::scan::ColorRole;

fn fixture_scan() -> PageScan {
    serde_json::from_str(include_str!("../fixtures/page_scan.json")).unwrap()
}

/// What `scan`, `scan_samples`, `rules` and `style` (`browser/page/design/`) emit, field for
/// field.
#[test]
fn page_output_is_the_contract() {
    assert_wire_exact::<PageScan>(include_str!("../fixtures/page_scan.json"));
    assert_wire_exact::<PageSamples>(include_str!("../fixtures/page_samples.json"));
    assert_wire_exact::<PageRules>(include_str!("../fixtures/page_rules.json"));
    // The style's own fields are StyleInfo's, but for the effective background, which Rust
    // works out.
    let json = include_str!("../fixtures/page_style.json");
    // Numbers compared by value (`16` and `16.0` alike).
    fn by_value(v: Value) -> Value {
        match v {
            Value::Number(n) => n.as_f64().map_or(Value::Null, |f| serde_json::json!(f)),
            Value::Array(a) => Value::Array(a.into_iter().map(by_value).collect()),
            Value::Object(m) => {
                Value::Object(m.into_iter().map(|(k, v)| (k, by_value(v))).collect())
            }
            v => v,
        }
    }
    let sent: Value = serde_json::from_str(json).unwrap();
    let mut back = serde_json::to_value(serde_json::from_str::<PageStyle>(json).unwrap()).unwrap();
    assert_eq!(back["effective_background"], Value::Null);
    back.as_object_mut().unwrap().remove("effective_background");
    assert_eq!(by_value(sent), by_value(back));
}

#[test]
fn plan_reads_colours_and_picks_buttons() {
    let plan = plan(fixture_scan());
    // The transparent background, though the most used, is dropped; the rest by count.
    assert!(
        plan.scan
            .colors
            .iter()
            .all(|c| c.value != "rgba(0, 0, 0, 0)")
    );
    assert_eq!(plan.scan.colors.len(), 8);
    assert_eq!(plan.scan.colors[0].role, ColorRole::Text);
    assert!(
        plan.scan
            .colors
            .windows(2)
            .all(|w| w[0].count >= w[1].count)
    );
    assert_eq!(
        plan.scan.body_background.as_deref(),
        Some("rgb(255, 255, 255)")
    );
    assert_eq!(plan.scan.focus_ring.as_deref(), Some("rgb(37, 99, 235)"));
    assert_eq!(
        plan.button,
        Some(ButtonPick {
            id: 0,
            bordered: false
        })
    );
    // The unfilled look counts its two bordered buttons.
    assert_eq!(
        plan.secondary,
        Some(ButtonPick {
            id: 1,
            bordered: true
        })
    );
    assert_eq!(
        plan.samples_op(),
        serde_json::json!({
            "op": "scan_samples",
            "button": {"id": 0, "bordered": false},
            "secondary": {"id": 1, "bordered": true},
            "card": 1,
            "link": 0,
            "limits": Limits::default()
        })
    );
    let samples: PageSamples =
        serde_json::from_str(include_str!("../fixtures/page_samples.json")).unwrap();
    let n = samples.components.len();
    let scan = plan.finish(samples);
    assert_eq!(scan.components.len(), n);
    assert!(!scan.truncated);
}

fn look(
    background: &str,
    count: u32,
    first: u32,
    bordered: u32,
    first_bordered: Option<u32>,
) -> ButtonLook {
    ButtonLook {
        background: background.into(),
        count,
        first,
        bordered,
        first_bordered,
    }
}

#[test]
fn button_picks_follow_the_pages_order() {
    let blue = "oklch(0.623 0.214 259.815)";
    let page = |buttons| PageScan {
        buttons,
        ..Default::default()
    };
    // Ties go to the look met first in the page, which for an unfilled look is its first
    // bordered button.
    let p = plan(page(vec![
        look("rgba(0, 0, 0, 0)", 9, 0, 3, Some(8)),
        look("rgb(240, 240, 240)", 3, 5, 0, None),
        look(blue, 2, 6, 0, None),
        look("red", 2, 1, 0, None),
    ]));
    assert_eq!(
        p.button,
        Some(ButtonPick {
            id: 2,
            bordered: false
        })
    );
    assert_eq!(
        p.secondary,
        Some(ButtonPick {
            id: 1,
            bordered: false
        })
    );
    // Unfilled with no border: doesn't count; no chromatic look: no buttons at all.
    let p = plan(page(vec![
        look("transparent", 9, 0, 0, None),
        look("rgb(240, 240, 240)", 3, 5, 0, None),
    ]));
    assert_eq!((p.button, p.secondary), (None, None));
    // An unreadable background the browser read as opaque and blue.
    let mut scan = page(vec![look("hwb(220 0% 0%)", 1, 0, 0, None)]);
    scan.color_fallbacks
        .insert("hwb(220 0% 0%)".into(), Some("#0055ff".into()));
    assert_eq!(
        plan(scan).button,
        Some(ButtonPick {
            id: 0,
            bordered: false
        })
    );
}

#[test]
fn colours_keep_the_first_five_hundred() {
    let colors = (0..600)
        .map(|i| ColorUse {
            value: format!("rgb({}, 0, 0)", i % 256),
            role: if i < 256 {
                ColorRole::Text
            } else {
                ColorRole::Border
            },
            count: (i % 7) as u32 + 1,
            on_button: 0,
            on_link: 0,
        })
        .collect::<Vec<_>>();
    let p = plan(PageScan {
        colors: colors.clone(),
        ..Default::default()
    });
    assert_eq!(p.scan.colors.len(), MAX_COLORS);
    // Stable: equal counts stay in the page's order.
    let sevens: Vec<_> = colors.iter().filter(|c| c.count == 7).collect();
    assert_eq!(&p.scan.colors[0], sevens[0]);
    assert_eq!(&p.scan.colors[1], sevens[1]);
}

/// `js_json_len` is `JSON.stringify(v)`'s length, as QuickJS writes it.
#[test]
fn json_length_is_javascripts() {
    let mut scan = plan(fixture_scan())
        .finish(serde_json::from_str(include_str!("../fixtures/page_samples.json")).unwrap());
    scan.spacing.push((0.1 + 0.2, 1));
    scan.spacing.push((1e-7, 1));
    scan.spacing.push((-3.0, 1));
    scan.title = "Ünïcode \"quoted\" \\ tab\t nl\n bell\u{7} 🙂".into();
    let value = serde_json::to_value(&scan).unwrap();
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let want: String = ctx.with(|ctx| {
        ctx.eval::<String, _>(format!(
            "(() => {{ const v = {value}; const s = JSON.stringify(v); let n = 0; for (const ch of s) {{ const c = ch.codePointAt(0); n += c < 0x80 ? 1 : c < 0x800 ? 2 : c < 0x10000 ? 3 : 4; }} return String(n); }})()"
        ))
        .unwrap()
    });
    assert_eq!(js_json_len(&value).to_string(), want);
    for (n, s) in [
        (16.0, "16"),
        (0.30000000000000004, "0.30000000000000004"),
        (1e-7, "1e-7"),
        (1.5e21, "1.5e+21"),
        (-0.0, "0"),
        (-2.5, "-2.5"),
        (123456.0, "123456"),
    ] {
        assert_eq!(js_number(n), s);
    }
}

#[test]
fn fit_halves_the_heaviest_list() {
    let mut scan = Scan {
        assets: (0..100)
            .map(|i| crate::site_design::scan::AssetRef {
                kind: crate::site_design::scan::AssetKind::Image,
                url: Some(format!("https://x.dev/{i}-{}.png", "a".repeat(200))),
                svg: None,
                label: "x".into(),
                width: None,
                height: None,
                used_on: vec![],
                logo: false,
            })
            .collect(),
        spacing: vec![(4.0, 2); 10],
        ..Default::default()
    };
    let before = js_json_len(&serde_json::to_value(&scan).unwrap());
    fit(&mut scan, before);
    assert!(!scan.truncated);
    fit(&mut scan, before / 3);
    assert!(scan.truncated);
    assert_eq!(scan.assets.len(), 25);
    assert_eq!(scan.spacing.len(), 10);
}

#[test]
fn rules_and_style_from_the_page() {
    let rules: SheetRules =
        serde_json::from_str::<PageRules>(include_str!("../fixtures/page_rules.json"))
            .unwrap()
            .into();
    assert_eq!(rules.focus_ring.as_deref(), Some("rgb(37, 99, 235)"));
    let style: StyleInfo =
        serde_json::from_str::<PageStyle>(include_str!("../fixtures/page_style.json"))
            .unwrap()
            .into();
    assert_eq!(
        style.effective_background.as_deref(),
        Some("rgb(250, 250, 251)")
    );
}

#[test]
fn plan_groups_and_ranks_what_the_page_read() {
    let plan = plan(fixture_scan());
    let s = &plan.scan;
    // 24.0001px rounds to the first style's 24px: one style, its characters added.
    assert_eq!(s.texts[0].chars, 2410);
    assert_eq!(s.texts[0].line_height, Some(24.0));
    assert_eq!(s.texts[0].size, 16.0);
    assert_eq!(s.texts[0].letter_spacing, None);
    assert!(s.texts.windows(2).all(|w| w[0].chars >= w[1].chars));
    // 4.2px rounds to 4px.
    assert_eq!(s.spacing, vec![(16.0, 80), (8.0, 60), (4.0, 23), (12.5, 2)]);
    assert!(s.breakpoints.windows(2).all(|w| w[0] <= w[1]));
    // The first of the most used card, the only link.
    assert_eq!((plan.card, plan.link), (Some(1), Some(0)));
    let op = plan.samples_op();
    assert_eq!(op["card"], 1);
    assert_eq!(op["limits"]["elements"], 6000);
}

/// The numbers as the page's JavaScript read them (`num`, `Math.round`), run in QuickJS.
#[test]
fn text_numbers_read_as_the_page_did() {
    let cases = [
        ("13.5px", "normal", "18.6667px", "-0.0005px"),
        ("", "650", "normal", "-0.2px"),
        ("1px", "399.5", "normal", "-0.0125px"),
        ("1e1px", "bold", "0.00049px", "0.0015px"),
        ("16px", "700", "24px", "-2.5005px"),
    ];
    let js = format!(
        "(() => {{ const num = (v) => {{ const n = parseFloat(v); return Number.isFinite(n) ? n : 0; }};\n\
         const round3 = (n) => Math.round(n * 1000) / 1000;\n\
         return JSON.stringify({}.map(([s, w, lh, ls]) => [num(s), Math.round(num(w)) || 400, lh === \"normal\" ? null : round3(num(lh)), ls === \"normal\" ? null : round3(num(ls))])); }})()",
        serde_json::to_string(&cases).unwrap()
    );
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let want: String = ctx.with(|ctx| ctx.eval::<String, _>(js).unwrap());
    let want: Vec<(f64, u16, Option<f64>, Option<f64>)> = serde_json::from_str(&want).unwrap();
    for ((size, weight, lh, ls), want) in cases.iter().zip(want) {
        let t = TextSample::from(PageText {
            size: size.to_string(),
            weight: weight.to_string(),
            line_height: lh.to_string(),
            letter_spacing: ls.to_string(),
            ..Default::default()
        });
        assert_eq!(
            (t.size, t.weight, t.line_height, t.letter_spacing),
            want,
            "{size} {weight} {lh} {ls}"
        );
    }
}

#[test]
fn variables_and_breakpoints_are_capped_and_sorted() {
    let page = PageScan {
        variables: (0..600)
            .map(|i| (format!("--v{i}"), "1".to_string()))
            .collect(),
        breakpoints: vec![1068, 734, 480],
        ..Default::default()
    };
    let s = plan(page).scan;
    assert_eq!(s.variables.len(), MAX_VARIABLES);
    assert_eq!(s.variables[0].0, "--v0");
    assert_eq!(s.breakpoints, vec![480, 734, 1068]);
    let rules: SheetRules = PageRules {
        variables: (0..600)
            .map(|i| (format!("--v{i}"), "1".to_string()))
            .collect(),
        breakpoints: vec![9, 3],
        ..Default::default()
    }
    .into();
    assert_eq!(
        (rules.variables.len(), rules.breakpoints),
        (MAX_VARIABLES, vec![3, 9])
    );
}
