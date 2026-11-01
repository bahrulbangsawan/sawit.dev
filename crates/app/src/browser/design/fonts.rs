//! The Fonts tab: each family the page actually uses, named once in its own face, with its
//! weights, a compact size scale (the raw values behind a disclosure), where it comes from,
//! and buttons that copy it or save its files.

use std::collections::HashSet;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::site_design::assets::font_assets;
use sawit_core::site_design::scan::{AssetRef, FontFace, Scan};
use sawit_core::site_design::tokens::Tokens;

use super::{DesignEvent, DesignPanel};
use crate::browser::panel;

const LICENSE: &str = "Fonts belong to their foundries. Check the license before shipping.";
const SPECIMEN: &str = "The quick brown fox jumps over the lazy dog 0123456789";

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
    format!("font-family: {};", stack(g))
}

fn stack(g: &FontGroup) -> String {
    if g.stack.trim().is_empty() {
        format!("\"{}\"", g.family)
    } else {
        g.stack.clone()
    }
}

/// The Tailwind v4 theme line for the family: `--font-sans: "Inter", sans-serif;`, named by
/// its role in the tokens (`sans`, `serif`, `mono`, `display`), else after the family.
pub(super) fn tailwind_line(g: &FontGroup, role: Option<&str>) -> String {
    let name = role.map(str::to_string).unwrap_or_else(|| {
        g.family
            .to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    });
    format!("--font-{name}: {};", stack(g))
}

