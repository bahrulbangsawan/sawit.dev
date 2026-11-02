//! The Export section (spec "Design panel" 9): the layers' export settings, a format and a scale
//! each, and the button that exports the layers as they say ([`BoardView::export_selection`]).
//!
//! - A layer with no setting shows one row, PNG at 1×, which is what exporting it makes. The row
//!   goes into the layer once it is changed, or another is added.
//! - Layers whose settings differ show "Mixed", and its "Replace all" gives them all that one
//!   row. Until then the Export button exports each with its own settings.
//! - The settings change only through `sawit_board::design::export::set_export_rows`, which keeps
//!   what a setting holds besides its format and scale while its row is left as it is.
//!
//! It also holds the canvas's one export writer (ruling P6-C8), which File ▸ Export, Export as SVG
//! and Export frames to PDF write through too (`super::super::commands`): into the exports folder,
//! then the first file shows in the Finder ([`BoardView::reveal_exports`]).

mod files;

use std::rc::Rc;

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::export::{ExportRow, export_rows, set_export_rows};
use sawit_board::design::{DesignSelection, Edit, Mixed};
use serde_json::Value;

use super::controls::{
    Choices, choice_menu, guarded, icon_button, mixed_row, on_click_edit, row, row_button, section,
};
use super::number_field::number_text;
use super::paints::{ADD_ICON, REMOVE_ICON};
use crate::board::BoardView;
use crate::board::option_icons;

/// The formats a row can be set to: each one's extension, and what the menu calls it.
pub(super) const FORMATS: [(&str, &str); 4] = [
    ("png", "PNG"),
    ("jpg", "JPG"),
    ("svg", "SVG"),
    ("pdf", "PDF"),
];
/// The scales a row can be set to.
const SCALES: [f64; 5] = [0.5, 1.0, 2.0, 3.0, 4.0];
/// The formats made of pixels, which a scale makes larger or smaller: the others are the same
/// file whatever the scale.
const PIXEL_FORMATS: [&str; 3] = ["png", "jpg", "webp"];
/// The folder in Sawit's own folder that takes the exports of a canvas outside a project.
const EXPORTS: &str = "exports";
/// The format the page is exported in with nothing selected, when no other is asked for.
const PAGE_FORMAT: &str = "png";

/// The Export section for the layers of `sel`.
pub(super) fn render(sel: &Rc<DesignSelection>, cx: &mut Context<BoardView>) -> AnyElement {
    let this = cx.entity().downgrade();
    let shown = guarded("ui.exports", Vec::new(), || export_rows(sel));
    let settings = sel.get("exports");
    // Whether the layers hold settings of their own, which a row's button can remove.
    let held = matches!(&settings, Mixed::Same(Value::Array(held)) if !held.is_empty());
    let add = icon_button("design-exports-add", ADD_ICON, "Add export")
        .debug_selector(|| "design-exports-add".into())
        .on_click(on_click_edit(&this, |sel| {
            Some(set_export_rows(sel, &with_another(export_rows(sel))))
        }))
        .into_any_element();
    let mut rows: Vec<AnyElement> = if settings == Mixed::Mixed {
        let replace = |sel: &DesignSelection| Some(set_export_rows(sel, &[ExportRow::default()]));
        vec![mixed_row("exports", &this, replace, cx)]
    } else {
        let rows = shown.iter().enumerate();
        rows.map(|(index, row)| export_row(index, row, held, &this))
            .collect()
    };
    rows.push(export_button(sel.ids.len(), &this));
    section("export", "Export", vec![add], rows, cx)
}

/// Row `index`, `row`: its format and its scale, and while the layers hold settings of their
/// own (`held`), the button that removes it.
fn export_row(
    index: usize,
    shown: &ExportRow,
    held: bool,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let id = format!("exports-{index}");
    let drawn = row()
        .items_center()
        .child(format_menu(&id, index, shown, view))
        .child(scale_menu(&id, index, shown, view));
    if !held {
        return drawn.into_any_element();
    }
    let remove = row_button(
        &id,
        "remove",
        REMOVE_ICON,
        "Remove export",
        view,
        move |sel| {
            let mut rows = export_rows(sel);
            (index < rows.len()).then(|| {
                rows.remove(index);
                set_export_rows(sel, &rows)
            })
        },
    );
    drawn.child(remove).into_any_element()
}

