//! A table's card: its name, schema and columns with their keys and types, or zoomed out, only
//! its name as large as fits.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::diagram;

use super::layout::short_type;
use super::{CARD_PAD, DETAIL_ZOOM, DiagramView, HEADER_H, ROW_H};

/// What a card says, made once per schema load rather than on every frame: the table's name and
/// schema, and each column's name and type ("varchar(255)", with "?" when nullable).
pub(super) struct CardText {
    name: SharedString,
    schema: SharedString,
    columns: Vec<(SharedString, SharedString)>,
}

impl CardText {
    pub(super) fn new(table: &diagram::ErdTable) -> Self {
        Self {
            name: table.name.clone().into(),
            schema: table.schema.clone().into(),
            columns: table
                .columns
                .iter()
                .map(|col| {
                    let ty = format!(
                        "{}{}",
                        short_type(&col.data_type),
                        if col.nullable { "?" } else { "" }
                    );
                    (col.name.clone().into(), ty.into())
                })
                .collect(),
        }
    }
}

impl DiagramView {
    pub(super) fn render_card(&self, ix: usize, dim: bool, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let table = &self.erd.tables[ix];
        let card = &self.texts[ix];
        let z = self.zoom;
        let at = self.to_screen(self.pos[ix]) - self.viewport.get().origin;
        let size = self.sizes[ix];
        let selected = self.selected == Some(ix);
        let current_hit = self.hits.get(self.hit).copied();
        let table_hit = self.is_hit((ix, None));
        let border = if selected {
            theme.primary
        } else if table_hit {
            theme.warning
        } else {
            theme.border
        };
        let detail = z >= DETAIL_ZOOM;
        let text = px((12. * z).max(if detail { 1. } else { 10. }));
        let header = h_flex()
            .h(px(HEADER_H * z))
            .px(px(10. * z))
            .gap(px(6. * z))
            .flex_none()
            .bg(theme.muted)
            .border_b(px(1.))
            .border_color(border.opacity(if selected { 0.6 } else { 1. }))
            .text_size(if detail { text } else { px((14. * z).max(11.)) })
            .child(
                svg()
                    .path(Lucide::Table.path())
                    .size(px(13. * z))
                    .flex_none()
                    .text_color(theme.primary),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(card.name.clone()),
            )
            .when(detail, |el| {
                el.child(div().flex_1()).child(
                    div()
                        .flex_none()
                        .text_size(px(10.5 * z))
                        .text_color(theme.muted_foreground)
                        .child(card.schema.clone()),
                )
            });
        let rows =
            detail.then(|| {
                let columns = table.columns.iter().zip(&card.columns);
                columns.enumerate().map(move |(c, (col, (name, ty)))| {
                    let hit = self.is_hit((ix, Some(c)));
                    let current = current_hit == Some((ix, Some(c)));
                    let key = if col.pk {
                        Some((Lucide::KeyRound, theme.warning))
                    } else if col.fk {
                        Some((Lucide::Link, theme.info))
                    } else {
                        None
                    };
                    h_flex()
                        .h(px(ROW_H * z))
                        .px(px(10. * z))
                        .gap(px(6. * z))
                        .text_size(text)
                        .when(hit, |el| {
                            el.bg(theme.warning.opacity(if current { 0.3 } else { 0.14 }))
                        })
                        .child(div().w(px(12. * z)).flex_none().when_some(
                            key,
                            |el, (icon, color)| {
                                el.child(
                                    svg().path(icon.path()).size(px(11. * z)).text_color(color),
                                )
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.foreground)
                                .when(col.pk, |el| el.font_semibold())
                                .child(name.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .max_w(px(size.width * z * 0.45))
                                .truncate()
                                .font_family(theme.mono_font_family.clone())
                                .text_size(px(11. * z))
                                .text_color(theme.muted_foreground)
                                .child(ty.clone()),
                        )
                })
            });
        v_flex()
            .id(("erd-table", ix))
            .absolute()
            .left(at.x)
            .top(at.y)
            .w(px(size.width * z))
            .h(px(size.height * z))
            .overflow_hidden()
            .rounded(px(8. * z))
            .border(px(if selected { 2. } else { 1. }))
            .border_color(border)
            .bg(theme.background)
            .when(dim, |el| el.opacity(0.35))
            .map(|el| {
                if detail {
                    el.child(header)
                } else {
                    // Zoomed out: just the name, as large as fits, broken
                    // only at underscores.
                    let (fit, lines) =
                        name_lines(&table.name, size.width * z - 8., size.height * z - 8.);
                    el.bg(theme.muted).child(
                        v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .p(px(4.))
                            .text_size(px(fit))
                            .line_height(px(fit * 1.2))
                            .font_semibold()
                            .text_color(theme.foreground)
                            .children(
                                lines
                                    .into_iter()
                                    .map(|l| div().max_w_full().truncate().child(l)),
                            ),
                    )
                }
            })
            .when_some(rows, |el, rows| el.pt(px(CARD_PAD * z / 2.)).children(rows))
            .into_any_element()
    }
}

/// A zoomed-out card's label: the font size that fits the name's longest
/// `_`-separated part into `w`, and the name packed into lines at its
/// underscores. Whatever exceeds the lines `h` holds joins the last line,
/// which the card truncates with an ellipsis.
pub(super) fn name_lines(name: &str, w: f32, h: f32) -> (f32, Vec<String>) {
    let parts: Vec<&str> = name.split_inclusive('_').collect();
    let longest = parts
        .iter()
        .map(|p| p.chars().count())
        .max()
        .unwrap_or(0)
        .max(4) as f32;
    let fit = (w / (longest * 0.6)).clamp(8., 13.).min((h / 1.2).max(8.));
    let cols = ((w / (fit * 0.6)) as usize).max(1);
    let max_lines = ((h / (fit * 1.2)) as usize).max(1);
    let mut lines: Vec<String> = Vec::new();
    for p in parts {
        match lines.last_mut() {
            Some(l) if l.chars().count() + p.chars().count() <= cols => l.push_str(p),
            _ => lines.push(p.to_string()),
        }
    }
    if lines.len() > max_lines {
        let rest = lines.split_off(max_lines - 1).concat();
        lines.push(rest);
    }
    (fit, lines)
}
