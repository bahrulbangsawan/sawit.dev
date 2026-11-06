//! The editors under a gradient or an image row of the Fill and Stroke sections
//! (`sawit_board::design::paints::gradient`): a gradient's kind, angle and stops (each its
//! position, colour and alpha, with a button to add one and one to remove one), and an image's
//! fill mode, tile scale, quarter turn, adjustments and the image itself. Every row also has the
//! kind menu, which turns a solid into a gradient and back.

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::DesignSelection;
use sawit_board::design::paints::gradient::{
    self, ADJUSTMENTS, GradientRow, ImageRow, KINDS, SCALE_MODES,
};

use super::super::controls::{Choices, choice_menu, row, row_button};
use super::super::number_field::{self, Rule, TextField};
use super::{ADD_ICON, OPACITY_WIDTH, REMOVE_ICON, hex_text};
use crate::board::BoardView;

const POSITION_WIDTH: f32 = 52.;
const ANGLE_WIDTH: f32 = 64.;

/// The kind menu of row `index` of list `key`: Solid, Linear, Radial, Angular or Diamond, the
/// row's own checked (none for an image or a pattern).
pub(super) fn kind_menu(
    key: &'static str,
    index: usize,
    current: Option<&'static str>,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let label = KINDS
        .iter()
        .find(|(kind, _)| Some(*kind) == current)
        .map_or("Paint", |(_, name)| *name);
    let view = view.clone();
    let pick = move |kind: &&'static str, cx: &mut App| {
        let kind = *kind;
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(|sel| Some(gradient::set_kind(sel, key, index, kind)), cx);
        });
    };
    let choices = Choices::new(
        || KINDS.iter().map(|(k, n)| (*k, (*n).into())).collect(),
        current,
        pick,
    );
    div()
        .w(px(84.))
        .flex_shrink_0()
        .child(choice_menu(
            &format!("{key}-{index}-kind"),
            label,
            "Paint type",
            choices,
            false,
        ))
        .into_any_element()
}

/// The kind a row's menu shows as chosen.
pub(super) fn shown_kind(row: &GradientRow) -> Option<&'static str> {
    KINDS.iter().map(|(k, _)| *k).find(|k| *k == row.kind)
}

/// The angle field of gradient row `index` of list `key`, in degrees.
pub(super) fn angle_field(
    view: &mut BoardView,
    sel: &DesignSelection,
    key: &'static str,
    index: usize,
    row: &GradientRow,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let rule = Rule::new(
        move |sel| {
            gradient::gradient(sel, key, index)
                .map_or_else(SharedString::default, |g| degrees(g.angle))
        },
        move |sel, typed| {
            let typed = typed.trim().trim_end_matches('°');
            let angle: f64 = typed.parse().ok()?;
            Some(gradient::set_angle(sel, key, index, angle))
        },
    );
    TextField::new(format!("{key}.{index}.angle"), rule)
        .width(ANGLE_WIDTH)
        .showing(degrees(row.angle))
        .render(view, sel, window, cx)
}

fn degrees(angle: f64) -> SharedString {
    format!("{}°", (angle * 10.0).round() / 10.0).into()
}

