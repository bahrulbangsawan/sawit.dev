//! The canvas's menus (spec D8, "Menu policy", "Context menu"): the main menu, the context menu,
//! the Actions palette and the keyboard-shortcuts sheet. `sawit_board` lists what they hold
//! ([`menu`](sawit_board::menu), [`actions`](sawit_board::actions),
//! [`shortcuts`](sawit_board::shortcuts)); this draws it, and runs the command picked through
//! [`BoardView::run_menu`].
//!
//! - **The main menu** drops from the top bar's ≡ button ([`super::top_bar`]): FigCraft's menu
//!   catalog under Sawit's policy, its submenus nested, each item with its keys on the right.
//! - **The context menu** opens where the right button goes down on the canvas, once the text
//!   being typed in has ended and what is there is selected ([`BoardView::on_right_down`]), and
//!   under the selection for ⇧F10 (`view.contextMenu`). Its Paste here pastes where it opened.
//! - **The Actions palette** (⌘K, ⌘/) searches the main menu's commands and lists the best
//!   [`PALETTE_RESULTS`]. Up and Down move the highlight, Enter runs it, and Escape or a press
//!   outside closes the palette, as in FigCraft.
//! - **The shortcuts sheet** (⌃⇧?, Help ▸ Keyboard shortcuts) lists the commands' keys in a column
//!   per group. Escape or a press outside closes it.
//!
//! The keys they show are the keymap's as it is now ([`super::command_keys`]): FigCraft's while
//! they still run their commands, written as FigCraft writes them, else the keys the user gave
//! them in the keymap editor or keymap.json.
//!
//! Each takes the keyboard while it is open, handles its own keys, and gives the keyboard back to
//! the canvas when it closes (ruling P6-C3). The canvas takes only the keys pressed while it has
//! the keyboard itself, so nothing typed in the palette reaches its tools. One of the context menu,
//! the palette and the sheet is open at a time. They show while the UI is hidden too, unlike the
//! rest of the chrome: the empty canvas's context menu holds Show/Hide UI, the way back (ruling
//! P6-C6).
//!
//! The palette's field, the sheet and the context menu's popup are built with the first frame
//! each shows in, which has the window an op has none of: an op only marks one open (ruling
//! P6-C7). Every model is read behind the engine's guard: a panic in it leaves that menu empty,
//! and is logged (D15).

mod palette;
mod popup;
mod sheet;

use figcraft_engine::guard;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::actions::ActionItem;
use sawit_board::policy;
use sawit_board::shortcuts::ShortcutGroup;
use serde_json::{Value, json};

pub(super) use popup::fill_main_menu;

use super::{BoardView, commands};

/// How many of the best matches the palette lists.
const PALETTE_RESULTS: usize = 12;
/// The palette's width.
const PALETTE_WIDTH: f32 = 480.;
/// How far under the canvas tab's top the palette floats: clear of the top bar.
const PALETTE_TOP: f32 = 64.;
/// The height of a row of the palette.
const PALETTE_ROW: f32 = 30.;
/// The picture in the palette's search field.
pub(super) const SEARCH_ICON: Lucide = Lucide::Search;
/// The width of a column of the sheet: one group's keys.
const SHEET_COLUMN: f32 = 248.;
/// Most keys a row of the sheet shows, as FigCraft's own panel: the rest work without being
/// listed.
const SHEET_KEYS: usize = 2;
/// How far under the selection's bottom left corner the context menu opens from the keyboard, as
/// FigCraft's does.
const UNDER_SELECTION: f64 = 8.;
/// A menu with more items than this scrolls, as gpui-kit's own menus built from a list do.
const SCROLLS_PAST: usize = 20;
/// What joins the names in an action's path (`sawit_board::actions::ActionItem::path`).
const PATH_SEPARATOR: &str = " › ";

/// Where the context menu opened: its place in the window, and the page point under it, where its
/// Paste here pastes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MenuAt {
    pub(super) position: Point<Pixels>,
    pub(super) page: (f64, f64),
}

/// The context menu, the palette and the sheet: which is open, and what each keeps while it is.
#[derive(Default)]
pub(super) struct Menus {
    /// The Actions palette is open.
    pub(super) palette_open: bool,
    /// The keyboard-shortcuts sheet is open.
    pub(super) sheet_open: bool,
    /// The context menu is open, there.
    pub(super) context_menu_at: Option<MenuAt>,
    /// The open palette's field and what it searches, from the first frame it shows in.
    palette: Option<Palette>,
    /// The open sheet's keyboard and rows, from the first frame it shows in.
    sheet: Option<Sheet>,
    /// The open context menu's popup, from the first frame it shows in.
    context_menu: Option<ContextPopup>,
    /// The keyboard of each closed since the last frame: the canvas takes it back with the next
    /// one, if one of them still has it.
    closed: Vec<FocusHandle>,
}

