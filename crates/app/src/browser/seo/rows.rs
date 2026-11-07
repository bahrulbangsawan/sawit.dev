//! The SEO panel's rows: a check's line with its status icon, the scores, the counts, the page's
//! facts and a check's evidence.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::web::seo::{CheckResult, Evidence, Status};

use super::super::panel;

/// A status's icon and colour: never colour alone, each status has its own shape.
pub fn status_icon(status: Status, cx: &App) -> Icon {
    let theme = cx.theme();
    let (icon, color) = match status {
        Status::Pass => (Lucide::CircleCheck, theme.success),
        Status::Warn => (Lucide::TriangleAlert, theme.warning),
        Status::Fail => (Lucide::CircleX, theme.danger),
        Status::Na => (Lucide::CircleMinus, theme.muted_foreground),
        Status::Skipped => (Lucide::CircleDashed, theme.muted_foreground),
    };
    Icon::new(icon).small().text_color(color)
}

/// Problems first, then passes, then what didn't apply.
pub fn status_rank(status: Status) -> u8 {
    match status {
        Status::Fail => 0,
        Status::Warn => 1,
        Status::Pass => 2,
        Status::Na => 3,
        Status::Skipped => 4,
    }
}

/// `2/2`, `1.5/2`, `n/a`, `skipped`.
pub fn points(c: &CheckResult) -> String {
    if !c.status.scored() {
        return c.status.as_str().to_string();
    }
    let fmt = |v: f64| {
        if v.fract() == 0. {
            format!("{v:.0}")
        } else {
            format!("{v:.1}")
        }
    };
    format!("{}/{}", fmt(c.score), fmt(c.max))
}

/// A check's line: icon, name over its finding (one line, the whole of it in a tooltip), points,
/// and a chevron that says it opens.
pub fn check_line(c: &CheckResult, open: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let details: SharedString = c.details.clone().into();
    h_flex()
        .w_full()
        .gap_2()
        .items_start()
        .text_xs()
        .child(div().pt(px(1.)).child(status_icon(c.status, cx)))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .items_start()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(c.name.clone()),
                )
                .child(
                    div()
                        .w_full()
                        .text_color(theme.muted_foreground)
                        .when(!open, |d| d.truncate())
                        .child(details),
                ),
        )
        .child(
            div()
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(points(c)),
        )
        .child(
            Icon::new(if open {
                Lucide::ChevronDown
            } else {
                Lucide::ChevronRight
            })
            .xsmall()
            .text_color(theme.muted_foreground),
        )
}

/// The big numbers: each a value with its suffix over a label, side by side in one box.
pub fn scores(items: Vec<(SharedString, u32, SharedString)>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let count = items.len();
    h_flex()
        .w_full()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .children(
            items
                .into_iter()
                .enumerate()
                .map(|(i, (label, value, suffix))| {
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .px_3()
                        .py_2p5()
                        .gap_0p5()
                        .when(i + 1 < count, |d| d.border_r_1().border_color(theme.border))
                        .child(
                            h_flex()
                                .items_baseline()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_2xl()
                                        .line_height(relative(1.1))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.foreground)
                                        .child(value.to_string()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(suffix),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(label),
                        )
                }),
        )
}

/// "17 passed · 4 warnings · 3 failed", each with its status icon.
pub fn counts(passed: usize, warned: usize, failed: usize, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let one = |status: Status, n: usize, word: &str| {
        h_flex()
            .gap_1()
            .items_center()
            .child(status_icon(status, cx).xsmall())
            .child(format!("{n} {word}"))
    };
    h_flex()
        .gap_3()
        .flex_wrap()
        .text_xs()
        .text_color(muted)
        .child(one(Status::Pass, passed, "passed"))
        .child(one(
            Status::Warn,
            warned,
            if warned == 1 { "warning" } else { "warnings" },
        ))
        .child(one(Status::Fail, failed, "failed"))
}

/// A label/value row of the page's facts, the whole value in a tooltip; "Missing" when absent.
pub fn fact_row(label: &str, value: Option<String>, cx: &App) -> Option<AnyElement> {
    let theme = cx.theme();
    let id = SharedString::from(format!("seo-fact-{label}"));
    let el = match value {
        Some(v) => {
            let tip: SharedString = v.clone().into();
            div()
                .id(id)
                .truncate()
                .text_color(theme.foreground)
                .child(v)
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .into_any_element()
        }
        None => div()
            .text_color(theme.muted_foreground)
            .child("Missing")
            .into_any_element(),
    };
    Some(panel::row(label.to_string(), el, cx).into_any_element())
}

/// One piece of evidence: the address, its status, a note; the address whole in a tooltip.
pub fn evidence_line(e: &Evidence, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let status = if e.status == 0 {
        if e.note.is_empty() {
            String::new()
        } else {
            e.note.clone()
        }
    } else if e.note.is_empty() {
        e.status.to_string()
    } else {
        format!("{} · {}", e.status, e.note)
    };
    let tip: SharedString = format!("{} {}", e.url, status).into();
    h_flex()
        .id(SharedString::from(format!("seo-evidence-{}", e.url)))
        .w_full()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme.mono_font_family.clone())
                .text_color(theme.foreground)
                .child(e.url.clone()),
        )
        .child(
            div()
                .flex_none()
                .max_w(px(120.))
                .truncate()
                .text_color(theme.muted_foreground)
                .child(status),
        )
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
}
