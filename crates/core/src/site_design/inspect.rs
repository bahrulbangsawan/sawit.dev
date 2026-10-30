//! Inspect details: the JSON contract of the page's `style` op, plus contrast and a CSS rule.

use serde::{Deserialize, Serialize};

use super::color::{self, Rgba};
use crate::annotations::Rect;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoxModel {
    pub margin: [f64; 4],
    pub border: [f64; 4],
    pub padding: [f64; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Typography {
    pub family: String,
    pub used_family: Option<String>,
    pub size: f64,
    pub weight: u16,
    pub line_height: Option<f64>,
    pub letter_spacing: Option<f64>,
    pub color: String,
    pub align: String,
    pub transform: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Background {
    pub color: String,
    pub image: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Border {
    pub color: String,
    pub style: String,
    pub width: [f64; 4],
    pub radius: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effects {
    pub shadow: Option<String>,
    pub opacity: f64,
    pub transition: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub display: String,
    pub position: String,
    pub gap: Option<String>,
    pub flex_direction: Option<String>,
    pub justify: Option<String>,
    pub align: Option<String>,
    pub grid_template_columns: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleInfo {
    pub selector: String,
    pub label: String,
    pub rect: Rect,
    #[serde(rename = "box")]
    pub box_model: BoxModel,
    pub typography: Typography,
    pub background: Background,
    pub border: Border,
    pub effects: Effects,
    pub layout: Layout,
    pub states: Vec<(String, Vec<(String, String)>)>,
    pub effective_background: Option<String>,
    pub html: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contrast {
    pub ratio: f64,
    pub large: bool,
    pub aa: bool,
    pub aaa: bool,
}

/// WCAG contrast of the text color on the effective background (white when unknown).
pub fn contrast(info: &StyleInfo) -> Option<Contrast> {
    let fg = color::parse(&info.typography.color)?;
    let white = Rgba {
        r: 255,
        g: 255,
        b: 255,
        a: 1.0,
    };
    let bg = info
        .effective_background
        .as_deref()
        .and_then(color::parse)
        .unwrap_or(white);
    let ratio = color::contrast_ratio(fg, bg);
    let t = &info.typography;
    let large = t.size >= 24.0 || (t.size >= 18.66 && t.weight >= 700);
    let (aa_min, aaa_min) = if large { (3.0, 4.5) } else { (4.5, 7.0) };
    Some(Contrast {
        ratio,
        large,
        aa: ratio >= aa_min,
        aaa: ratio >= aaa_min,
    })
}

/// A number without a trailing `.0`, at most 3 decimals.
pub(crate) fn num(n: f64) -> String {
    let s = format!("{:.3}", n);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn px(n: f64) -> String {
    format!("{}px", num(n))
}

/// Collapse top/right/bottom/left to the shortest CSS shorthand.
fn shorthand(v: [f64; 4]) -> String {
    let [t, r, b, l] = v;
    if t == r && r == b && b == l {
        px(t)
    } else if t == b && r == l {
        format!("{} {}", px(t), px(r))
    } else if r == l {
        format!("{} {} {}", px(t), px(r), px(b))
    } else {
        format!("{} {} {} {}", px(t), px(r), px(b), px(l))
    }
}

fn is_transparent(css: &str) -> bool {
    let c = css.trim();
    c.is_empty()
        || c.eq_ignore_ascii_case("transparent")
        || color::parse(c).is_some_and(|c| c.a == 0.0)
}

fn usable(v: &Option<String>) -> Option<&str> {
    v.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "none" && *s != "normal")
}

pub fn css_rule(info: &StyleInfo) -> String {
    let mut props: Vec<(&str, String)> = Vec::new();
    let l = &info.layout;
    let t = &info.typography;
    let m = &info.box_model;

    // 1. layout
    if l.display != "block" && l.display != "inline" && !l.display.is_empty() {
        props.push(("display", l.display.clone()));
    }
    if l.position != "static" && !l.position.is_empty() {
        props.push(("position", l.position.clone()));
    }
    if m.padding != [0.0; 4] {
        props.push(("padding", shorthand(m.padding)));
    }
    if m.margin != [0.0; 4] {
        props.push(("margin", shorthand(m.margin)));
    }
    if let Some(g) = usable(&l.gap)
        && g != "0px"
    {
        props.push(("gap", g.to_string()));
    }
    if let Some(v) = usable(&l.flex_direction)
        && v != "row"
    {
        props.push(("flex-direction", v.to_string()));
    }
    if let Some(v) = usable(&l.justify) {
        props.push(("justify-content", v.to_string()));
    }
    if let Some(v) = usable(&l.align) {
        props.push(("align-items", v.to_string()));
    }

    // 2. type
    let family = t
        .used_family
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&t.family);
    if !family.trim().is_empty() {
        props.push(("font-family", family.to_string()));
    }
    props.push(("font-size", px(t.size)));
    props.push(("font-weight", t.weight.to_string()));
    if let Some(lh) = t.line_height {
        props.push(("line-height", px(lh)));
    }
    if let Some(ls) = t.letter_spacing
        && ls != 0.0
    {
        props.push(("letter-spacing", px(ls)));
    }
    if t.align != "start" && t.align != "left" && !t.align.is_empty() {
        props.push(("text-align", t.align.clone()));
    }
    if t.transform != "none" && !t.transform.is_empty() {
        props.push(("text-transform", t.transform.clone()));
    }
    if !t.color.trim().is_empty() {
        props.push(("color", t.color.clone()));
    }

    // 3. background
    if !is_transparent(&info.background.color) {
        props.push(("background-color", info.background.color.clone()));
    }
    if let Some(img) = usable(&info.background.image) {
        props.push(("background-image", img.to_string()));
    }

    // 4. border and effects
    let b = &info.border;
    let styled = b.style != "none" && b.style != "hidden";
    if styled && b.width.iter().any(|w| *w > 0.0) {
        if b.width.iter().all(|w| *w == b.width[0]) {
            props.push((
                "border",
                format!("{} {} {}", px(b.width[0]), b.style, b.color),
            ));
        } else {
            props.push(("border-width", shorthand(b.width)));
            props.push(("border-style", b.style.clone()));
            props.push(("border-color", b.color.clone()));
        }
    }
    let radius = b.radius.trim();
    if !radius.is_empty() && radius != "0px" && radius != "0" {
        props.push(("border-radius", radius.to_string()));
    }
    if let Some(s) = usable(&info.effects.shadow) {
        props.push(("box-shadow", s.to_string()));
    }
    if info.effects.opacity != 1.0 {
        props.push(("opacity", num(info.effects.opacity)));
    }
    if let Some(tr) = usable(&info.effects.transition)
        && tr != "all 0s ease 0s"
    {
        props.push(("transition", tr.to_string()));
    }

    let mut out = format!("{} {{\n", info.selector);
    for (k, v) in props {
        out.push_str(&format!("  {k}: {v};\n"));
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn button() -> StyleInfo {
        serde_json::from_str(include_str!("fixtures/style_button.json")).unwrap()
    }

    /// What `browser_design.js`'s `style` emits, field for field.
    #[test]
    fn page_output_is_the_contract() {
        crate::site_design::assert_wire_exact::<StyleInfo>(include_str!(
            "fixtures/page_style.json"
        ));
    }

    #[test]
    fn contrast_of_white_on_blue() {
        let c = contrast(&button()).unwrap();
        assert!((c.ratio - 4.70).abs() < 0.01, "{}", c.ratio);
        assert!(!c.large);
        assert!(c.aa);
        assert!(!c.aaa);
    }

    #[test]
    fn contrast_large_and_unparsable() {
        let mut b = button();
        b.typography.size = 24.0;
        let c = contrast(&b).unwrap();
        assert!(c.large && c.aa);
        b.typography.color = "lab(1 2 3)".into();
        assert!(contrast(&b).is_none());
    }

    #[test]
    fn css_rule_skips_defaults() {
        let css = css_rule(&button());
        assert!(
            css.starts_with("button.buy {\n  display: inline-flex;\n"),
            "{css}"
        );
        assert!(css.contains("  border-radius: 980px;\n"));
        assert!(css.contains("  padding: 8px 16px;\n"));
        assert!(css.contains("  font-family: SF Pro Text;\n"));
        assert!(!css.contains("margin"));
        assert!(!css.contains("position"));
        assert!(!css.contains("letter-spacing"));
        assert!(!css.contains("text-align"));
        assert!(!css.contains("transition"));
        assert!(!css.contains("opacity"));
        assert!(!css.contains("border:"));
        assert!(css.ends_with("}\n"));
    }

    #[test]
    fn css_rule_border_and_effects() {
        let mut b = button();
        b.border.style = "solid".into();
        b.border.width = [1.0; 4];
        b.border.color = "rgb(0, 0, 0)".into();
        b.effects.opacity = 0.5;
        b.effects.shadow = Some("rgba(0, 0, 0, 0.2) 0px 2px 4px 0px".into());
        let css = css_rule(&b);
        assert!(css.contains("  border: 1px solid rgb(0, 0, 0);\n"));
        assert!(css.contains("  opacity: 0.5;\n"));
        assert!(css.contains("  box-shadow: rgba(0, 0, 0, 0.2) 0px 2px 4px 0px;\n"));
    }
}