/// The format menu of row `index`, which shows `shown`'s.
fn format_menu(
    id: &str,
    index: usize,
    shown: &ExportRow,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let current = FORMATS
        .iter()
        .map(|(format, _)| *format)
        .find(|format| *format == shown.format);
    let choices = || {
        let named = FORMATS.iter();
        named
            .map(|(format, name)| (*format, (*name).into()))
            .collect()
    };
    let view = view.clone();
    let pick = move |format: &&'static str, cx: &mut App| {
        let format = *format;
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(change_row(index, |row| row.format = format.to_string()), cx);
        });
    };
    let menu = format!("{id}-format");
    let label = shown.format.to_uppercase();
    let choices = Choices::new(choices, current, pick)
        .with_pictures(|format: &&'static str| option_icons::export_format(format));
    choice_menu(&menu, label, "Format", choices, false).into_any_element()
}

/// The scale menu of row `index`, which shows `shown`'s. Off for a format that isn't pixels.
fn scale_menu(
    id: &str,
    index: usize,
    shown: &ExportRow,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let current = SCALES.into_iter().find(|scale| *scale == shown.scale);
    let choices = || {
        let named = SCALES.into_iter();
        named.map(|scale| (scale, scale_name(scale))).collect()
    };
    let view = view.clone();
    let pick = move |scale: &f64, cx: &mut App| {
        let scale = *scale;
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(change_row(index, |row| row.scale = scale), cx);
        });
    };
    let pixels = PIXEL_FORMATS.contains(&shown.format.as_str());
    let menu = format!("{id}-scale");
    let choices = Choices::new(choices, current, pick);
    choice_menu(&menu, scale_name(shown.scale), "Scale", choices, !pixels).into_any_element()
}

/// A scale as the menu names it: "2×".
fn scale_name(scale: f64) -> SharedString {
    format!("{}×", number_text(scale)).into()
}

/// The edit that changes row `index` of the layers' export rows, as they are when it is made,
/// with `change`. `None` when there is no such row.
fn change_row(
    index: usize,
    change: impl FnOnce(&mut ExportRow),
) -> impl FnOnce(&DesignSelection) -> Option<Edit> {
    move |sel| {
        let mut rows = export_rows(sel);
        change(rows.get_mut(index)?);
        Some(set_export_rows(sel, &rows))
    }
}

/// `rows` and one more: the last one's format at the next scale it isn't at yet (1× when it is at
/// every larger one), or PNG at 1× after none.
fn with_another(mut rows: Vec<ExportRow>) -> Vec<ExportRow> {
    let next = match rows.last() {
        None => ExportRow::default(),
        Some(last) => {
            let taken = |scale: f64| {
                (rows.iter()).any(|row| row.format == last.format && row.scale == scale)
            };
            let mut larger = SCALES.into_iter().filter(|scale| *scale > last.scale);
            let scale = larger.find(|scale| !taken(*scale));
            ExportRow {
                format: last.format.clone(),
                scale: scale.unwrap_or(1.0),
            }
        }
    };
    rows.push(next);
    rows
}

/// The Export button, for `count` layers: it exports them, then shows the first file in the
/// Finder; a failed export says why in a toast.
fn export_button(count: usize, view: &WeakEntity<BoardView>) -> AnyElement {
    let label = if count == 1 {
        "Export 1 layer".to_string()
    } else {
        format!("Export {count} layers")
    };
    let view = view.clone();
    Button::new("design-export")
        .outline()
        .small()
        .w_full()
        .label(label)
        .debug_selector(|| "design-export".into())
        .on_click(move |_, _, cx| {
            let _ = view.update(cx, |view, cx| view.export_and_reveal(cx));
        })
        .into_any_element()
}

#[cfg(test)]
mod tests;
