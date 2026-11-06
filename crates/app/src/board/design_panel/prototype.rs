//! The Design panel's Prototype tab (`sawit_board::design::prototype`): the one selected layer's
//! interactions, each its trigger, action, destination, transition and duration, with buttons to
//! add and remove one; the page's flows, each with Present and remove, and a button that makes the
//! selected top-level frame start one; and Present, which shows the frames in the player
//! ([`crate::board::player`]). While the tab shows, the canvas draws the connections.

use figcraft_doc::NodeId;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::prototype::{self, ACTIONS, TRANSITIONS, TRIGGERS};
use sawit_board::design::{DesignSelection, Edit};

use super::controls::{Choices, choice_menu, row, row_button};
use super::number_field::{Rule, TextField};
use super::paints::REMOVE_ICON;
use crate::board::BoardView;

fn menu<T: Clone + PartialEq + 'static>(
    id: String,
    label: String,
    tooltip: &'static str,
    items: Vec<(T, SharedString)>,
    current: Option<T>,
    pick: impl Fn(&T, &mut App) + 'static,
) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .child(choice_menu(
            &id,
            label,
            tooltip,
            Choices::new(move || items.clone(), current, pick),
            false,
        ))
        .into_any_element()
}

fn label_of(list: &[(&'static str, &'static str)], id: &str) -> String {
    list.iter()
        .find(|(k, _)| *k == id)
        .map_or(id.to_string(), |(_, n)| (*n).to_string())
}