/// Values rounded to whole pixels, each once: the scale a reader sees, without the
/// sub-pixel steps fluid type leaves behind (`15, 16.8, 18.85` → `15 · 17 · 19`).
pub(super) fn scale(values: &[f64]) -> Vec<u32> {
    let mut out: Vec<u32> = values
        .iter()
        .filter(|v| v.is_finite() && **v > 0.)
        .map(|v| v.round() as u32)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// `15 · 17 · 19 px`.
fn scale_text(values: &[u32]) -> String {
    let parts: Vec<String> = values.iter().map(u32::to_string).collect();
    format!("{} px", parts.join(" · "))
}

/// The raw values, as the page computed them.
fn px_list(v: &[f64]) -> String {
    v.iter()
        .map(|n| format!("{}", (n * 100.).round() / 100.))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The face GPUI draws `family` with for its specimen: the family itself when this Mac has
/// it, the app's own faces for the CSS names of the system's (`-apple-system`,
/// `ui-monospace`…), else none.
pub(super) fn face_for(
    family: &str,
    installed: &[String],
    ui: &str,
    mono: &str,
) -> Option<SharedString> {
    if let Some(name) = installed.iter().find(|n| n.eq_ignore_ascii_case(family)) {
        return Some(name.clone().into());
    }
    match family.to_ascii_lowercase().as_str() {
        "-apple-system" | "blinkmacsystemfont" | "system-ui" | "ui-sans-serif" | "ui-rounded" => {
            Some(SharedString::from(ui.to_string()))
        }
        "ui-monospace" | "sfmono-regular" => Some(SharedString::from(mono.to_string())),
        _ => None,
    }
}

/// A weight's name: `600 Semibold`.
pub(super) fn weight_name(w: u16) -> String {
    let name = match w {
        0..=149 => "Thin",
        150..=249 => "Extra light",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "Semibold",
        650..=749 => "Bold",
        750..=849 => "Extra bold",
        _ => "Black",
    };
    format!("{w} {name}")
}

pub(super) fn render(
    scan: &Scan,
    tokens: &Tokens,
    expanded: &HashSet<String>,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let groups = group_fonts(scan);
    let installed = cx.text_system().all_font_names();
    if groups.is_empty() {
        return panel::empty(
            "No text on this page",
            "Families, weights and sizes show here once the page sets text.",
            None,
            cx,
        )
        .into_any_element();
    }
    let mut cards: Vec<AnyElement> = Vec::new();
    for (ix, g) in groups.iter().enumerate() {
        let roles: Vec<String> = tokens
            .families
            .iter()
            .filter(|(_, fam)| first_family(fam).eq_ignore_ascii_case(&g.family))
            .map(|(role, _)| role.clone())
            .collect();
        cards.push(card(
            ix,
            g,
            &roles,
            &installed,
            expanded.contains(&g.family),
            cx,
        ));
    }
    v_flex()
        .gap_3()
        .children(cards)
        .child(
            h_flex()
                .gap_1p5()
                .items_start()
                .pt_1()
                .child(panel::glyph(Lucide::Info, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(LICENSE),
                ),
        )
        .into_any_element()
}

fn card(
    ix: usize,
    g: &FontGroup,
    roles: &[String],
    installed: &[String],
    open: bool,
    cx: &mut Context<DesignPanel>,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let face = face_for(
        &g.family,
        installed,
        &theme.font_family,
        &theme.mono_font_family,
    );
    let has_face = face.is_some();
    let face = face.unwrap_or_default();
    let source = if g.files.is_empty() {
        "System font".to_string()
    } else {
        let n = g.files.len();
        format!("Web font · {n} file{}", if n == 1 { "" } else { "s" })
    };
    let css = css_line(g);
    let tw = tailwind_line(g, roles.first().map(String::as_str));
    let files = g.files.clone();
    let family = g.family.clone();
    let sizes = scale(&g.sizes);
    let heights = scale(&g.line_heights);
    let raw_count = g.sizes.len() + g.line_heights.len();
    let rounded_away = g.sizes.len() > sizes.len() || g.line_heights.len() > heights.len();
    v_flex()
        .gap_3()
        .p_3()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        // The name once, in its own face when this Mac has it.
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .truncate()
                        .text_lg()
                        .when(has_face, |el| el.font_family(face.clone()))
                        .child(g.family.clone()),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .children(roles.iter().map(|r| panel::tag(r.clone(), cx)))
                        .child(div().text_xs().text_color(muted).child(source)),
                ),
        )
        .child(if has_face {
            div()
                .text_sm()
                .line_clamp(2)
                .font_family(face.clone())
                .child(SPECIMEN)
                .into_any_element()
        } else {
            div()
                .text_xs()
                .text_color(muted)
                .child("This Mac doesn't have this face, so no specimen is shown.")
                .into_any_element()
        })
        .child(panel::row(
            "Weights",
            h_flex()
                .gap_1()
                .flex_wrap()
                .children(g.weights.iter().map(|w| panel::tag(weight_name(*w), cx))),
            cx,
        ))
        .child(panel::row("Sizes", div().child(scale_text(&sizes)), cx))
        .when(!heights.is_empty(), |el| {
            el.child(panel::row(
                "Line heights",
                div().child(scale_text(&heights)),
                cx,
            ))
        })
        .when(rounded_away, |el| {
            let key = family.clone();
            el.child(
                v_flex()
                    .gap_1()
                    .child(
                        Button::new(("design-font-raw", ix))
                            .ghost()
                            .xsmall()
                            .icon(if open {
                                Lucide::ChevronDown
                            } else {
                                Lucide::ChevronRight
                            })
                            .label(if open {
                                "Hide the exact values".to_string()
                            } else {
                                format!("Show all {raw_count} exact values")
                            })
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if !this.expanded.remove(&key) {
                                    this.expanded.insert(key.clone());
                                }
                                cx.notify();
                            })),
                    )
                    .when(open, |el| {
                        el.child(panel::row(
                            "Sizes",
                            div().child(format!("{} px", px_list(&g.sizes))),
                            cx,
                        ))
                        .when(!g.line_heights.is_empty(), |el| {
                            el.child(panel::row(
                                "Line heights",
                                div().child(format!("{} px", px_list(&g.line_heights))),
                                cx,
                            ))
                        })
                    }),
            )
        })
        .child(
            h_flex()
                .gap_1()
                .flex_wrap()
                .child(
                    Button::new(("design-font-css", ix))
                        .outline()
                        .xsmall()
                        .icon(Lucide::Copy)
                        .label("Copy font-family")
                        .tooltip(css.clone())
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.copy(css.clone(), cx);
                        })),
                )
                .child(
                    Button::new(("design-font-tw", ix))
                        .outline()
                        .xsmall()
                        .icon(Lucide::Copy)
                        .label("Copy Tailwind")
                        .tooltip(format!("Copy the @theme line: {tw}"))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.copy(tw.clone(), cx);
                        })),
                )
                .child(
                    Button::new(("design-font-save", ix))
                        .outline()
                        .xsmall()
                        .icon(Lucide::Download)
                        .label("Save font files")
                        .disabled(files.is_empty())
                        .tooltip(if files.is_empty() {
                            "A system font: the page loads no files for it".to_string()
                        } else {
                            format!(
                                "Download its {} file{} to the export folder's assets/fonts",
                                files.len(),
                                if files.len() == 1 { "" } else { "s" }
                            )
                        })
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(DesignEvent::SaveAssets(files.clone()));
                        })),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{css_line, group_fonts, scale, tailwind_line, weight_name};
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
        assert_eq!(
            tailwind_line(&groups[0], Some("sans")),
            "--font-sans: \"Inter\", sans-serif;"
        );
        assert_eq!(tailwind_line(&groups[1], None), "--font-arial: Arial;");
    }

    /// Fluid type's sub-pixel sizes read as the whole pixels a reader sees, each once.
    #[test]
    fn sizes_round_into_a_compact_scale() {
        assert_eq!(
            scale(&[15., 16.8, 18.85, 16.4, 0., f64::NAN]),
            [15, 16, 17, 19]
        );
        assert!(scale(&[]).is_empty());
    }

    /// A specimen is drawn in the face itself, or in the app's own for the system's CSS
    /// names; a web font this Mac lacks gets none.
    #[test]
    fn specimens_use_a_face_this_mac_has() {
        let installed = vec!["Georgia".to_string(), "Menlo".to_string()];
        let face = |f: &str| super::face_for(f, &installed, ".SystemUIFont", "Menlo");
        assert_eq!(face("georgia").as_deref(), Some("Georgia"));
        assert_eq!(face("-apple-system").as_deref(), Some(".SystemUIFont"));
        assert_eq!(face("BlinkMacSystemFont").as_deref(), Some(".SystemUIFont"));
        assert_eq!(face("ui-monospace").as_deref(), Some("Menlo"));
        assert_eq!(face("Kebun Display"), None);
    }

    #[test]
    fn weights_have_names() {
        assert_eq!(weight_name(400), "400 Regular");
        assert_eq!(weight_name(600), "600 Semibold");
        assert_eq!(weight_name(900), "900 Black");
    }
}
