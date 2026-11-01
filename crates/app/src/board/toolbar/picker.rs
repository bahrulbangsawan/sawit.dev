//! The toolbar's Templates & table picker, a board's last button: the templates the engine lists
//! when it opens, and a grid that places a table of as many rows and columns as the cell under the
//! pointer.

use gpui_kit::component::button::*;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::toolbar::{self as model, Template};

use super::{PICKER_ICON, Picker};
use crate::board::BoardView;

/// The picker's table grid: up to this many rows, and as many columns.
const GRID: u32 = 6;
/// The side of a cell of that grid.
const CELL: f32 = 18.;

impl BoardView {
    /// The picker opens: the engine lists the templates now, once, for as long as it stays open
    /// (ruling P3-C14), and a flyout that was open closes. When the list can't be had, the error
    /// shows as a toast and the picker offers its table alone.
    fn open_picker(&mut self, cx: &mut Context<Self>) {
        self.open_flyout = None;
        let templates = self.run(model::templates, cx).unwrap_or_default();
        self.picker = Some(Picker {
            templates,
            hover: None,
        });
    }

    /// Places template `id`, one the picker lists, in the middle of the view, and closes the
    /// picker. A text being typed in on the canvas ends first, as before any command of the
    /// chrome ([`BoardView::end_text_edit`], ruling R-TEXTEDIT): placed inside the edit, the
    /// template made no undo step of its own, and the canvas went on typing in the text while the
    /// template showed selected (CANVAS-UI-5).
    pub(in crate::board) fn place_template(&mut self, id: &str, cx: &mut Context<Self>) {
        self.picker = None;
        if self.end_text_edit(Some("board.template"), cx) {
            self.run(|board| model::place_template(board, id), cx);
        }
    }

    /// Places a table of `rows` by `cols` cells in the middle of the view, and closes the picker.
    /// A text being typed in ends first, as for a template.
    pub(in crate::board) fn place_table(&mut self, rows: u32, cols: u32, cx: &mut Context<Self>) {
        self.picker = None;
        if self.end_text_edit(Some("board.table"), cx) {
            self.run(|board| model::place_table(board, rows, cols), cx);
        }
    }
}

/// The button that opens the Templates & table picker, and the picker above it, open while
/// `picker` is given.
pub(super) fn picker_button(picker: Option<Picker>, view: &WeakEntity<BoardView>) -> Popover {
    let open = picker.is_some();
    let Picker { templates, hover } = picker.unwrap_or_default();
    let button = Button::new("board-picker")
        .ghost()
        .icon(PICKER_ICON)
        .accessibility_label("Templates & table")
        .tooltip("Templates & table")
        .debug_selector(|| "board-picker".into());
    let (on_change, view) = (view.clone(), view.clone());
    Popover::new("board-picker-popover")
        .anchor(Anchor::BottomCenter)
        .trigger(button)
        .open(open)
        .on_open_change(move |open, _, cx| {
            let _ = on_change.update(cx, |view, cx| {
                if *open {
                    view.open_picker(cx);
                } else {
                    view.picker = None;
                    cx.notify();
                }
            });
        })
        .content(move |_, _, cx| picker_content(&templates, hover, &view, cx))
}

/// The picker: the board's templates, then a grid of 6 × 6 cells that places a table of as many
/// rows and columns as the cell under the pointer.
fn picker_content(
    templates: &[Template],
    hover: Option<(u32, u32)>,
    view: &WeakEntity<BoardView>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, line, hovered) = (theme.muted_foreground, theme.border, theme.accent);
    let heading = |text: String| div().text_xs().text_color(muted).child(text);
    let rows = templates.iter().enumerate().map(|(i, template)| {
        let (id, view) = (template.id.clone(), view.clone());
        v_flex()
            .id(("board-template", i))
            .px_2()
            .py_1()
            .rounded(px(6.))
            .hover(move |row| row.bg(hovered))
            .child(div().text_sm().child(template.name.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(template.description.clone()),
            )
            .on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.place_template(&id, cx));
            })
    });
    let table = match hover {
        Some((rows, cols)) => format!("Table  {rows} × {cols}"),
        None => "Table".to_owned(),
    };
    v_flex()
        .id("board-picker-content")
        .debug_selector(|| "board-picker-content".into())
        .w(px(260.))
        .gap_2()
        .child(heading("Templates".to_owned()))
        .child(v_flex().gap(px(2.)).children(rows))
        .child(div().h(px(1.)).bg(line))
        .child(heading(table))
        .child(table_grid(hover, view, cx))
}

/// The picker's grid: the cells up to the one under the pointer light up, and a click places a
/// table of that many rows and columns.
fn table_grid(hover: Option<(u32, u32)>, view: &WeakEntity<BoardView>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (line, lit_line, lit) = (theme.border, theme.primary, theme.primary.opacity(0.3));
    let leave = view.clone();
    let rows = (1..=GRID).map(|row| {
        let cells = (1..=GRID).map(|col| {
            let on = hover.is_some_and(|(rows, cols)| row <= rows && col <= cols);
            let (point, place) = (view.clone(), view.clone());
            div()
                .id(("board-table-cell", ((row - 1) * GRID + col - 1) as usize))
                .size(px(CELL))
                .rounded(px(3.))
                .border_1()
                .border_color(if on { lit_line } else { line })
                .when(on, |cell| cell.bg(lit))
                .on_hover(move |entered, _, cx| {
                    if *entered {
                        let _ = point.update(cx, |view, cx| point_at(view, Some((row, col)), cx));
                    }
                })
                .on_click(move |_, _, cx| {
                    let _ = place.update(cx, |view, cx| view.place_table(row, col, cx));
                })
        });
        h_flex().gap(px(3.)).children(cells)
    });
    v_flex()
        .id("board-table-grid")
        .gap(px(3.))
        .on_hover(move |entered, _, cx| {
            if !*entered {
                let _ = leave.update(cx, |view, cx| point_at(view, None, cx));
            }
        })
        .children(rows)
}

/// The pointer is over cell `cell` of the picker's grid (rows and columns), or off it.
fn point_at(view: &mut BoardView, cell: Option<(u32, u32)>, cx: &mut Context<BoardView>) {
    if let Some(picker) = &mut view.picker
        && picker.hover != cell
    {
        picker.hover = cell;
        cx.notify();
    }
}
