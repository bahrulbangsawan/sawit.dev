//! The roster in the canvas's top bar: the colours of the agents at work, then what they are at,
//! "3 building · 1 done · 1 failed", with the idle ones collapsed into "+N". A click lists each
//! agent with its components and their state; the user's choice takes the view to one.

use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agents::work::RowLook;

use super::rows::said;
use crate::board::BoardView;

/// How many agents' colours the roster shows at most.
const DOTS: usize = 6;

/// The roster of `view`'s canvas, while agents are on it and their cursors show.
pub(in crate::board) fn render_roster(
    view: &BoardView,
    cx: &Context<BoardView>,
) -> Option<AnyElement> {
    if !crate::agents::shown(cx) || view.agents.agents.is_empty() && view.agents.works.is_empty() {
        return None;
    }
    let now = cx.background_executor().now();
    let presence = &view.agents;
    let tally = presence.tally(now);
    let words = tally.words();
    let mut agents: Vec<_> = presence.agents.iter().collect();
    agents.sort_by_key(|a| std::cmp::Reverse(a.seen));
    let busy = |id: &SharedString| {
        let working = (presence.works.iter()).any(|w| {
            w.look.id == *id && matches!(w.row(now), RowLook::Working { .. } | RowLook::Failed)
        });
        let recent = (presence.agents.iter()).any(|a| {
            a.look.id == *id
                && now.saturating_duration_since(a.seen) < sawit_core::agents::FADE_AFTER
        });
        working || recent
    };
    let colours: Vec<Hsla> = (agents.iter())
        .filter(|a| busy(&a.look.id))
        .take(DOTS)
        .map(|a| a.look.color)
        .collect();
    let mut label = words.clone();
    if tally.idle > 0 {
        label = match label.is_empty() {
            true => format!("{} idle", tally.idle),
            false => format!("{label}, {} idle", tally.idle),
        };
    }
    let name: SharedString = format!("Agents: {label}. List them").into();
    let dots = h_flex().children(colours.into_iter().enumerate().map(|(ix, colour)| {
        div()
            .size(px(10.0))
            .rounded_full()
            .border_1()
            .border_color(gpui_kit::white())
            .bg(colour)
            .when(ix > 0, |dot| dot.ml(px(-3.0)))
    }));
    let this = cx.entity().downgrade();
    let muted = cx.theme().muted_foreground;
    let roster = Button::new("board-agents-roster")
        .ghost()
        .small()
        .accessibility_label(name.clone())
        .tooltip(name)
        .debug_selector(|| "board-agents-roster".into())
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .text_xs()
                .child(dots)
                .when(!words.is_empty(), |row| row.child(words))
                .when(tally.idle > 0, |row| {
                    row.child(div().text_color(muted).child(format!("+{}", tally.idle)))
                }),
        )
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu: PopupMenu, _, cx| {
            list(menu, &this, cx)
        });
    Some(roster.into_any_element())
}

/// The roster's list: each agent, the most recent first, then its components with their state,
/// or what it last did; a click takes the view there.
fn list(mut menu: PopupMenu, view: &WeakEntity<BoardView>, cx: &App) -> PopupMenu {
    let Some(board) = view.upgrade() else {
        return menu;
    };
    let board = board.read(cx);
    let now = cx.background_executor().now();
    let mut agents: Vec<_> = board.agents.agents.iter().collect();
    agents.sort_by_key(|a| std::cmp::Reverse(a.seen));
    for (ix, agent) in agents.into_iter().enumerate() {
        if ix > 0 {
            menu = menu.separator();
        }
        menu = menu.label(agent.look.label.clone());
        let works: Vec<_> = (board.agents.works.iter())
            .filter(|w| w.look.id == agent.look.id)
            .collect();
        for work in &works {
            let (view, id) = (view.clone(), work.id.clone());
            let text = said(work, work.row(now));
            let text = text
                .rsplit_once(" · ")
                .map_or(text.clone(), |(rest, _)| rest.to_string());
            menu = menu.item(PopupMenuItem::new(text).on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.go_to_work(&id, cx));
            }));
        }
        if works.is_empty()
            && let Some(at) = agent.at
        {
            let view = view.clone();
            let text = match agent.doing.is_empty() {
                true => "Working".to_string(),
                false => capitalized(&agent.doing),
            };
            menu = menu.item(PopupMenuItem::new(text).on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.go_to(at.page, at.point, cx));
            }));
        }
    }
    menu
}

/// `words` with its first letter a capital: "Adding 3 stickies".
fn capitalized(words: &str) -> String {
    let mut letters = words.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}