/// The stops of gradient row `index` of list `key`, one row each, then the button that adds one.
pub(super) fn stop_rows(
    view: &mut BoardView,
    sel: &DesignSelection,
    key: &'static str,
    index: usize,
    row_data: &GradientRow,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    let this = cx.entity().downgrade();
    let mut rows = Vec::new();
    let removable = row_data.stops.len() > 2;
    for (stop, data) in row_data.stops.iter().enumerate() {
        let id = format!("{key}.{index}.stop{stop}");
        let position = TextField::new(
            format!("{id}.position"),
            Rule::new(
                move |sel| stop_text(sel, key, index, stop, |s| number_field::percent(s.position)),
                move |sel, typed| {
                    let p = number_field::parse("opacity", typed)?;
                    Some(gradient::set_stop(
                        sel,
                        key,
                        index,
                        stop,
                        Some(p),
                        None,
                        None,
                    ))
                },
            ),
        )
        .width(POSITION_WIDTH)
        .showing(number_field::percent(data.position))
        .render(view, sel, window, cx);
        let hex = TextField::new(
            format!("{id}.hex"),
            Rule::new(
                move |sel| stop_text(sel, key, index, stop, |s| hex_text(&s.hex)),
                move |sel, typed| {
                    sawit_board::design::paints::parse_hex(typed)?;
                    Some(gradient::set_stop(
                        sel,
                        key,
                        index,
                        stop,
                        None,
                        Some(typed),
                        None,
                    ))
                },
            ),
        )
        .showing(hex_text(&data.hex))
        .render(view, sel, window, cx);
        let alpha = TextField::new(
            format!("{id}.alpha"),
            Rule::new(
                move |sel| stop_text(sel, key, index, stop, |s| number_field::percent(s.opacity)),
                move |sel, typed| {
                    let o = number_field::parse("opacity", typed)?;
                    Some(gradient::set_stop(
                        sel,
                        key,
                        index,
                        stop,
                        None,
                        None,
                        Some(o),
                    ))
                },
            ),
        )
        .width(OPACITY_WIDTH)
        .showing(number_field::percent(data.opacity))
        .render(view, sel, window, cx);
        let swatch = stop_swatch(&data.hex, data.opacity, cx);
        let mut line = row()
            .items_center()
            .pl_4()
            .child(position)
            .child(swatch)
            .child(hex)
            .child(alpha);
        if removable {
            line = line.child(row_button(
                &id,
                "remove",
                REMOVE_ICON,
                "Remove stop",
                &this,
                move |sel| Some(gradient::remove_stop(sel, key, index, stop)),
            ));
        }
        rows.push(line.into_any_element());
    }
    let add = row_button(
        &format!("{key}.{index}"),
        "add-stop",
        ADD_ICON,
        "Add stop",
        &this,
        move |sel| Some(gradient::add_stop(sel, key, index)),
    );
    rows.push(
        row()
            .pl_4()
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Add stop"),
            )
            .child(add)
            .into_any_element(),
    );
    rows
}

fn stop_text(
    sel: &DesignSelection,
    key: &str,
    index: usize,
    stop: usize,
    text: impl Fn(&gradient::Stop) -> SharedString,
) -> SharedString {
    gradient::gradient(sel, key, index)
        .and_then(|g| g.stops.get(stop).map(&text))
        .unwrap_or_default()
}

fn stop_swatch(hex: &str, opacity: f64, cx: &App) -> AnyElement {
    let colour = gpui_kit::Rgba::try_from(hex)
        .map(Hsla::from)
        .unwrap_or_default();
    div()
        .flex_shrink_0()
        .size(px(16.))
        .rounded(px(3.))
        .border_1()
        .border_color(cx.theme().input)
        .bg(colour.opacity(opacity as f32))
        .into_any_element()
}

/// The editor of image row `index` of list `key`: its fill mode, its quarter turn, the tile scale
/// while tiled, the image itself, and its adjustments.
pub(super) fn image_rows(
    view: &mut BoardView,
    sel: &DesignSelection,
    key: &'static str,
    index: usize,
    image: &ImageRow,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    let this = cx.entity().downgrade();
    let id = format!("{key}.{index}");
    let mode_label = SCALE_MODES
        .iter()
        .find(|(m, _)| *m == image.scale_mode)
        .map_or("Fill", |(_, n)| *n);
    let pick = {
        let view = this.clone();
        move |mode: &&'static str, cx: &mut App| {
            let mode = *mode;
            let _ = view.update(cx, |view, cx| {
                view.edit_layers(
                    |sel| Some(gradient::set_scale_mode(sel, key, index, mode)),
                    cx,
                );
            });
        }
    };
    let current = SCALE_MODES
        .iter()
        .map(|(m, _)| *m)
        .find(|m| *m == image.scale_mode);
    let modes = choice_menu(
        &format!("{key}-{index}-scaleMode"),
        mode_label,
        "Image fill mode",
        Choices::new(
            || SCALE_MODES.iter().map(|(m, n)| (*m, (*n).into())).collect(),
            current,
            pick,
        ),
        false,
    );
    let rotate = row_button(
        &id,
        "rotate",
        gpui_kit::assets::IconName::RotateCw,
        "Rotate 90°",
        &this,
        move |sel| Some(gradient::rotate_image(sel, key, index)),
    );
    let choose = {
        let view = this.clone();
        Button::new(SharedString::from(format!("design-{id}-choose")))
            .ghost()
            .small()
            .label("Choose…")
            .tooltip("Put another image in this paint")
            .on_click(move |_, window, cx| {
                let _ = view.update(cx, |view, cx| view.replace_image(key, index, window, cx));
            })
    };
    let mut rows = vec![
        row()
            .pl_4()
            .items_center()
            .child(modes)
            .child(rotate)
            .child(choose)
            .into_any_element(),
    ];
    if image.scale_mode == "tile" {
        let scale = TextField::new(
            format!("{id}.scale"),
            Rule::new(
                move |sel| {
                    gradient::image(sel, key, index)
                        .map_or_else(SharedString::default, |i| number_field::percent(i.scale))
                },
                move |sel, typed| {
                    let s = number_field::parse("opacity", typed).or_else(|| typed.parse().ok())?;
                    Some(gradient::set_tile_scale(sel, key, index, s))
                },
            ),
        )
        .showing(number_field::percent(image.scale))
        .render(view, sel, window, cx);
        rows.push(
            row()
                .pl_4()
                .items_center()
                .child(div().text_xs().w(px(84.)).child("Tile scale"))
                .child(scale)
                .into_any_element(),
        );
    }
    for (n, (name, label)) in ADJUSTMENTS.iter().enumerate() {
        let name: &'static str = name;
        let value = image.adjustments.get(n).copied().unwrap_or(0.0);
        let field = TextField::new(
            format!("{id}.{name}"),
            Rule::new(
                move |sel| {
                    gradient::image(sel, key, index)
                        .and_then(|i| i.adjustments.get(n).copied())
                        .map_or_else(SharedString::default, adjustment_text)
                },
                move |sel, typed| {
                    let typed = typed.trim().trim_end_matches('%');
                    let v: f64 = typed.parse().ok()?;
                    Some(gradient::set_adjustment(sel, key, index, name, v / 100.0))
                },
            ),
        )
        .width(OPACITY_WIDTH)
        .showing(adjustment_text(value))
        .render(view, sel, window, cx);
        rows.push(
            row()
                .pl_4()
                .items_center()
                .child(div().flex_1().text_xs().child(*label))
                .child(field)
                .into_any_element(),
        );
    }
    if image.adjustments.iter().any(|v| *v != 0.0) {
        rows.push(
            row()
                .pl_4()
                .child(row_button(
                    &id,
                    "reset",
                    gpui_kit::assets::IconName::Undo2,
                    "Reset adjustments",
                    &this,
                    move |sel| Some(gradient::reset_adjustments(sel, key, index)),
                ))
                .into_any_element(),
        );
    }
    rows
}

