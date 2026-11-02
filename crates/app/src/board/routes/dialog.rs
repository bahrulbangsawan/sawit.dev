//! Add Routes from Dev Server's dialog, over the canvas like the Actions palette: the dev server
//! (a choice when the project runs several), its routes ticked to be added with the file each comes
//! from, the dynamic routes skipped for want of a sample, the viewports, and Add. Escape or Cancel
//! closes it, Enter adds.

use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::routes::{MOST_ROUTES, server_name};

use super::{Dialog, Step};
use crate::board::BoardView;
use crate::catalog::tilde;

const WIDTH: f32 = 520.;
const TOP: f32 = 64.;
/// The routes list's tallest, before it scrolls.
const LIST_MAX: f32 = 260.;

/// The dialog, while it is open.
pub(in crate::board) fn render_routes_dialog(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Option<AnyElement> {
    // It takes the keyboard with the first frame it is drawn in.
    if let Some(dialog) = view.routes_ui.dialog.as_mut()
        && std::mem::take(&mut dialog.focus_pending)
    {
        window.focus(&dialog.focus, cx);
    }
    let dialog = view.routes_ui.dialog.as_ref()?;
    let theme = cx.theme().clone();
    let muted = theme.muted_foreground;
    let section = |title: &'static str| {
        div()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(muted)
            .child(title)
    };
    let body = match &dialog.step {
        Step::Servers => note("Looking for the project's dev server…", muted).into_any_element(),
        Step::NoServer(folder) => v_flex()
            .gap_2()
            .child(note(
                &format!(
                    "No dev server is running in {}. Start one (npm run dev, say), then try again.",
                    tilde(folder)
                ),
                muted,
            ))
            .child(
                Button::new("routes-retry")
                    .small()
                    .label("Try Again")
                    .on_click(cx.listener(|view, _, _, cx| view.find_servers(cx))),
            )
            .into_any_element(),
        Step::Routes | Step::Ready => v_flex()
            .gap_3()
            .child(servers(dialog, cx))
            .child(section("Routes"))
            .child(routes(dialog, cx))
            .child(section("Viewports"))
            .child(widths(dialog, cx))
            .into_any_element(),
    };
    let count = dialog.chosen().len() * dialog.chosen_widths().len();
    let ready = dialog.step == Step::Ready && count > 0;
    let footer = h_flex()
        .justify_end()
        .gap_2()
        .child(
            Button::new("routes-cancel")
                .ghost()
                .small()
                .label("Cancel")
                .on_click(cx.listener(|view, _, _, cx| view.close_routes_dialog(cx))),
        )
        .child(
            Button::new("routes-add")
                .primary()
                .small()
                .label(match count {
                    1 => "Add 1 Frame".to_string(),
                    n => format!("Add {n} Frames"),
                })
                .disabled(!ready)
                .on_click(cx.listener(|view, _, _, cx| {
                    view.add_chosen_routes(cx);
                })),
        );
    Some(
        v_flex()
            .id("routes-dialog-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .items_center()
            .pt(px(TOP))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    view.close_routes_dialog(cx);
                }),
            )
            .child(
                v_flex()
                    .id("routes-dialog")
                    .debug_selector(|| "routes-dialog".into())
                    .track_focus(&dialog.focus)
                    .on_key_down(cx.listener(dialog_key))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(WIDTH))
                    .max_w_full()
                    .p_4()
                    .gap_4()
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .shadow_lg()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Add Routes from Dev Server"),
                    )
                    .child(body)
                    .child(footer),
            )
            .into_any_element(),
    )
}

fn note(text: &str, color: Hsla) -> Div {
    div().text_sm().text_color(color).child(text.to_string())
}

