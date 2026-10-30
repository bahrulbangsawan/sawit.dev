//! The Fonts segment: each family the page actually uses, with its weights, sizes and
//! line-heights, where it comes from, and a sample line.

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::site_design::assets::font_assets;
use sawit_core::site_design::scan::{AssetRef, FontFace, Scan};
use sawit_core::site_design::tokens::Tokens;

use super::{DesignEvent, DesignPanel};

const LICENSE: &str = "Fonts belong to their foundries. Check the license before shipping.";
const SAMPLE: &str = "The quick brown fox jumps over the lazy dog";

/// One family in use.
#[derive(Debug, PartialEq)]
pub(super) struct FontGroup {
    /// The face actually used.
    pub family: String,
    /// The computed `font-family` stack it was seen in.
    pub stack: String,
    pub weights: Vec<u16>,
    pub sizes: Vec<f64>,
    pub line_heights: Vec<f64>,
    /// `@font-face` files; empty for a system font.
    pub files: Vec<AssetRef>,
}

fn unquote(name: &str) -> &str {
    name.trim().trim_matches(['"', '\'']).trim()
}

fn first_family(stack: &str) -> &str {
    unquote(stack.split(',').next().unwrap_or(stack))
}

fn insert_sorted<T: PartialOrd + Copy>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}

/// The families in use, the one with the most text first.
pub(super) fn group_fonts(scan: &Scan) -> Vec<FontGroup> {
    let mut groups: Vec<(FontGroup, u32)> = Vec::new();
    for t in &scan.texts {
        let name = t
            .used_family
            .as_deref()
            .map_or_else(|| first_family(&t.family), unquote)
            .to_string();
        if name.is_empty() {
            continue;
        }
        let at = match groups
            .iter()
            .position(|(g, _)| g.family.eq_ignore_ascii_case(&name))
        {
            Some(at) => at,
            None => {
                let faces: Vec<FontFace> = scan
                    .font_faces
                    .iter()
                    .filter(|f| unquote(&f.family).eq_ignore_ascii_case(&name))
                    .cloned()
                    .collect();
                groups.push((
                    FontGroup {
                        family: name,
                        stack: t.family.clone(),
                        weights: Vec::new(),
                        sizes: Vec::new(),
                        line_heights: Vec::new(),
                        files: font_assets(&faces),
                    },
                    0,
                ));
                groups.len() - 1
            }
        };
        let (g, chars) = &mut groups[at];
        *chars += t.chars;
        insert_sorted(&mut g.weights, t.weight);
        insert_sorted(&mut g.sizes, t.size);
        if let Some(lh) = t.line_height {
            insert_sorted(&mut g.line_heights, lh);
        }
    }
    groups.sort_by_key(|(_, chars)| std::cmp::Reverse(*chars));
    groups.into_iter().map(|(g, _)| g).collect()
}

/// `font-family: "Inter", sans-serif;`
pub(super) fn css_line(g: &FontGroup) -> String {
    let stack = if g.stack.trim().is_empty() {
        format!("\"{}\"", g.family)
    } else {
        g.stack.clone()
    };
    format!("font-family: {stack};")
}

/// `font-['Inter_Tight']`
pub(super) fn tailwind_class(g: &FontGroup) -> String {
    format!("font-['{}']", g.family.replace(' ', "_"))
}

