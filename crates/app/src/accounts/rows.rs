//! The pieces the page and the chip share: a usage bar with its reset time, and the words for a
//! window's state (no data, reset, stale, fresh). Nothing is estimated: a value is what Claude
//! Code reported, with when.

use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::agent_accounts::usage::{Snapshot, Window};
use sawit_core::github::ago;

/// A value older than this is "stale".
const STALE_SECS: f64 = 15.0 * 60.0;

/// `23%`.
pub fn pct(w: &Window) -> String {
    format!("{}%", w.used_percentage.round())
}

/// The window if it hasn't reset yet.
pub fn current(w: Option<Window>, now: f64) -> Option<Window> {
    w.filter(|w| w.resets_at > now)
}

/// "5h 23% · 7d 41%", or nothing to show.
pub fn short(s: Option<&Snapshot>, now: f64) -> Option<String> {
    let s = s?;
    let parts: Vec<String> = [("5h", s.five_hour), ("7d", s.seven_day)]
        .into_iter()
        .filter_map(|(l, w)| current(w, now).map(|w| format!("{l} {}", pct(&w))))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// "resets in 2h 13m".
pub fn resets_in(w: &Window, now: f64) -> String {
    let secs = (w.resets_at - now).max(0.0) as i64;
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    match (d, h) {
        (0, 0) => format!("resets in {m}m"),
        (0, _) => format!("resets in {h}h {m}m"),
        _ => format!("resets in {d}d {h}h"),
    }
}

/// How old the snapshot is, and whether that is stale.
pub fn observed(s: &Snapshot, now: f64) -> Option<(String, bool)> {
    let at = s.observed_at?;
    Some((
        format!("as of {}", ago(at as i64, now as i64)),
        now - at > STALE_SECS,
    ))
}

/// The bar's colour: the accent below 70%, orange to 89%, red from 90%.
pub fn bar_color(used: f64, cx: &App) -> Hsla {
    let theme = cx.theme();
    match used.round() as i64 {
        ..70 => theme.primary,
        70..90 => theme.warning,
        _ => theme.danger,
    }
}

/// One window as a labelled bar: "5-Hour · 23% · resets in 2h 13m", or why there's no value.
pub fn window_bar(
    label: &str,
    w: Option<Window>,
    stale: bool,
    now: f64,
    cx: &App,
) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let (text, fill, color) = match w {
        Some(w) if w.resets_at > now => (
            format!("{label} · {} · {}", pct(&w), resets_in(&w, now)),
            (w.used_percentage / 100.0).clamp(0.0, 1.0) as f32,
            bar_color(w.used_percentage, cx),
        ),
        Some(_) => (
            format!("{label} · window reset · no newer data"),
            0.0,
            muted,
        ),
        None => (format!("{label} · no data yet"), 0.0, muted),
    };
    v_flex()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(if stale { muted } else { cx.theme().foreground })
                .child(text),
        )
        .child(
            div()
                .h(px(6.))
                .w_full()
                .rounded_full()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .w(relative(fill))
                        .bg(color)
                        .opacity(if stale { 0.5 } else { 1.0 }),
                ),
        )
}

/// A round swatch in the account's colour.
pub fn swatch(hex: &str, size: f32) -> impl IntoElement {
    let rgb = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xD97757);
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(gpui_kit::rgb(rgb))
}