/// The palette's search field and what it searches.
struct Palette {
    query: Entity<InputState>,
    /// The field's keyboard, kept to give back to the canvas.
    keyboard: FocusHandle,
    /// The main menu's commands, as they were when the palette opened.
    actions: Vec<ActionItem>,
    /// The query last searched for, and its best matches ([`Palette::matches`]).
    searched: Option<(String, Vec<ActionItem>)>,
    /// The match Enter runs: the best one, after each change of the query.
    highlight: usize,
    _query_changed: Subscription,
}

/// The sheet's keyboard and rows.
struct Sheet {
    focus: FocusHandle,
    groups: Vec<ShortcutGroup>,
}

/// The context menu's popup, its keyboard, kept to give back to the canvas (a popup can't be read
/// while one of its items runs), and what hears it close.
struct ContextPopup {
    menu: Entity<PopupMenu>,
    keyboard: FocusHandle,
    _closed: Subscription,
}

impl Menus {
    pub(super) fn any_open(&self) -> bool {
        self.palette_open || self.sheet_open || self.context_menu_at.is_some()
    }
}

impl BoardView {
    /// The keyboard of the palette, the shortcuts sheet or the context menu open over the canvas,
    /// if one is and has been drawn: what takes the keyboard in the canvas's place when the user
    /// comes back to it ([`BoardView::take_keyboard`]).
    pub(super) fn menu_keyboard(&self) -> Option<FocusHandle> {
        let menus = &self.menus;
        let palette = (menus.palette.as_ref()).map(|palette| palette.keyboard.clone());
        let sheet = (menus.sheet.as_ref()).map(|sheet| sheet.focus.clone());
        let context_menu = (menus.context_menu.as_ref()).map(|menu| menu.keyboard.clone());
        palette.or(sheet).or(context_menu)
    }
}

impl BoardView {
    /// Runs item `id` of the main menu, the context menu or the Actions palette, as a click on it
    /// does (spec, "Menu policy"). The menu open closes, and the canvas takes the keyboard. A text
    /// being typed in on the canvas ends, unless the command acts on it: the Text menu's commands,
    /// and copy and paste ([`Self::end_text_edit`], ruling R-TEXTEDIT). Then:
    /// - Copy, cut and paste go the keyboard's way ([`Self::run_clipboard`], ruling P6-C4): the
    ///   context menu's Paste here pastes where the menu opened.
    /// - A command Sawit carries out itself (import, export, save, copy as) is carried out by the
    ///   canvas, as its shortcut is ([`commands::run_ui`], ruling P6-C1).
    /// - Any other runs in the engine ([`Self::choose`]), and a refusal says why, in a toast.
    ///
    /// Last, the canvas carries out what the command asked of it, as after every input.
    pub fn run_menu(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let here = (id == "edit.paste")
            .then(|| self.menus.context_menu_at.map(|at| at.page))
            .flatten();
        self.close_menus(cx);
        window.focus(&self.focus, cx);
        if !self.end_text_edit(Some(id), cx) {
            return;
        }
        if policy::is_clipboard(id) {
            self.run_clipboard(id, here, false, cx);
        } else if policy::is_ui(id) {
            commands::run_ui(self, id, window, cx);
        } else {
            self.choose(id, json!({}), cx);
        }
        self.handle_ui_requests(Vec::new(), cx);
    }

    /// Opens the Actions palette, or closes it when it is open: what `app.actions` (⌘K, ⌘/) asks
    /// of the canvas (`palette.actions`), as FigCraft's app does.
    pub(super) fn toggle_palette(&mut self, cx: &mut Context<Self>) {
        let open = !self.menus.palette_open;
        self.close_menus(cx);
        self.menus.palette_open = open;
        cx.notify();
    }

    /// Opens the shortcuts sheet, or closes it when it is open: what `help.shortcuts` (⌃⇧?) asks
    /// of the canvas (`panel.shortcuts`), as FigCraft's app does.
    pub(super) fn toggle_sheet(&mut self, cx: &mut Context<Self>) {
        let open = !self.menus.sheet_open;
        self.close_menus(cx);
        self.menus.sheet_open = open;
        cx.notify();
    }