fn px_list(v: &[f64]) -> String {
    v.iter()
        .map(|n| format!("{}", (n * 100.).round() / 100.))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn render(
    scan: &Scan,
    tokens: &Tokens,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let groups = group_fonts(scan);
    let theme = cx.theme();
    let (muted, border) = (theme.muted_foreground, theme.border);
    let installed = cx.text_system().all_font_names();
    let mut cards: Vec<AnyElement> = Vec::new();
    for (ix, g) in groups.iter().enumerate() {
        let roles: Vec<&str> = tokens
            .families
            .iter()
            .filter(|(_, fam)| first_family(fam).eq_ignore_ascii_case(&g.family))
            .map(|(role, _)| role.as_str())
            .collect();
        let source = if g.files.is_empty() {
            "System".to_string()
        } else {
            let n = g.files.len();
            format!("Web font, {n} file{}", if n == 1 { "" } else { "s" })
        };
        let has_it = installed.iter().any(|n| n.eq_ignore_ascii_case(&g.family));
        let css = css_line(g);
        let tw = tailwind_class(g);
        let files = g.files.clone();
        let detail = |label: &'static str, value: String| {
            h_flex()
                .gap_2()
                .items_start()
                .text_xs()
                .child(div().w(px(64.)).flex_none().text_color(muted).child(label))
                .child(div().flex_1().min_w_0().child(value))
        };
        cards.push(
            v_flex()
                .gap_1p5()
                .p_2()
                .rounded(theme.radius)
                .border_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_baseline()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(g.family.clone()),
                        )
                        .when(!roles.is_empty(), |el| {
                            el.child(div().text_xs().text_color(muted).child(roles.join(", ")))
                        }),
                )
                .child(
                    div()
                        .text_base()
                        .truncate()
                        .when(has_it, |el| el.font_family(g.family.clone()))
                        .when(!has_it, |el| el.text_color(muted))
                        .child(if has_it {
                            SAMPLE.to_string()
                        } else {
                            g.family.clone()
                        }),
                )
                .child(detail("Source", source))
                .child(detail(
                    "Weights",
                    g.weights
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                ))
                .child(detail("Sizes", format!("{} px", px_list(&g.sizes))))
                .when(!g.line_heights.is_empty(), |el| {
                    el.child(detail(
                        "Line height",
                        format!("{} px", px_list(&g.line_heights)),
                    ))
                })
                .child(
                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .child(
                            Button::new(("design-font-css", ix))
                                .xsmall()
                                .label("font-family")
                                .tooltip("Copy the CSS line")
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.copy(css.clone(), cx);
                                })),
                        )
                        .child(
                            Button::new(("design-font-tw", ix))
                                .xsmall()
                                .label("Tailwind")
                                .tooltip("Copy the font-[…] class")
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.copy(tw.clone(), cx);
                                })),
                        )
                        .child(
                            Button::new(("design-font-save", ix))
                                .xsmall()
                                .label("Save files")
                                .disabled(files.is_empty())
                                .tooltip("Download this family's font files")
                                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                    cx.emit(DesignEvent::SaveAssets(files.clone()));
                                })),
                        ),
                )
                .into_any_element(),
        );
    }
    v_flex()
        .gap_3()
        .when(cards.is_empty(), |el| {
            el.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("No text found on this page."),
            )
        })
        .children(cards)
        .child(div().text_xs().text_color(muted).child(LICENSE))
}

#[cfg(test)]
mod tests {
    use super::{css_line, group_fonts, tailwind_class};
    use sawit_core::site_design::scan::{FontFace, Scan, TextSample};

    fn text(stack: &str, used: Option<&str>, size: f64, weight: u16, chars: u32) -> TextSample {
        TextSample {
            tag: "p".into(),
            in_main: true,
            family: stack.into(),
            used_family: used.map(Into::into),
            size,
            weight,
            line_height: Some(size * 1.5),
            letter_spacing: None,
            color: "#000".into(),
            chars,
        }
    }

    #[test]
    fn families_group_by_the_face_used() {
        let scan = Scan {
            texts: vec![
                text("Arial", Some("Arial"), 14., 400, 10),
                text("\"Inter\", sans-serif", Some("Inter"), 16., 700, 50),
                text("\"Inter\", sans-serif", Some("Inter"), 16., 400, 30),
                text("\"Inter\", sans-serif", Some("Inter"), 12., 400, 5),
            ],
            font_faces: vec![FontFace {
                family: "\"Inter\"".into(),
                weight: "400".into(),
                style: "normal".into(),
                src: vec!["https://a.dev/i.woff2".into()],
            }],
            ..Scan::default()
        };
        let groups = group_fonts(&scan);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].family, "Inter");
        assert_eq!(groups[0].weights, [400, 700]);
        assert_eq!(groups[0].sizes, [12., 16.]);
        assert_eq!(groups[0].files.len(), 1);
        assert!(groups[1].files.is_empty());
        assert_eq!(css_line(&groups[0]), "font-family: \"Inter\", sans-serif;");
        assert_eq!(tailwind_class(&groups[0]), "font-['Inter']");
    }
}