/// The dev server: what it is, and a choice of the others the project runs.
fn servers(dialog: &Dialog, cx: &mut Context<BoardView>) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let framework = (dialog.found.as_ref())
        .and_then(|f| f.framework)
        .map(|f| format!(" · {}", f.name()))
        .unwrap_or_default();
    let Some(server) = dialog.servers.get(dialog.server) else {
        return div().into_any_element();
    };
    let about = div().text_sm().text_color(muted).truncate().child(format!(
        "{} · {}{framework}",
        server.label,
        tilde(&server.folder)
    ));
    if dialog.servers.len() < 2 {
        return v_flex()
            .gap_0p5()
            .child(div().text_sm().child(server_name(&server.url)))
            .child(about)
            .into_any_element();
    }
    let choices = dialog.servers.iter().enumerate().map(|(index, other)| {
        Button::new(("routes-server", index))
            .small()
            .ghost()
            .selected(index == dialog.server)
            .label(server_name(&other.url))
            .on_click(cx.listener(move |view, _, _, cx| view.find_routes(index, cx)))
    });
    v_flex()
        .gap_1()
        .child(h_flex().flex_wrap().gap_1().children(choices))
        .child(about)
        .into_any_element()
}

/// The routes found, each ticked to be added, then the dynamic routes skipped.
fn routes(dialog: &Dialog, cx: &mut Context<BoardView>) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let Some(found) = dialog.found.as_ref() else {
        return note("Finding its routes…", muted).into_any_element();
    };
    if found.routes.is_empty() {
        let why = match &found.crawl_error {
            Some(error) => format!("No routes found: {error}."),
            None => "No routes found in its files or its links.".to_string(),
        };
        return note(&why, muted).into_any_element();
    }
    let rows = found.routes.iter().enumerate().map(|(index, route)| {
        let path = route.path.clone();
        let ticked = !dialog.left_out.contains(&path);
        let from = match (&route.sample_of, &route.file) {
            (Some(pattern), _) => format!("a page of {pattern}"),
            (None, Some(file)) => file.clone(),
            (None, None) => "linked from the site".to_string(),
        };
        h_flex()
            .gap_2()
            .py_0p5()
            .child(
                Checkbox::new(("routes-route", index))
                    .label(path.clone())
                    .checked(ticked)
                    .on_click(cx.listener(move |view, on: &bool, _, cx| {
                        if let Some(dialog) = view.routes_ui.dialog.as_mut() {
                            match on {
                                true => dialog.left_out.remove(&path),
                                false => dialog.left_out.insert(path.clone()),
                            };
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .text_right()
                    .child(from),
            )
    });
    let mut notes = Vec::new();
    if !found.skipped.is_empty() {
        let names: Vec<&str> = found.skipped.iter().map(|s| s.route.as_str()).collect();
        notes.push(format!(
            "Skipped, with no page of theirs linked: {}",
            names.join(", ")
        ));
    }
    if found.routes.len() > MOST_ROUTES {
        notes.push(format!("The first {MOST_ROUTES} ticked are added at once."));
    }
    v_flex()
        .gap_1()
        .child(
            v_flex()
                .id("routes-list")
                .max_h(px(LIST_MAX))
                .overflow_y_scroll()
                .children(rows),
        )
        .children(notes.into_iter().map(|text| note(&text, muted).text_xs()))
        .into_any_element()
}

/// The widths offered, ticked or not.
fn widths(dialog: &Dialog, cx: &mut Context<BoardView>) -> AnyElement {
    let chips = dialog
        .widths
        .iter()
        .enumerate()
        .map(|(index, (width, on))| {
            Checkbox::new(("routes-width", index))
                .label(format!("{width}"))
                .checked(*on)
                .on_click(cx.listener(move |view, ticked: &bool, _, cx| {
                    if let Some(dialog) = view.routes_ui.dialog.as_mut()
                        && let Some((_, on)) = dialog.widths.get_mut(index)
                    {
                        *on = *ticked;
                    }
                    cx.notify();
                }))
        });
    h_flex()
        .flex_wrap()
        .gap_4()
        .children(chips)
        .into_any_element()
}

/// Escape closes the dialog, Enter adds.
fn dialog_key(
    view: &mut BoardView,
    event: &KeyDownEvent,
    _: &mut Window,
    cx: &mut Context<BoardView>,
) {
    match event.keystroke.key.as_str() {
        "escape" => view.close_routes_dialog(cx),
        "enter" => {
            view.add_chosen_routes(cx);
        }
        _ => return,
    }
    cx.stop_propagation();
}
