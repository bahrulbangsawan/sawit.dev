//! The Inspect tab: the hovered (or pinned) element's box model, type, colours, layout,
//! border and effects, states and contrast. Every value copies on click.

mod box_model;
mod waiting;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::site_design::color;
use sawit_core::site_design::inspect::{StyleInfo, contrast, css_rule};
use sawit_core::site_design::tailwind::classes;

use self::box_model::box_model;
use self::waiting::waiting;
use super::rows::{copy_row, hex_and_alpha, swatch};
use super::{DesignEvent, DesignPanel};
use crate::browser::panel;

/// `12`, `12.5`: no trailing zeros.
fn num(n: f64) -> String {
    let s = format!("{:.2}", n);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// One value for four equal sides, else all four (top right bottom left).
fn sides(v: &[f64; 4]) -> String {
    if v.iter().all(|n| *n == v[0]) {
        format!("{}px", num(v[0]))
    } else {
        v.iter().map(|n| num(*n)).collect::<Vec<_>>().join(" ") + " px"
    }
}

/// The ratio as the row shows it: `4.52:1`.
fn ratio_text(ratio: f64) -> String {
    format!("{:.2}:1", (ratio * 100.).floor() / 100.)
}

/// A value with its colour's swatch: `■ #132643 60%`.
fn color_value(value: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    match color::parse(value) {
        Some(c) => {
            let (hex, alpha) = hex_and_alpha(&c);
            h_flex()
                .gap_1p5()
                .items_center()
                .child(swatch(&c, px(14.), cx))
                .child(div().font_family(theme.mono_font_family.clone()).child(hex))
                .children(alpha.map(|a| {
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("· {a}"))
                }))
                .into_any_element()
        }
        None => div().truncate().child(value.to_string()).into_any_element(),
    }
}

/// The copy text of a colour: its hex with alpha, as CSS reads it.
fn color_copy(value: &str) -> String {
    color::parse(value).map_or_else(|| value.to_string(), |c| c.to_hex())
}

/// A pale pass/fail tag for a contrast level.
fn level(label: &'static str, pass: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (bg, fg) = if pass {
        (theme.success.opacity(0.16), theme.success)
    } else {
        (theme.danger.opacity(0.16), theme.danger)
    };
    h_flex()
        .flex_none()
        .px_1p5()
        .h(px(18.))
        .gap_1()
        .items_center()
        .rounded(px(4.))
        .bg(bg)
        .text_xs()
        .text_color(fg)
        .child(if pass { "✓" } else { "✕" })
        .child(label)
}

