//! The Inspect segment: the hovered (or pinned) element's box model, styles, states and
//! contrast, with copy buttons.

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::site_design::color;
use sawit_core::site_design::inspect::{StyleInfo, contrast, css_rule};
use sawit_core::site_design::tailwind::classes;

use super::colors::rgba_u32;
use super::{DesignEvent, DesignPanel};

const HINT: &str = "Hover an element on the page to read its styles. Click to pin it.";

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

/// A nested box of the diagram: the four sides' numbers around `inner`.
fn layer(
    title: &'static str,
    v: [f64; 4],
    tint: Hsla,
    muted: Hsla,
    inner: impl IntoElement,
) -> Div {
    let n = move |x: f64| div().text_xs().text_color(muted).child(num(x));
    v_flex()
        .p_1()
        .gap_0p5()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_dashed()
        .border_color(muted)
        .bg(tint)
        .child(div().w_full().text_xs().text_color(muted).child(title))
        .child(n(v[0]))
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .justify_between()
                .child(n(v[3]))
                .child(inner)
                .child(n(v[1])),
        )
        .child(n(v[2]))
}

fn box_model(info: &StyleInfo, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let content = div()
        .px_2()
        .py_0p5()
        .rounded(px(3.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_xs()
        .child(format!(
            "{} × {}",
            num(info.rect.width),
            num(info.rect.height)
        ));
    let b = &info.box_model;
    let tint = |a: f32| theme.accent.opacity(a);
    layer(
        "margin",
        b.margin,
        tint(0.3),
        muted,
        layer(
            "border",
            b.border,
            tint(0.6),
            muted,
            layer("padding", b.padding, tint(0.9), muted, content),
        ),
    )
}

fn section(title: &'static str, muted: Hsla, rows: Vec<AnyElement>) -> Option<AnyElement> {
    (!rows.is_empty()).then(|| {
        v_flex()
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(muted)
                    .child(title),
            )
            .children(rows)
            .into_any_element()
    })
}

fn prop(name: &'static str, value: impl Into<SharedString>, cx: &App) -> AnyElement {
    h_flex()
        .gap_2()
        .items_start()
        .text_xs()
        .child(
            div()
                .w(px(84.))
                .flex_none()
                .text_color(cx.theme().muted_foreground)
                .child(name),
        )
        .child(div().flex_1().min_w_0().child(value.into()))
        .into_any_element()
}

fn opt(name: &'static str, value: &Option<String>, cx: &App) -> Option<AnyElement> {
    value.as_ref().map(|v| prop(name, v.clone(), cx))
}

/// A row whose value is a color: a swatch before its text.
fn color_prop(name: &'static str, value: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let swatch = color::parse(value);
    h_flex()
        .gap_2()
        .items_center()
        .text_xs()
        .child(
            div()
                .w(px(84.))
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(name),
        )
        .when_some(swatch, |el, c| {
            el.child(
                div()
                    .size_3()
                    .flex_none()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(gpui_kit::rgba(rgba_u32(&c))),
            )
        })
        .child(div().flex_1().min_w_0().child(value.to_string()))
        .into_any_element()
}

fn badge(label: &'static str, pass: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (bg, fg) = if pass {
        (theme.success, theme.success_foreground)
    } else {
        (theme.danger, theme.danger_foreground)
    };
    div()
        .px_1p5()
        .rounded(px(3.))
        .bg(bg)
        .text_color(fg)
        .text_xs()
        .child(format!("{label} {}", if pass { "pass" } else { "fail" }))
}