/// An adjustment as its field shows it: -100 to 100.
fn adjustment_text(value: f64) -> SharedString {
    format!("{}", (value * 100.0).round()).into()
}

impl BoardView {
    /// The image editor's Choose…: asks for an image file, then puts it in image paint `index` of
    /// list `key` of the selected layers, keeping the paint's fill mode, crop and adjustments
    /// (`image.replace`). A dialog that is cancelled does nothing.
    pub(super) fn replace_image(
        &mut self,
        key: &'static str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose Image".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.first().map(|p| p.to_string_lossy().to_string()) else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                let Some(ids) = view.board().map(|b| b.session.selection().to_vec()) else {
                    return;
                };
                let target = if key == "strokes" { "stroke" } else { "fill" };
                let params =
                    serde_json::json!({"ids": ids, "target": target, "index": index, "path": path});
                view.choose("image.replace", params, cx);
            });
        })
        .detach();
    }
}

/// The menu that binds `field` (`fills/0/color`) of the selected layers to a variable of `kind`,
/// or lets it go: "No variable" and the file's variables of that kind, the bound one checked.
pub(in crate::board::design_panel) fn bind_menu(
    view: &BoardView,
    sel: &DesignSelection,
    field: String,
    kind: &'static str,
    this: &WeakEntity<BoardView>,
) -> Option<AnyElement> {
    let doc = view.board()?.session.ds().ok()?.doc.clone();
    let options = sawit_board::design::variables::bindable(&doc, kind);
    if options.is_empty() {
        return None;
    }
    let first = *sel.ids.first()?;
    let current = sawit_board::design::variables::bound_to(&doc, first, &field);
    // A diamond, filled while bound: the menu names the variable; the row's hex shows its colour.
    let label = if current.is_some() { "◆" } else { "◇" }.to_string();
    let view = this.clone();
    let ids = sel.ids.clone();
    let bind_field = field.clone();
    let pick = move |variable: &Option<String>, cx: &mut App| {
        let e = sawit_board::design::variables::bind(&ids, &bind_field, variable.as_deref());
        let _ = view.update(cx, |view, cx| view.edit_layers(|_| Some(e), cx));
    };
    let items = move || {
        let mut items: Vec<(Option<String>, SharedString)> = vec![(None, "No variable".into())];
        items.extend(
            options
                .iter()
                .map(|(id, name)| (Some(id.clone()), name.clone().into())),
        );
        items
    };
    Some(
        div()
            .w(px(32.))
            .flex_shrink_0()
            .child(choice_menu(
                &format!("bind-{field}"),
                label,
                "Bind to a variable",
                Choices::new(items, Some(current), pick),
                false,
            ))
            .into_any_element(),
    )
}
