//! Map a `StyleInfo` to Tailwind v4 classes. Exact default matches use the named class,
//! everything else an arbitrary value.

use super::color;
use super::inspect::{StyleInfo, num};

/// `prefix` + a spacing step: `0` -> `-0`, `1` -> `-px`, even px -> `-{px/4}`, else `-[Npx]`.
pub fn spacing_class(prefix: &str, px: f64) -> String {
    if px < 0.0 {
        return format!("-{}", spacing_class(prefix, -px));
    }
    if px == 0.0 {
        format!("{prefix}-0")
    } else if px == 1.0 {
        format!("{prefix}-px")
    } else if px.fract() == 0.0 && px % 2.0 == 0.0 {
        format!("{prefix}-{}", num(px / 4.0))
    } else {
        format!("{prefix}-[{}px]", num(px))
    }
}

fn edges(prefix: &str, v: [f64; 4], out: &mut Vec<String>) {
    let [t, r, b, l] = v;
    if t == r && r == b && b == l {
        if t != 0.0 {
            out.push(spacing_class(prefix, t));
        }
        return;
    }
    let mut add = |suffix: &str, px: f64| {
        if px != 0.0 {
            out.push(spacing_class(&format!("{prefix}{suffix}"), px));
        }
    };
    if l == r {
        add("x", l);
    }
    if t == b {
        add("y", t);
    }
    if t != b {
        add("t", t);
    }
    if l != r {
        add("r", r);
    }
    if t != b {
        add("b", b);
    }
    if l != r {
        add("l", l);
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.005
}

fn opt(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn width_class(prefix: &str, w: f64) -> String {
    if w == 1.0 {
        prefix.to_string()
    } else if [0.0, 2.0, 4.0, 8.0].contains(&w) {
        format!("{prefix}-{}", num(w))
    } else {
        format!("{prefix}-[{}px]", num(w))
    }
}

fn radius_class(radius: &str) -> Option<String> {
    let r = radius.trim();
    if r.is_empty() {
        return None;
    }
    let mut parts = r.split_whitespace();
    let first = parts.next()?;
    if parts.any(|p| p != first) {
        // Per-corner radii: keep them verbatim.
        return Some(format!(
            "rounded-[{}]",
            r.split_whitespace().collect::<Vec<_>>().join("_")
        ));
    }
    if let Some(p) = first.strip_suffix('%') {
        let v: f64 = p.parse().ok()?;
        return match v {
            v if v >= 50.0 => Some("rounded-full".into()),
            v if v <= 0.0 => None,
            v => Some(format!("rounded-[{}%]", num(v))),
        };
    }
    let v: f64 = first.strip_suffix("px")?.parse().ok()?;
    if v <= 0.0 {
        return None;
    }
    if v >= 500.0 {
        return Some("rounded-full".into());
    }
    let named = [
        (2.0, "xs"),
        (4.0, "sm"),
        (6.0, "md"),
        (8.0, "lg"),
        (12.0, "xl"),
        (16.0, "2xl"),
        (24.0, "3xl"),
        (32.0, "4xl"),
    ];
    Some(match named.iter().find(|(n, _)| *n == v) {
        Some((_, name)) => format!("rounded-{name}"),
        None => format!("rounded-[{}px]", num(v)),
    })
}

fn gap_class(gap: &str) -> Option<String> {
    let vals: Vec<f64> = gap
        .split_whitespace()
        .map(|p| p.strip_suffix("px")?.parse::<f64>().ok())
        .collect::<Option<_>>()?;
    match vals.as_slice() {
        [v] if *v > 0.0 => Some(spacing_class("gap", *v)),
        [y, x] if y == x && *y > 0.0 => Some(spacing_class("gap", *y)),
        [y, x] => {
            let mut s = Vec::new();
            if *x > 0.0 {
                s.push(spacing_class("gap-x", *x));
            }
            if *y > 0.0 {
                s.push(spacing_class("gap-y", *y));
            }
            (!s.is_empty()).then(|| s.join(" "))
        }
        _ => None,
    }
}

pub fn classes(info: &StyleInfo) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let l = &info.layout;
    let t = &info.typography;

    // 1. layout
    let display = match l.display.as_str() {
        "flex" => Some("flex"),
        "inline-flex" => Some("inline-flex"),
        "grid" => Some("grid"),
        "inline-grid" => Some("inline-grid"),
        "block" => Some("block"),
        "inline-block" => Some("inline-block"),
        "none" => Some("hidden"),
        "contents" => Some("contents"),
        _ => None,
    };
    out.extend(display.map(String::from));
    match opt(&l.flex_direction) {
        Some("column") => out.push("flex-col".into()),
        Some("column-reverse") => out.push("flex-col-reverse".into()),
        Some("row-reverse") => out.push("flex-row-reverse".into()),
        _ => {}
    }
    let justify = match opt(&l.justify) {
        Some("flex-start" | "start") => Some("justify-start"),
        Some("flex-end" | "end") => Some("justify-end"),
        Some("center") => Some("justify-center"),
        Some("space-between") => Some("justify-between"),
        Some("space-around") => Some("justify-around"),
        Some("space-evenly") => Some("justify-evenly"),
        _ => None,
    };
    out.extend(justify.map(String::from));
    let items = match opt(&l.align) {
        Some("flex-start" | "start") => Some("items-start"),
        Some("flex-end" | "end") => Some("items-end"),
        Some("center") => Some("items-center"),
        Some("baseline") => Some("items-baseline"),
        Some("stretch") => Some("items-stretch"),
        _ => None,
    };
    out.extend(items.map(String::from));
    if let Some(g) = opt(&l.gap).and_then(gap_class) {
        out.extend(g.split(' ').map(String::from));
    }

    // 2. position
    if matches!(
        l.position.as_str(),
        "relative" | "absolute" | "fixed" | "sticky"
    ) {
        out.push(l.position.clone());
    }

    // 3. padding, margin
    edges("p", info.box_model.padding, &mut out);
    edges("m", info.box_model.margin, &mut out);

    // 4. text size
    if t.size > 0.0 {
        let named = [
            (12.0, "xs"),
            (14.0, "sm"),
            (16.0, "base"),
            (18.0, "lg"),
            (20.0, "xl"),
            (24.0, "2xl"),
            (30.0, "3xl"),
            (36.0, "4xl"),
            (48.0, "5xl"),
            (60.0, "6xl"),
            (72.0, "7xl"),
            (96.0, "8xl"),
            (128.0, "9xl"),
        ];
        out.push(match named.iter().find(|(n, _)| *n == t.size) {
            Some((_, name)) => format!("text-{name}"),
            None => format!("text-[{}px]", num(t.size)),
        });
    }

    // 5. weight
    let weight = match t.weight {
        100 => "font-thin".to_string(),
        200 => "font-extralight".into(),
        300 => "font-light".into(),
        400 => "font-normal".into(),
        500 => "font-medium".into(),
        600 => "font-semibold".into(),
        700 => "font-bold".into(),
        800 => "font-extrabold".into(),
        900 => "font-black".into(),
        0 => String::new(),
        w => format!("font-[{w}]"),
    };
    if !weight.is_empty() {
        out.push(weight);
    }

    // 6. leading
    if let Some(lh) = t.line_height.filter(|v| *v > 0.0) {
        let named = [
            (1.0, "none"),
            (1.25, "tight"),
            (1.375, "snug"),
            (1.5, "normal"),
            (1.625, "relaxed"),
            (2.0, "loose"),
        ];
        let ratio = if t.size > 0.0 { lh / t.size } else { f64::NAN };
        out.push(match named.iter().find(|(n, _)| near(*n, ratio)) {
            Some((_, name)) => format!("leading-{name}"),
            None => format!("leading-[{}px]", num(lh)),
        });
    }

    // 7. tracking
    if let Some(ls) = t.letter_spacing.filter(|v| v.abs() > 1e-6 && t.size > 0.0) {
        let em = ls / t.size;
        let named = [
            (-0.05, "tighter"),
            (-0.025, "tight"),
            (0.025, "wide"),
            (0.05, "wider"),
            (0.1, "widest"),
        ];
        out.push(match named.iter().find(|(n, _)| near(*n, em)) {
            Some((_, name)) => format!("tracking-{name}"),
            None => format!("tracking-[{}em]", num((em * 1000.0).round() / 1000.0)),
        });
    }

    // 8. align, transform
    match t.align.as_str() {
        "center" => out.push("text-center".into()),
        "right" | "end" => out.push("text-right".into()),
        _ => {}
    }
    match t.transform.as_str() {
        "uppercase" => out.push("uppercase".into()),
        "lowercase" => out.push("lowercase".into()),
        "capitalize" => out.push("capitalize".into()),
        _ => {}
    }

    // 9. colors
    if let Some(c) = color::parse(&t.color).filter(|c| c.a > 0.0) {
        out.push(format!("text-[{}]", c.to_hex()));
    }
    if let Some(c) = color::parse(&info.background.color).filter(|c| c.a > 0.0) {
        out.push(format!("bg-[{}]", c.to_hex()));
    }

    // 10. border
    let b = &info.border;
    if b.style != "none" && b.style != "hidden" && b.width.iter().any(|w| *w > 0.0) {
        let [t_, r, b_, l] = b.width;
        if t_ == r && r == b_ && b_ == l {
            out.push(width_class("border", t_));
        } else {
            for (p, w) in [
                ("border-t", t_),
                ("border-r", r),
                ("border-b", b_),
                ("border-l", l),
            ] {
                if w > 0.0 {
                    out.push(width_class(p, w));
                }
            }
        }
        if let Some(c) = color::parse(&b.color).filter(|c| c.a > 0.0) {
            out.push(format!("border-[{}]", c.to_hex()));
        }
    }

    // 11. radius
    out.extend(radius_class(&b.radius));

    // 12. shadow
    if let Some(s) = opt(&info.effects.shadow).filter(|s| *s != "none") {
        out.push(format!(
            "shadow-[{}]",
            s.split_whitespace().collect::<Vec<_>>().join("_")
        ));
    }

    // 13. opacity
    let o = info.effects.opacity;
    if o.is_finite() && (0.0..1.0).contains(&o) {
        let pct = o * 100.0;
        if near(pct % 5.0, 0.0) || near(pct % 5.0, 5.0) {
            out.push(format!("opacity-{}", pct.round() as i64));
        } else {
            let s = num(o);
            out.push(format!("opacity-[{}]", s.trim_start_matches('0')));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button() -> StyleInfo {
        serde_json::from_str(include_str!("fixtures/style_button.json")).unwrap()
    }

    #[test]
    fn spacing_steps() {
        assert_eq!(spacing_class("p", 16.0), "p-4");
        assert_eq!(spacing_class("p", 6.0), "p-1.5");
        assert_eq!(spacing_class("p", 13.0), "p-[13px]");
        assert_eq!(spacing_class("p", 1.0), "p-px");
        assert_eq!(spacing_class("p", 0.0), "p-0");
        assert_eq!(spacing_class("m", -8.0), "-m-2");
    }

    #[test]
    fn button_classes() {
        assert_eq!(
            classes(&button()),
            [
                "inline-flex",
                "justify-center",
                "items-center",
                "px-4",
                "py-2",
                "text-[17px]",
                "font-normal",
                "leading-[20px]",
                "text-[#ffffff]",
                "bg-[#0071e3]",
                "rounded-full"
            ]
        );
    }

    #[test]
    fn named_and_arbitrary_values() {
        let mut b = button();
        b.layout.flex_direction = Some("column".into());
        b.layout.gap = Some("16px".into());
        b.layout.position = "relative".into();
        b.box_model.padding = [8.0; 4];
        b.box_model.margin = [0.0, 4.0, 0.0, 12.0];
        b.typography.size = 16.0;
        b.typography.weight = 650;
        b.typography.line_height = Some(24.0);
        b.typography.letter_spacing = Some(-0.4);
        b.typography.align = "center".into();
        b.typography.transform = "uppercase".into();
        b.border.style = "solid".into();
        b.border.width = [1.0; 4];
        b.border.color = "rgb(0, 0, 0)".into();
        b.border.radius = "8px".into();
        b.effects.shadow = Some("rgba(0, 0, 0, 0.2) 0px 2px 4px 0px".into());
        b.effects.opacity = 0.5;
        let c = classes(&b);
        for want in [
            "flex-col",
            "gap-4",
            "relative",
            "p-2",
            "mr-1",
            "ml-3",
            "text-base",
            "font-[650]",
            "leading-normal",
            "tracking-tight",
            "text-center",
            "uppercase",
            "border",
            "border-[#000000]",
            "rounded-lg",
            "shadow-[rgba(0,_0,_0,_0.2)_0px_2px_4px_0px]",
            "opacity-50",
        ] {
            assert!(c.iter().any(|x| x == want), "missing {want} in {c:?}");
        }
        b.effects.opacity = 0.33;
        assert!(classes(&b).contains(&"opacity-[.33]".to_string()));
        b.border.radius = "50%".into();
        assert!(classes(&b).contains(&"rounded-full".to_string()));
    }

    #[test]
    fn odd_values_are_skipped() {
        let mut b = button();
        b.typography.color = "garbage".into();
        b.background.color = "rgba(0, 0, 0, 0)".into();
        b.border.radius = "wat".into();
        b.layout.gap = Some("normal".into());
        let c = classes(&b);
        assert!(
            !c.iter()
                .any(|x| x.starts_with("text-[#") || x.starts_with("bg-"))
        );
        assert!(
            !c.iter()
                .any(|x| x.starts_with("rounded") || x.starts_with("gap"))
        );
    }
}
