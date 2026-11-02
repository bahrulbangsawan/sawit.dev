//! The Skills page's first card: Sawit Skills, where they stand, and the footer button's actions.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::sawit_skills::Level;
use sawit_core::skills::{self, SAWIT_PACK};

use super::button::{face, menu};
use super::{Job, run, try_status};
use crate::catalog::{self, Mark};

/// The card: Sawit's tile, what Sawit Skills put where, their state across the agents, and
/// Install, Update, Add to This Project and Remove.
pub fn card(cx: &App) -> Option<AnyElement> {
    let theme = cx.theme().clone();
    let muted = theme.muted_foreground;
    let state = try_status(cx)?.read(cx);
    let (level, running) = (state.level, state.running);
    let pack = skills::get(SAWIT_PACK).expect("the catalog has the Sawit pack");
    let (mark, badge, color) = match (running, level) {
        (Some(job), _) => (Mark::Spinner, job.doing().to_string(), theme.warning),
        (None, None) => (Mark::Spinner, "Checking…".to_string(), muted),
        (None, Some(Level::NotInstalled)) => (
            Mark::Icon(Lucide::CircleDashed),
            "Not installed".into(),
            muted,
        ),
        (None, Some(Level::Update)) => (
            Mark::Icon(Lucide::CircleAlert),
            "Update available".into(),
            theme.warning,
        ),
        (None, Some(Level::Installed)) => (
            Mark::Icon(Lucide::CircleCheck),
            "Installed, current".into(),
            theme.success,
        ),
    };
    let actions = level.map(|level| {
        let (icon, label, name, _) = face(level);
        let (label, name, tooltip) = match level {
            Level::Installed => (
                label,
                name,
                "Check Every Agent Again and Show Where Sawit Skills Are",
            ),
            _ => (
                label,
                name,
                "Install into Every Agent, Then Show What Changed",
            ),
        };
        let primary = Button::new("skills-sawit-install")
            .when(level != Level::Installed, |b| b.primary())
            .when(level == Level::Installed, |b| b.outline())
            .small()
            .icon(icon)
            .label(running.map_or(label, Job::doing))
            .loading(running.is_some())
            .accessibility_label(running.map_or(name, Job::doing))
            .tooltip(tooltip)
            .on_click(|_, window, cx| run(Job::Install, None, window.window_handle(), cx));
        DropdownButton::new("skills-sawit-split")
            .small()
            .when(level != Level::Installed, |b| b.primary())
            .when(level == Level::Installed, |b| b.outline())
            .button(primary)
            .disabled(running.is_some())
            .menu_accessibility_label("Sawit Skills Actions")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |m, _, _| menu(m, level, None, true))
    });
    v_flex()
        .id("skills-sawit-card")
        .role(Role::Group)
        .aria_label("Sawit Skills")
        .px_4()
        .py_3()
        .gap_1p5()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .child(catalog::columns(
            None,
            catalog::tile(
                img(crate::assets::LOGO_MARK)
                    .size(px(22.))
                    .flex_none()
                    .into_any_element(),
                cx,
            ),
            v_flex()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(div().text_sm().font_semibold().child("Sawit Skills"))
                        .child(catalog::tag(None, "Built In", cx))
                        .child(div().text_xs().text_color(muted).child("by Sawit")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .whitespace_normal()
                        .child(
                            "Teaches every coding agent on this Mac to use Sawit: these skills in each agent's \
                             skills folder, a short block in its own instruction file (~/.claude/CLAUDE.md, \
                             ~/.codex/AGENTS.md, ~/.gemini/GEMINI.md, …), and Sawit's MCP server in each AI \
                             client that lacks it. Add to This Project puts the block in the project's instruction file: the one it has \
                             (AGENTS.md, CLAUDE.md, …), or a new AGENTS.md.",
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .text_color(muted)
                        .whitespace_normal()
                        .child(pack.skills.join(" · ")),
                )
                .when(!state.detail.is_empty(), |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(state.detail.clone()),
                    )
                }),
            catalog::pill(mark, badge, color),
            h_flex().gap_1().children(actions),
        ))
        .into_any_element()
        .into()
}