pub(super) fn render(
    info: Option<&StyleInfo>,
    pinned: bool,
    last: Option<&StyleInfo>,
    inspecting: bool,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let Some(info) = info else {
        return waiting(last, inspecting, cx);
    };
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mono = theme.mono_font_family.clone();
    let t = &info.typography;
    let selector = info.selector.clone();
    let html_selector = info.selector.clone();
    let css = css_rule(info);
    let tailwind = classes(info).join(" ");
    let installed = cx.text_system().all_font_names();
    let family = t.used_family.clone().unwrap_or_else(|| t.family.clone());
    let face = super::fonts::face_for(
        &family,
        &installed,
        &cx.theme().font_family,
        &cx.theme().mono_font_family,
    );
    let has_face = face.is_some();
    let face = face.unwrap_or_default();

    // Header: what it is, and what can be copied of it.
    let header = v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .id("design-inspect-label")
                                .truncate()
                                .text_sm()
                                .font_family(mono.clone())
                                .cursor_pointer()
                                .child(info.label.clone())
                                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                    cx.emit(DesignEvent::Flash(selector.clone()));
                                })),
                        )
                        .child(div().text_xs().text_color(muted).child(if pinned {
                            format!(
                                "Pinned · {} × {}",
                                num(info.rect.width),
                                num(info.rect.height)
                            )
                        } else {
                            format!(
                                "Hovered · {} × {} · click to pin",
                                num(info.rect.width),
                                num(info.rect.height)
                            )
                        })),
                )
                .when(pinned, |el| {
                    el.child(
                        Button::new("design-unpin")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .label("Unpin")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.unpin(cx);
                            })),
                    )
                }),
        )
        .child(
            h_flex()
                .gap_1()
                .flex_wrap()
                .child(
                    Button::new("design-copy-css")
                        .outline()
                        .xsmall()
                        .icon(Lucide::Copy)
                        .label("CSS")
                        .tooltip("Copy as a CSS rule")
                        .accessibility_label("Copy as a CSS rule")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.copy(css.clone(), cx);
                        })),
                )
                .child(
                    Button::new("design-copy-tailwind")
                        .outline()
                        .xsmall()
                        .icon(Lucide::Copy)
                        .label("Tailwind")
                        .tooltip("Copy as Tailwind classes")
                        .accessibility_label("Copy as Tailwind classes")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.copy(tailwind.clone(), cx);
                        })),
                )
                .child(
                    Button::new("design-copy-html")
                        .outline()
                        .xsmall()
                        .icon(Lucide::Code)
                        .label("HTML")
                        .tooltip("Copy the markup with computed styles inlined")
                        .accessibility_label("Copy the markup with computed styles inlined")
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(DesignEvent::CopyHtml(html_selector.clone()));
                        })),
                ),
        );

    // Typography: a specimen in the face itself when this Mac has it.
    let mut type_rows: Vec<AnyElement> = Vec::new();
    type_rows.push(copy_row(
        "i-family",
        "Family",
        div()
            .truncate()
            .when(has_face, |el| el.font_family(face.clone()))
            .child(family.clone()),
        family.clone(),
        cx,
    ));
    let size = match t.line_height {
        Some(lh) => format!("{}px / {}px", num(t.size), num(lh)),
        None => format!("{}px / normal", num(t.size)),
    };
    type_rows.push(copy_row(
        "i-size",
        "Size / line",
        div().child(size.clone()),
        size,
        cx,
    ));
    type_rows.push(copy_row(
        "i-weight",
        "Weight",
        div().child(t.weight.to_string()),
        t.weight.to_string(),
        cx,
    ));
    if let Some(ls) = t.letter_spacing {
        type_rows.push(copy_row(
            "i-tracking",
            "Letter spacing",
            div().child(format!("{}px", num(ls))),
            format!("{}px", num(ls)),
            cx,
        ));
    }
    type_rows.push(copy_row(
        "i-align",
        "Align",
        div().child(t.align.clone()),
        t.align.clone(),
        cx,
    ));
    if t.transform != "none" {
        type_rows.push(copy_row(
            "i-transform",
            "Transform",
            div().child(t.transform.clone()),
            t.transform.clone(),
            cx,
        ));
    }

    // Colours, with the contrast of the text on what is behind it.
    let mut color_rows = vec![
        copy_row(
            "i-color",
            "Text",
            color_value(&t.color, cx),
            color_copy(&t.color),
            cx,
        ),
        copy_row(
            "i-bg",
            "Background",
            color_value(&info.background.color, cx),
            color_copy(&info.background.color),
            cx,
        ),
    ];
    if let Some(image) = &info.background.image {
        color_rows.push(copy_row(
            "i-bg-image",
            "Image",
            div().truncate().child(image.clone()),
            image.clone(),
            cx,
        ));
    }
    if let Some(c) = contrast(info) {
        let ratio = ratio_text(c.ratio);
        color_rows.push(copy_row(
            "i-contrast",
            "Contrast",
            h_flex()
                .gap_1p5()
                .items_center()
                .flex_wrap()
                .child(div().font_weight(FontWeight::MEDIUM).child(ratio.clone()))
                .child(level("AA", c.aa, cx))
                .child(level("AAA", c.aaa, cx))
                .child(div().text_color(muted).child(if c.large {
                    "large text"
                } else {
                    "body text"
                })),
            ratio,
            cx,
        ));
    }

    let l = &info.layout;
    let mut layout = vec![
        copy_row(
            "i-display",
            "Display",
            div().child(l.display.clone()),
            l.display.clone(),
            cx,
        ),
        copy_row(
            "i-position",
            "Position",
            div().child(l.position.clone()),
            l.position.clone(),
            cx,
        ),
    ];
    let optional = [
        ("i-gap", "Gap", &l.gap),
        ("i-direction", "Direction", &l.flex_direction),
        ("i-justify", "Justify", &l.justify),
        ("i-align-items", "Align items", &l.align),
        ("i-columns", "Columns", &l.grid_template_columns),
    ];
    for (id, name, value) in optional {
        if let Some(v) = value {
            layout.push(copy_row(
                id,
                name,
                div().truncate().child(v.clone()),
                v.clone(),
                cx,
            ));
        }
    }

    let b = &info.border;
    let mut border: Vec<AnyElement> = Vec::new();
    if b.width.iter().any(|w| *w > 0.) {
        let line = format!("{} {}", sides(&b.width), b.style);
        border.push(copy_row(
            "i-border",
            "Border",
            div().child(line.clone()),
            line,
            cx,
        ));
        border.push(copy_row(
            "i-border-color",
            "Border color",
            color_value(&b.color, cx),
            color_copy(&b.color),
            cx,
        ));
    }
    if !b.radius.is_empty() && b.radius != "0px" {
        border.push(copy_row(
            "i-radius",
            "Radius",
            div().child(b.radius.clone()),
            b.radius.clone(),
            cx,
        ));
    }
    let e = &info.effects;
    if let Some(shadow) = &e.shadow {
        border.push(copy_row(
            "i-shadow",
            "Shadow",
            div().truncate().child(shadow.clone()),
            shadow.clone(),
            cx,
        ));
    }
    if e.opacity < 1. {
        border.push(copy_row(
            "i-opacity",
            "Opacity",
            div().child(num(e.opacity)),
            num(e.opacity),
            cx,
        ));
    }
    if let Some(tr) = &e.transition {
        border.push(copy_row(
            "i-transition",
            "Transition",
            div().truncate().child(tr.clone()),
            tr.clone(),
            cx,
        ));
    }

    let states: Vec<AnyElement> = info
        .states
        .iter()
        .enumerate()
        .map(|(ix, (pseudo, decls))| {
            let text = decls
                .iter()
                .map(|(p, v)| format!("{p}: {v};"))
                .collect::<Vec<_>>()
                .join("\n");
            let shown = text.clone();
            copy_row(
                ("i-state", ix),
                pseudo.clone(),
                div()
                    .font_family(mono.clone())
                    .text_color(cx.theme().foreground)
                    .child(shown),
                text,
                cx,
            )
        })
        .collect();

    let section = |title: &'static str, rows: Vec<AnyElement>, cx: &App| {
        (!rows.is_empty()).then(|| panel::section(title, None, v_flex().children(rows), cx))
    };
    let theme = cx.theme();
    v_flex()
        .gap_4()
        .child(header)
        .child(panel::section("Box", None, box_model(info, cx), cx))
        .children(section("Typography", type_rows, cx))
        .children(section("Colors", color_rows, cx))
        .children(section("Layout", layout, cx))
        .children(section("Border and effects", border, cx))
        .children(section("States", states, cx))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("Click a value to copy it."),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{color_copy, num, ratio_text, sides};

    #[test]
    fn numbers_and_sides_read_short() {
        assert_eq!(num(12.0), "12");
        assert_eq!(num(12.5), "12.5");
        assert_eq!(sides(&[1., 1., 1., 1.]), "1px");
        assert_eq!(sides(&[1., 2., 3., 4.]), "1 2 3 4 px");
        assert_eq!(ratio_text(4.5), "4.50:1");
        assert_eq!(ratio_text(4.499), "4.49:1");
    }

    #[test]
    fn colours_copy_as_css_reads_them() {
        assert_eq!(color_copy("rgb(0, 113, 227)"), "#0071e3");
        assert_eq!(color_copy("rgba(19, 38, 67, 0.6)"), "#13264399");
        assert_eq!(color_copy("currentcolor"), "currentcolor");
    }
}