pub(super) fn render(
    info: Option<&StyleInfo>,
    pinned: bool,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let Some(info) = info else {
        return div()
            .text_sm()
            .text_color(muted)
            .child(HINT)
            .into_any_element();
    };
    let t = &info.typography;
    let selector = info.selector.clone();
    let html_selector = info.selector.clone();
    let css = css_rule(info);
    let tailwind = classes(info).join(" ");

    let mut typography = vec![
        prop(
            "Family",
            t.used_family.clone().unwrap_or_else(|| t.family.clone()),
            cx,
        ),
        prop("Size", format!("{}px", num(t.size)), cx),
        prop("Weight", t.weight.to_string(), cx),
    ];
    if let Some(lh) = t.line_height {
        typography.push(prop("Line height", format!("{}px", num(lh)), cx));
    }
    if let Some(ls) = t.letter_spacing {
        typography.push(prop("Letter spacing", format!("{}px", num(ls)), cx));
    }
    typography.push(color_prop("Color", &t.color, cx));
    typography.push(prop("Align", t.align.clone(), cx));
    if t.transform != "none" {
        typography.push(prop("Transform", t.transform.clone(), cx));
    }

    let mut background = vec![color_prop("Color", &info.background.color, cx)];
    background.extend(opt("Image", &info.background.image, cx));

    let b = &info.border;
    let mut border = vec![
        prop("Width", sides(&b.width), cx),
        prop("Style", b.style.clone(), cx),
        color_prop("Color", &b.color, cx),
        prop("Radius", b.radius.clone(), cx),
    ];
    border.retain(|_| b.width.iter().any(|w| *w > 0.) || !b.radius.is_empty());

    let e = &info.effects;
    let mut effects: Vec<AnyElement> = Vec::new();
    effects.extend(opt("Shadow", &e.shadow, cx));
    if e.opacity < 1. {
        effects.push(prop("Opacity", num(e.opacity), cx));
    }
    effects.extend(opt("Transition", &e.transition, cx));

    let l = &info.layout;
    let mut layout = vec![
        prop("Display", l.display.clone(), cx),
        prop("Position", l.position.clone(), cx),
    ];
    layout.extend(opt("Gap", &l.gap, cx));
    layout.extend(opt("Direction", &l.flex_direction, cx));
    layout.extend(opt("Justify", &l.justify, cx));
    layout.extend(opt("Align items", &l.align, cx));
    layout.extend(opt("Columns", &l.grid_template_columns, cx));

    let states: Vec<AnyElement> = info
        .states
        .iter()
        .map(|(pseudo, decls)| {
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(pseudo.clone()),
                )
                .children(decls.iter().map(|(p, v)| {
                    div()
                        .pl_2()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{p}: {v}"))
                }))
                .into_any_element()
        })
        .collect();

    let contrast_row = contrast(info).map(|c| {
        h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .text_xs()
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(ratio_text(c.ratio)),
            )
            .child(badge("AA", c.aa, cx))
            .child(badge("AAA", c.aaa, cx))
            .child(div().text_color(muted).child(if c.large {
                "large text"
            } else {
                "normal text"
            }))
            .into_any_element()
    });

    let theme = cx.theme();
    let sections: Vec<AnyElement> = [
        section("Typography", muted, typography),
        section("Contrast", muted, contrast_row.into_iter().collect()),
        section("Background", muted, background),
        section("Border", muted, border),
        section("Effects", muted, effects),
        section("Layout", muted, layout),
        section("States", muted, states),
    ]
    .into_iter()
    .flatten()
    .collect();

    v_flex()
        .gap_3()
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .id("design-inspect-label")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .font_family(theme.mono_font_family.clone())
                        .cursor_pointer()
                        .child(info.label.clone())
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(DesignEvent::Flash(selector.clone()));
                        })),
                )
                .when(pinned, |el| {
                    el.child(
                        Button::new("design-unpin")
                            .xsmall()
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
                .child(
                    Button::new("design-copy-css")
                        .xsmall()
                        .label("CSS")
                        .tooltip("Copy as a CSS rule")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.copy(css.clone(), cx);
                        })),
                )
                .child(
                    Button::new("design-copy-tailwind")
                        .xsmall()
                        .label("Tailwind")
                        .tooltip("Copy as Tailwind classes")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.copy(tailwind.clone(), cx);
                        })),
                )
                .child(
                    Button::new("design-copy-html")
                        .xsmall()
                        .label("HTML")
                        .tooltip("Copy the markup with computed styles inlined")
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(DesignEvent::CopyHtml(html_selector.clone()));
                        })),
                ),
        )
        .child(box_model(info, cx))
        .children(sections)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{num, ratio_text, sides};

    #[test]
    fn numbers_and_sides_read_short() {
        assert_eq!(num(12.0), "12");
        assert_eq!(num(12.5), "12.5");
        assert_eq!(sides(&[1., 1., 1., 1.]), "1px");
        assert_eq!(sides(&[1., 2., 3., 4.]), "1 2 3 4 px");
        assert_eq!(ratio_text(4.5), "4.50:1");
        assert_eq!(ratio_text(4.499), "4.49:1");
    }
}
