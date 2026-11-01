//! The coding agents running in the terminal, as the tab strip and the pane headers show them:
//! each agent's logo in place of the terminal glyph, and for a tab, every distinct agent of its
//! panes side by side.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::terminal::{AgentKind, names_phrase};

/// The logos' size: the tab strip's and pane headers' icons, `Icon::xsmall`.
pub const LOGO: Pixels = px(12.);
/// Past this many distinct agents a tab shows the first two and how many more.
const MOST_SHOWN: usize = 3;

/// Rasterised logos by logo name and device pixels; `None` for one that couldn't be made.
type Rasters = HashMap<(&'static str, u32), Option<Arc<RenderImage>>>;

thread_local! {
    /// Colour logos rasterised at the device size they are drawn at. An `img` of an SVG is
    /// rasterised at the SVG's own size (Factory's is 508 px), which scaled down to 12 px on
    /// the GPU loses its edges; one made at the drawn size stays crisp.
    static RASTERS: RefCell<Rasters> = RefCell::new(HashMap::new());
}

/// `name`'s logo from `assets/agent_logos`, rasterised `device` pixels square.
fn raster(name: &'static str, device: u32, cx: &App) -> Option<Arc<RenderImage>> {
    RASTERS.with(|cache| {
        cache
            .borrow_mut()
            .entry((name, device))
            .or_insert_with(|| {
                let path = format!("agent_logos/{name}.svg");
                let bytes = cx.asset_source().load(&path).ok().flatten()?;
                let renderer = cx.svg_renderer();
                let svg = renderer.parse_svg(&bytes).ok()?;
                let side = DevicePixels(device as i32);
                renderer
                    .render_parsed(&svg, SvgSize::ExactSize(size(side, side)))
                    .ok()
            })
            .clone()
    })
}

/// `kind`'s logo, `side` square. A colour logo keeps its colours, which read on the dark and
/// the light themes alike; a one-colour logo takes the text's colour. An agent with no logo
/// gets the generic agent glyph.
pub fn logo(kind: AgentKind, side: Pixels, window: &Window, cx: &App) -> AnyElement {
    draw(kind, side, None, window, cx)
}

/// [`logo`] on a surface of its own, a primary button's say: a one-colour logo, and the glyph
/// of an agent with none, take `tint`, the surface's text colour.
pub fn logo_in(kind: AgentKind, side: Pixels, tint: Hsla, window: &Window, cx: &App) -> AnyElement {
    draw(kind, side, Some(tint), window, cx)
}

/// [`logo`], its one colour `tint` when given, else the text's.
fn draw(
    kind: AgentKind,
    side: Pixels,
    tint: Option<Hsla>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let Some(name) = kind.logo() else {
        return Icon::new(Lucide::Bot)
            .size(side)
            .flex_none()
            .when_some(tint, |icon, tint| icon.text_color(tint))
            .into_any_element();
    };
    if name.ends_with("-color") {
        let device = (f32::from(side) * window.scale_factor()).ceil() as u32;
        if let Some(image) = raster(name, device, cx) {
            return img(ImageSource::Render(image))
                .size(side)
                .flex_none()
                .into_any_element();
        }
    }
    svg()
        .path(SharedString::from(format!("agent_logos/{name}.svg")))
        .size(side)
        .flex_none()
        .text_color(tint.unwrap_or(cx.theme().foreground))
        .into_any_element()
}

/// `kinds` with each agent once, where it first comes.
pub fn distinct(kinds: impl IntoIterator<Item = AgentKind>) -> Vec<AgentKind> {
    let mut seen = Vec::new();
    for kind in kinds {
        if !seen.contains(&kind) {
            seen.push(kind);
        }
    }
    seen
}

/// What a tab's icon shows for the agents running in its panes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    /// The logos, in the panes' reading order.
    pub shown: Vec<AgentKind>,
    /// How many more agents run, past those shown: "+2".
    pub more: usize,
}

/// The badge for `kinds`, the agents of a tab's panes in reading order: each distinct agent
/// once, all of them up to three, else the first two and how many more.
pub fn badge(kinds: impl IntoIterator<Item = AgentKind>) -> Badge {
    let mut shown = distinct(kinds);
    let mut more = 0;
    if shown.len() > MOST_SHOWN {
        more = shown.len() - (MOST_SHOWN - 1);
        shown.truncate(MOST_SHOWN - 1);
    }
    Badge { shown, more }
}

/// "Claude Code and Codex running": what runs, for the accessible name and the tooltip.
pub fn running_phrase(kinds: &[AgentKind]) -> String {
    format!("{} running", names_phrase(&distinct(kinds.iter().copied())))
}

/// A tooltip of `lines`: the first as it is, the rest muted under it.
pub fn tooltip(lines: Vec<String>, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, cx| {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_0p5()
            .children(lines.iter().enumerate().map(|(n, line)| {
                div()
                    .when(n > 0, |el| el.text_color(muted))
                    .child(line.clone())
            }))
    })
    .build(window, cx)
}

/// The logos of `badge` side by side, then "+N" when more run, 12 px high like the glyph
/// they stand in for. Tests find each logo as `{selector}-{agent id}`, and the count as
/// `{selector}-more`.
pub fn logos(badge: &Badge, selector: &str, window: &Window, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .gap(px(2.))
        .children(badge.shown.iter().map(|kind| {
            let name = format!("{selector}-{}", kind.id());
            div()
                .flex_none()
                .debug_selector(move || name.clone())
                .child(logo(*kind, LOGO, window, cx))
        }))
        .when(badge.more > 0, |el| {
            let name = format!("{selector}-more");
            el.child(
                div()
                    .debug_selector(move || name.clone())
                    .text_xs()
                    .line_height(LOGO)
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("+{}", badge.more)),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{Badge, badge, running_phrase};
    use sawit_core::terminal::AgentKind::*;

    #[test]
    fn a_badge_shows_each_agent_once_in_order() {
        assert_eq!(
            badge([ClaudeCode]),
            Badge {
                shown: vec![ClaudeCode],
                more: 0
            }
        );
        assert_eq!(
            badge([Codex, ClaudeCode, Codex]),
            Badge {
                shown: vec![Codex, ClaudeCode],
                more: 0
            }
        );
        assert_eq!(
            badge([ClaudeCode, Codex, GeminiCli, ClaudeCode]),
            Badge {
                shown: vec![ClaudeCode, Codex, GeminiCli],
                more: 0
            }
        );
    }

    #[test]
    fn past_three_agents_a_badge_shows_two_and_how_many_more() {
        assert_eq!(
            badge([ClaudeCode, Codex, GeminiCli, Opencode]),
            Badge {
                shown: vec![ClaudeCode, Codex],
                more: 2
            }
        );
        assert_eq!(
            badge([ClaudeCode, Codex, ClaudeCode, GeminiCli, Opencode, Amp]),
            Badge {
                shown: vec![ClaudeCode, Codex],
                more: 3
            }
        );
    }

    #[test]
    fn the_phrase_names_each_agent_once() {
        assert_eq!(running_phrase(&[ClaudeCode]), "Claude Code running");
        assert_eq!(
            running_phrase(&[ClaudeCode, Codex, ClaudeCode]),
            "Claude Code and Codex running"
        );
    }
}