    /// Opens the context menu at `position` in the window, over page point `page`.
    pub(super) fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        page: (f64, f64),
        cx: &mut Context<Self>,
    ) {
        self.close_menus(cx);
        self.menus.context_menu_at = Some(MenuAt { position, page });
        cx.notify();
    }

    /// Opens the context menu from the keyboard: what `view.contextMenu` (⇧F10) asks of the canvas
    /// (`contextMenu`, ruling P6-C17). It is the selection's menu, 8 px under the selection's
    /// bottom left corner as in FigCraft, or the empty canvas's, in the middle of the canvas; there
    /// too when the corner is out of sight.
    pub(super) fn open_context_menu_from_keys(&mut self, cx: &mut Context<Self>) {
        let Some(ds) = self.board().and_then(|board| board.session.ds().ok()) else {
            return;
        };
        let view = &ds.view;
        let under = guarded("ui.contextMenu", || {
            let selected = ds
                .selection
                .iter()
                .filter_map(|id| ds.doc.world_bounds(*id));
            let bounds = selected.reduce(|a, b| a.union(b))?;
            let (x, y) = view.page_to_screen(bounds.x0, bounds.y1);
            Some((x, y + UNDER_SELECTION))
        });
        let in_sight =
            |&(x, y): &(f64, f64)| (0.0..=view.vw).contains(&x) && (0.0..=view.vh).contains(&y);
        let (x, y) = under
            .filter(in_sight)
            .unwrap_or((view.vw / 2.0, view.vh / 2.0));
        let page = view.screen_to_page(x, y);
        let position = self.bounds.origin + point(px(x as f32), px(y as f32));
        self.open_context_menu(position, page, cx);
    }

    /// Closes the palette, the sheet and the context menu, whichever is open. The keyboard it has
    /// goes back to the canvas with the next frame ([`Self::render_menus`]).
    pub(super) fn close_menus(&mut self, cx: &mut Context<Self>) {
        let menus = &mut self.menus;
        if let Some(palette) = menus.palette.take() {
            menus.closed.push(palette.keyboard);
        }
        if let Some(sheet) = menus.sheet.take() {
            menus.closed.push(sheet.focus);
        }
        if let Some(popup) = menus.context_menu.take() {
            menus.closed.push(popup.keyboard);
        }
        if menus.any_open() {
            menus.palette_open = false;
            menus.sheet_open = false;
            menus.context_menu_at = None;
            cx.notify();
        }
    }

    /// The answer to `op`, a control op, when it is the key Escape and the palette, the sheet or
    /// the context menu is open: that closes, as with the key in the window, and the canvas never
    /// gets the key (ruling P6-C7). `None` for any other op, which the canvas runs.
    pub(super) fn close_menus_for_op(
        &mut self,
        op: &Value,
        cx: &mut Context<Self>,
    ) -> Option<Value> {
        let text = |name: &str| op.get(name).and_then(Value::as_str);
        let held = |name: &str| op.get(name).and_then(Value::as_bool).unwrap_or(false);
        let escape = text("op") == Some("key")
            && text("key") == Some("Escape")
            && !["shift", "alt", "cmd", "ctrl"].into_iter().any(held);
        if !escape || !self.menus.any_open() {
            return None;
        }
        self.close_menus(cx);
        Some(json!({"ok": true, "handled": true}))
    }

    /// What of the palette, the sheet and the context menu is open, as this frame draws it over
    /// the canvas tab, each built with the first frame it shows in. First the canvas takes back
    /// the keyboard of those closed since the last frame, if one of them still has it or nothing
    /// has it (ruling P6-C3); then the one that opened takes it.
    pub(super) fn render_menus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let closed = std::mem::take(&mut self.menus.closed);
        let lost = window.focused(cx).is_none()
            || closed
                .iter()
                .any(|focus| focus.contains_focused(window, cx));
        if !closed.is_empty() && lost {
            window.focus(&self.focus, cx);
        }
        [
            self.render_palette(window, cx),
            self.render_sheet(window, cx),
            self.render_context_menu(window, cx),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// The sheet's `groups` with each command's keys as the keymap binds them now
/// ([`super::command_keys_all`]), [`SHEET_KEYS`] at most: a row whose keys are all taken out, and
/// a group left with no row, go.
pub(super) fn live_sheet(groups: Vec<ShortcutGroup>, cx: &mut App) -> Vec<ShortcutGroup> {
    groups
        .into_iter()
        .filter_map(|mut group| {
            group.rows.retain_mut(|row| {
                let mut keys = super::command_keys_all(row.id, cx);
                keys.truncate(SHEET_KEYS);
                row.keys = keys.join(" or ");
                !row.keys.is_empty()
            });
            (!group.rows.is_empty()).then_some(group)
        })
        .collect()
}

/// The palette's `items` with each command's keys as the keymap binds them now
/// ([`super::command_keys`]).
pub(super) fn live_actions(items: &mut [ActionItem], cx: &mut App) {
    for item in items {
        item.shortcut = super::command_keys(item.id, cx);
    }
}

/// `keys`, a key and its modifiers as the menus write them (`⇧⌘K`), drawn as a key, the way
/// gpui-kit's `Kbd` draws one. `Kbd` itself takes a GPUI keystroke, and writes some keys otherwise
/// than FigCraft does (ruling P6-C15).
fn keycap(keys: impl Into<SharedString>, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex_shrink_0()
        .min_w_5()
        .px_1()
        .py_0p5()
        .rounded(theme.radius * 0.5)
        .line_height(relative(1.))
        .text_center()
        .text_xs()
        .text_color(theme.muted_foreground)
        .bg(theme.muted)
        .child(keys.into())
}

/// `model()`, built behind the engine's guard as every model a frame builds is (D15): a panic in
/// it is logged, and leaves the empty `T`, with no toast.
fn guarded<T: Default>(name: &str, model: impl FnOnce() -> T) -> T {
    guard::run(name, || Ok(model())).unwrap_or_default()
}

#[cfg(test)]
mod tests;