/// The tab's body.
pub(super) fn render_prototype(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let muted = theme.muted_foreground;
    let Some((doc, page, selection)) = view.board().and_then(|b| {
        let ds = b.session.ds().ok()?;
        Some((ds.doc.clone(), ds.page, ds.selection.clone()))
    }) else {
        return div().into_any_element();
    };
    let this = cx.entity().downgrade();
    let run = |edit: Edit| {
        let view = this.clone();
        move |cx: &mut App| {
            let edit = edit.clone();
            let _ = view.update(cx, |view, cx| view.choose(edit.command, edit.params, cx));
        }
    };
    let frames = prototype::destinations(&doc, page);
    let frame_items: Vec<(NodeId, SharedString)> = frames
        .iter()
        .map(|(id, n)| (*id, n.clone().into()))
        .collect();
    let mut body = v_flex().gap_3().p_3();
    let start = selection.first().copied();
    body = body.child(
        Button::new("prototype-present")
            .primary()
            .small()
            .label("Present")
            .on_click(cx.listener(move |view, _, _, cx| view.open_player(start, cx))),
    );
    if let [id] = selection.as_slice() {
        let id = *id;
        let mut list = v_flex().gap_2().child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Interactions"),
        );
        let empty_sel = DesignSelection::read(&doc, &[]);
        for (index, row_data) in prototype::interactions(&doc, id).into_iter().enumerate() {
            let trigger = {
                let run = this.clone();
                menu(
                    format!("proto-{index}-trigger"),
                    label_of(&TRIGGERS, &row_data.trigger),
                    "Trigger",
                    TRIGGERS.iter().map(|(k, n)| (*k, (*n).into())).collect(),
                    TRIGGERS
                        .iter()
                        .map(|(k, _)| *k)
                        .find(|k| *k == row_data.trigger),
                    move |t: &&'static str, cx| {
                        let e = prototype::set_trigger(id, index, t);
                        let _ = run.update(cx, |v, cx| v.choose(e.command, e.params, cx));
                    },
                )
            };
            let dest = row_data.dest.or_else(|| frames.first().map(|f| f.0));
            let url = row_data.url.clone();
            let action = {
                let run = this.clone();
                menu(
                    format!("proto-{index}-action"),
                    label_of(&ACTIONS, &row_data.action),
                    "Action",
                    ACTIONS.iter().map(|(k, n)| (*k, (*n).into())).collect(),
                    ACTIONS
                        .iter()
                        .map(|(k, _)| *k)
                        .find(|k| *k == row_data.action),
                    move |a: &&'static str, cx| {
                        if let Some(e) = prototype::set_action(id, index, a, dest, url.as_deref()) {
                            let _ = run.update(cx, |v, cx| v.choose(e.command, e.params, cx));
                        }
                    },
                )
            };
            let mut rows = vec![row().child(trigger).child(action).into_any_element()];
            if matches!(
                row_data.action.as_str(),
                "navigate" | "openOverlay" | "scrollTo"
            ) {
                let current = row_data.dest;
                let label = frames
                    .iter()
                    .find(|(f, _)| Some(*f) == current)
                    .map_or("Choose a frame".to_string(), |(_, n)| n.clone());
                let kind = row_data.action.clone();
                let run = this.clone();
                rows.push(
                    row()
                        .child(menu(
                            format!("proto-{index}-dest"),
                            label,
                            "Destination",
                            frame_items.clone(),
                            current,
                            move |d: &NodeId, cx| {
                                if let Some(e) =
                                    prototype::set_action(id, index, &kind, Some(*d), None)
                                {
                                    let _ =
                                        run.update(cx, |v, cx| v.choose(e.command, e.params, cx));
                                }
                            },
                        ))
                        .into_any_element(),
                );
            }
            let transition = {
                let run = this.clone();
                menu(
                    format!("proto-{index}-transition"),
                    label_of(&TRANSITIONS, &row_data.transition),
                    "Animation",
                    TRANSITIONS.iter().map(|(k, n)| (*k, (*n).into())).collect(),
                    TRANSITIONS
                        .iter()
                        .map(|(k, _)| *k)
                        .find(|k| *k == row_data.transition),
                    move |t: &&'static str, cx| {
                        let e = prototype::set_transition(id, index, t);
                        let _ = run.update(cx, |v, cx| v.choose(e.command, e.params, cx));
                    },
                )
            };
            let duration = TextField::new(
                format!("proto.{index}.duration"),
                Rule::new(
                    move |_| SharedString::from(format!("{}ms", row_data.duration_ms)),
                    move |_, typed| {
                        let ms: f64 = typed.trim().trim_end_matches("ms").trim().parse().ok()?;
                        prototype::set_duration(id, index, ms)
                    },
                ),
            )
            .width(72.)
            .showing(format!("{}ms", row_data.duration_ms).into())
            .render(view, &empty_sel, window, cx);
            let remove = row_button(
                &format!("proto-{index}"),
                "remove",
                REMOVE_ICON,
                "Remove interaction",
                &this,
                move |_| Some(prototype::remove(id, index)),
            );
            rows.push(
                row()
                    .items_center()
                    .child(transition)
                    .child(duration)
                    .child(remove)
                    .into_any_element(),
            );
            list = list.child(
                v_flex()
                    .gap_1()
                    .p_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.border)
                    .children(rows),
            );
        }
        let other = frames
            .iter()
            .map(|f| f.0)
            .find(|f| Some(*f) != doc.top_level_frame(id));
        let add = run(prototype::add(id, other));
        list = list.child(
            Button::new("prototype-add")
                .ghost()
                .small()
                .label("+ Interaction")
                .on_click(move |_, _, cx| add(cx)),
        );
        body = body.child(list);
    } else {
        body = body.child(
            div()
                .text_sm()
                .text_color(muted)
                .child("Select one layer to give it interactions."),
        );
    }
    let mut flows = v_flex().gap_1().child(
        div()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .child("Flows"),
    );
    for (start, name) in prototype::flows(&doc, page) {
        let remove = run(prototype::remove_flow(start));
        flows = flows.child(
            h_flex()
                .gap_1()
                .items_center()
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(
                    Button::new(SharedString::from(format!("flow-present-{}", start.0)))
                        .ghost()
                        .xsmall()
                        .label("Present")
                        .on_click(
                            cx.listener(move |view, _, _, cx| view.open_player(Some(start), cx)),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("flow-remove-{}", start.0)))
                        .ghost()
                        .xsmall()
                        .label("Remove")
                        .on_click(move |_, _, cx| remove(cx)),
                ),
        );
    }
    let top = selection
        .first()
        .copied()
        .filter(|id| doc.top_level_frame(*id) == Some(*id));
    if let Some(top) = top {
        let add = run(prototype::add_flow(top));
        flows = flows.child(
            Button::new("flow-add")
                .ghost()
                .small()
                .label("+ Flow starting here")
                .on_click(move |_, _, cx| add(cx)),
        );
    }
    body.child(flows).into_any_element()
}
