//! The JSON contract of the page's `scan` and `rules` ops.
//!
//! Field names are the wire format: `browser_design.js` produces exactly these.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorRole {
    Text,
    Background,
    Border,
    Fill,
    Stroke,
    Shadow,
    Gradient,
}

/// One computed color value in one role, with how often it was seen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorUse {
    pub value: String,
    pub role: ColorRole,
    pub count: u32,
    /// Uses on button-like elements.
    #[serde(default)]
    pub on_button: u32,
    /// Uses on links that aren't button-like.
    #[serde(default)]
    pub on_link: u32,
}

/// A text style, keyed on the page by (tag bucket, family, size, weight, lh, ls, color,
/// in_main), with the characters set in it summed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSample {
    /// The lowercase tag, or `"button"` for button-like elements.
    pub tag: String,
    pub in_main: bool,
    /// The computed `font-family` stack.
    pub family: String,
    /// The first stack entry the page can render, or the generic.
    pub used_family: Option<String>,
    pub size: f64,
    pub weight: u16,
    /// px; `None` is `normal`.
    pub line_height: Option<f64>,
    /// px; `None` is `normal`.
    pub letter_spacing: Option<f64>,
    pub color: String,
    pub chars: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontFace {
    pub family: String,
    pub weight: String,
    pub style: String,
    pub src: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Image,
    Svg,
    Icon,
    Video,
    Lottie,
    Font,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetRef {
    pub kind: AssetKind,
    pub url: Option<String>,
    /// Inline SVG markup.
    pub svg: Option<String>,
    pub label: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default)]
    pub used_on: Vec<String>,
    #[serde(default)]
    pub logo: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentStyle {
    pub background: String,
    pub color: String,
    pub radius: String,
    pub padding: String,
    pub font_family: String,
    pub font_size: f64,
    pub font_weight: u16,
    pub height: f64,
    pub border: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentSample {
    /// `button`, `button-secondary`, `input`, `card`, `nav` or `link`.
    pub kind: String,
    pub selector: String,
    pub html: String,
    pub style: ComponentStyle,
}

/// One row of `assets.json`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AssetOutcome {
    pub file: Option<String>,
    pub url: Option<String>,
    pub kind: AssetKind,
    pub bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub label: String,
    pub used_on: Vec<String>,
    pub error: Option<String>,
}

/// What the page's `rules` op reads from stylesheets Sawit fetched for it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SheetRules {
    pub font_faces: Vec<FontFace>,
    pub variables: Vec<(String, String)>,
    pub breakpoints: Vec<u32>,
    pub focus_ring: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scan {
    pub url: String,
    pub title: String,
    pub viewport: (u32, u32),
    pub truncated: bool,
    pub colors: Vec<ColorUse>,
    pub texts: Vec<TextSample>,
    /// (px, count)
    pub spacing: Vec<(f64, u32)>,
    /// (computed radius, count)
    pub radii: Vec<(String, u32)>,
    /// (computed box-shadow, count)
    pub shadows: Vec<(String, u32)>,
    pub body_background: Option<String>,
    pub focus_ring: Option<String>,
    pub font_faces: Vec<FontFace>,
    pub variables: Vec<(String, String)>,
    pub breakpoints: Vec<u32>,
    pub assets: Vec<AssetRef>,
    pub components: Vec<ComponentSample>,
    /// Stylesheet hrefs whose `cssRules` the page couldn't read.
    pub blocked_sheets: Vec<String>,
    pub manifest: Option<String>,
}

impl Scan {
    /// Folds in what was read from fetched sheets. What the page itself reported wins.
    pub fn merge_rules(&mut self, rules: SheetRules) {
        for face in rules.font_faces {
            let seen = self.font_faces.iter().any(|f| {
                f.family == face.family && f.weight == face.weight && f.style == face.style
            });
            if !seen {
                self.font_faces.push(face);
            }
        }
        for (name, value) in rules.variables {
            if !self.variables.iter().any(|(n, _)| *n == name) {
                self.variables.push((name, value));
            }
        }
        self.breakpoints.extend(rules.breakpoints);
        self.breakpoints.sort_unstable();
        self.breakpoints.dedup();
        if self.focus_ring.is_none() {
            self.focus_ring = rules.focus_ring;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(family: &str, weight: &str) -> FontFace {
        FontFace {
            family: family.into(),
            weight: weight.into(),
            style: "normal".into(),
            src: vec![format!("https://x.dev/{family}-{weight}.woff2")],
        }
    }

    #[test]
    fn fixture_parses() {
        let s: Scan = serde_json::from_str(include_str!("fixtures/scan.json")).unwrap();
        assert_eq!(s.viewport, (1440, 900));
        assert_eq!(s.colors[5].on_button, 12);
        assert_eq!(s.colors[0].role, ColorRole::Text);
        assert_eq!(s.spacing[0], (4.0, 50));
        assert_eq!(s.components[0].style.height, 36.0);
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["colors"][1]["role"], "background");
        assert_eq!(back["spacing"][0], serde_json::json!([4.0, 50]));
    }

    /// What `browser_design.js`'s `scan` and `rules` emit, field for field.
    #[test]
    fn page_output_is_the_contract() {
        crate::site_design::assert_wire_exact::<Scan>(include_str!("fixtures/page_scan.json"));
        crate::site_design::assert_wire_exact::<SheetRules>(include_str!(
            "fixtures/page_rules.json"
        ));
    }

    #[test]
    fn merge_rules_dedupes() {
        let mut s = Scan {
            font_faces: vec![face("SF", "400")],
            variables: vec![("--x".into(), "page".into())],
            breakpoints: vec![1068, 734],
            ..Default::default()
        };
        s.merge_rules(SheetRules {
            font_faces: vec![face("SF", "400"), face("SF", "600")],
            variables: vec![("--x".into(), "sheet".into()), ("--y".into(), "1".into())],
            breakpoints: vec![734, 480],
            focus_ring: Some("rgb(0, 0, 255)".into()),
        });
        assert_eq!(s.font_faces.len(), 2);
        assert_eq!(
            s.variables,
            vec![("--x".into(), "page".into()), ("--y".into(), "1".into())]
        );
        assert_eq!(s.breakpoints, vec![480, 734, 1068]);
        assert_eq!(s.focus_ring.as_deref(), Some("rgb(0, 0, 255)"));

        s.merge_rules(SheetRules {
            focus_ring: Some("red".into()),
            ..Default::default()
        });
        assert_eq!(s.focus_ring.as_deref(), Some("rgb(0, 0, 255)"));
    }
}
