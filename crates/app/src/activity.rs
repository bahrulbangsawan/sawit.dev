//! Activity icons for the terminal, browser and canvas tabs: one shape per state
//! ([`sawit_core::activity::Activity`]), so the state never rests on colour or motion alone.
//!
//! - busy: a circle loader that turns while work goes on (still under Reduce Motion, which GPUI's
//!   animations honour by drawing their first frame and asking for no more);
//! - succeeded: a circle with a check, in the success colour;
//! - failed: a circle with a cross, in the danger colour;
//! - stopped: a circle with a square, muted;
//! - needs attention: a bell with a dot, in the warning colour;
//! - idle: the tab's own glyph.

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::activity::Activity;

/// One turn of the busy loader: slow enough to stay calm beside text.
const TURN: Duration = Duration::from_millis(1200);
/// The loader turns at most this often: smooth at 12 px, and cheap with many tabs busy.
const FPS: f32 = 30.;

/// The glyph of each state but idle (the tab keeps its own then).
pub fn glyph_of(activity: Activity) -> Option<Lucide> {
    match activity {
        Activity::Idle => None,
        Activity::Busy => Some(Lucide::LoaderCircle),
        Activity::Succeeded => Some(Lucide::CircleCheck),
        Activity::Failed { .. } => Some(Lucide::CircleX),
        Activity::Stopped => Some(Lucide::CircleStop),
        Activity::Attention => Some(Lucide::BellDot),
    }
}

/// The glyph `activity` takes, with its colour; `None` for idle (the tab keeps its own). Busy
/// keeps the text's colour: its turning shape says it.
pub fn glyph(activity: Activity, cx: &App) -> Option<(Lucide, Option<Hsla>)> {
    let theme = cx.theme();
    let color = match activity {
        Activity::Idle | Activity::Busy => None,
        Activity::Succeeded => Some(theme.success),
        Activity::Failed { .. } => Some(theme.danger),
        Activity::Stopped => Some(theme.muted_foreground),
        Activity::Attention => Some(theme.warning),
    };
    glyph_of(activity).map(|g| (g, color))
}

/// The tab's icon for `activity`: its own glyph (`idle`) while idle, else the state's, `size`
/// square, turning while busy. `id` keeps each tab's turn its own.
pub fn icon(
    activity: Activity,
    idle: AnyElement,
    size: Pixels,
    id: impl Into<ElementId>,
    cx: &App,
) -> AnyElement {
    let Some((name, color)) = glyph(activity, cx) else {
        return idle;
    };
    let icon = Icon::new(name).size(size).map(|i| match color {
        Some(c) => i.text_color(c),
        None => i,
    });
    if activity.animates() {
        icon.with_animation(
            id,
            Animation::new(TURN).repeat().with_max_fps(FPS),
            |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
    } else {
        icon.into_any_element()
    }
}

/// [`icon`] at the tab strips' size (`Icon::xsmall`, 12 px).
pub fn tab_icon(
    activity: Activity,
    idle: AnyElement,
    id: impl Into<ElementId>,
    cx: &App,
) -> AnyElement {
    icon(activity, idle, px(12.), id, cx)
}

/// A small badge over an agent's logo (which keeps its own look): the state's glyph on a disc in
/// the tab's colour (`ring`), at the logo's bottom right. `None` while idle.
pub fn badge(
    activity: Activity,
    ring: Hsla,
    id: impl Into<ElementId>,
    cx: &App,
) -> Option<AnyElement> {
    glyph(activity, cx)?;
    Some(
        div()
            .absolute()
            .right(px(-4.))
            .bottom(px(-4.))
            .size(px(10.))
            .rounded_full()
            .bg(ring)
            .flex()
            .items_center()
            .justify_center()
            .child(icon(activity, div().into_any_element(), px(9.), id, cx))
            .into_any_element(),
    )
}

/// What the tab's accessible description says of its state: `None` while idle.
pub fn description(activity: Activity) -> Option<SharedString> {
    (activity != Activity::Idle).then(|| activity.label().into())
}

#[cfg(test)]
mod tests {
    use super::{Activity, description, glyph_of};
    use gpui_kit::SharedString;

    #[test]
    fn every_state_but_idle_has_its_own_glyph() {
        let states = [
            Activity::Busy,
            Activity::Succeeded,
            Activity::Failed { code: Some(1) },
            Activity::Stopped,
            Activity::Attention,
        ];
        let mut paths: Vec<SharedString> = states
            .iter()
            .map(|s| glyph_of(*s).expect("a glyph").path())
            .collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), states.len(), "two states share a glyph");
        assert!(glyph_of(Activity::Idle).is_none());
        assert!(description(Activity::Idle).is_none());
        assert_eq!(
            description(Activity::Failed { code: Some(2) }).as_deref(),
            Some("Exited with code 2")
        );
    }
}
