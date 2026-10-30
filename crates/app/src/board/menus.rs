//! The canvas's menus (spec D8, "Menu policy", "Context menu"): the main menu, the context menu,
//! the Actions palette and the keyboard-shortcuts sheet. `sawit_board` lists what they hold
//! ([`menu`], [`actions`], [`shortcuts`]); this draws it, and runs the command picked through
//! [`BoardView::run_menu`].
//!
//! - **The main menu** drops from the top bar's ≡ button ([`super::top_bar`]): FigCraft's menu
//!   catalog under Sawit's policy, its submenus nested, each item with its keys on the right.
//! - **The context menu** opens where the right button goes down on the canvas, once what is there
//!   is selected ([`BoardView::on_right_down`]), and under the selection for ⇧F10
//!   (`view.contextMenu`). Its Paste here pastes where it opened.
//! - **The Actions palette** (⌘K, ⌘/) searches the main menu's commands and lists the best
//!   [`PALETTE_RESULTS`]. Up and Down move the highlight, Enter runs it, and Escape or a press
//!   outside closes the palette, as in FigCraft.
//! - **The shortcuts sheet** (⌃⇧?, Help ▸ Keyboard shortcuts) lists FigCraft's keys in a column per
//!   group. Escape or a press outside closes it.
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

use figcraft_engine::guard;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::actions::{self, ActionItem};
use sawit_board::menu::{self, MenuNode};
use sawit_board::policy;
use sawit_board::shortcuts::{self, ShortcutGroup};
use serde_json::{Value, json};

use super::BoardView;

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
    fn any_open(&self) -> bool {
        self.palette_open || self.sheet_open || self.context_menu_at.is_some()
    }
}

impl Palette {
    /// The commands that match the query, best first: [`PALETTE_RESULTS`] at most.
    fn matches(&self, cx: &App) -> Vec<ActionItem> {
        let query = self.query.read(cx).value();
        let mut found = actions::search(&self.actions, &query);
        found.truncate(PALETTE_RESULTS);
        found
    }
}

impl BoardView {
    /// Runs item `id` of the main menu, the context menu or the Actions palette, as a click on it
    /// does (spec, "Menu policy"). The menu open closes, and the canvas takes the keyboard. Then:
    /// - Copy, cut and paste go the keyboard's way ([`Self::run_clipboard`], ruling P6-C4): the
    ///   context menu's Paste here pastes where the menu opened.
    /// - A command Sawit carries out itself (import, export, save, copy as) says it is coming, as
    ///   its shortcut does, for now (ruling P6-C1).
    /// - Any other runs in the engine ([`Self::choose`]), and a refusal says why, in a toast.
    ///
    /// Last, the canvas carries out what the command asked of it, as after every input.
    pub fn run_menu(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let here = (id == "edit.paste")
            .then(|| self.menus.context_menu_at.map(|at| at.page))
            .flatten();
        self.close_menus(cx);
        window.focus(&self.focus, cx);
        if policy::is_clipboard(id) {
            self.run_clipboard(id, here, cx);
        } else if policy::is_ui(id) {
            self.run_ui_command(id, cx);
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

    /// The palette, centred under the top of the tab, over a backdrop that takes the presses
    /// around it: its search field, which has the keyboard, and its best matches.
    fn render_palette(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.menus.palette_open {
            return None;
        }
        if self.menus.palette.is_none() {
            let actions = (self.board())
                .map(|board| guarded("ui.actions", || actions::all_actions(&board.session)))
                .unwrap_or_default();
            let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search actions"));
            let query_changed =
                cx.subscribe_in(&query, window, |view, _, event: &InputEvent, _, cx| {
                    if let (InputEvent::Change, Some(palette)) = (event, &mut view.menus.palette) {
                        palette.highlight = 0;
                        cx.notify();
                    }
                });
            query.update(cx, |query, cx| query.focus(window, cx));
            let keyboard = query.focus_handle(cx);
            self.menus.palette = Some(Palette {
                query,
                keyboard,
                actions,
                highlight: 0,
                _query_changed: query_changed,
            });
        }
        let palette = self.menus.palette.as_ref()?;
        let rows: Vec<AnyElement> = (palette.matches(cx).iter().enumerate())
            .map(|(index, item)| palette_row(index, item, index == palette.highlight, cx))
            .collect();
        let theme = cx.theme();
        let list = if rows.is_empty() {
            div()
                .px_3()
                .py_2()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("No matching actions")
        } else {
            div().flex().flex_col().p_1().gap_px().children(rows)
        };
        let field = Input::new(&palette.query)
            .prefix(Icon::new(SEARCH_ICON).text_color(theme.muted_foreground))
            .appearance(false);
        Some(
            v_flex()
                .id("board-palette-backdrop")
                // The presses around the palette are its own: one closes it, and none reaches
                // what is under it.
                .occlude()
                .absolute()
                .inset_0()
                .items_center()
                .pt(px(PALETTE_TOP))
                .child(
                    v_flex()
                        .id("board-palette")
                        .debug_selector(|| "board-palette".into())
                        .w(px(PALETTE_WIDTH))
                        .max_w_full()
                        .overflow_hidden()
                        .rounded(theme.radius_lg)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .on_key_down(cx.listener(Self::palette_key))
                        .on_mouse_down_out(cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            view.close_menus(cx);
                        }))
                        .child(
                            div()
                                .px_2()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(field),
                        )
                        .child(list),
                )
                .into_any_element(),
        )
    }

    /// A key pressed in the palette: Up and Down move the highlight, stopping at the first and the
    /// last match, Enter runs it, and Escape closes the palette. Any other key goes on to its
    /// search field.
    fn palette_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        match keystroke.key.as_str() {
            "up" => self.move_palette_highlight(-1, cx),
            "down" => self.move_palette_highlight(1, cx),
            "enter" => {
                if let Some(index) = self.menus.palette.as_ref().map(|p| p.highlight) {
                    self.run_palette_match(index, window, cx);
                }
            }
            "escape" => self.close_menus(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Highlights the palette's match `index`: the one Enter runs.
    fn highlight_palette_match(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(palette) = &mut self.menus.palette
            && palette.highlight != index
        {
            palette.highlight = index;
            cx.notify();
        }
    }

    /// Moves the palette's highlight by `step` matches, stopping at the first and the last.
    fn move_palette_highlight(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(palette) = &self.menus.palette else {
            return;
        };
        let last = palette.matches(cx).len().saturating_sub(1);
        let index = palette.highlight.saturating_add_signed(step).min(last);
        self.highlight_palette_match(index, cx);
    }

    /// Runs the palette's match `index` as from the main menu ([`Self::run_menu`]), which closes
    /// the palette.
    fn run_palette_match(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = (self.menus.palette.as_ref())
            .and_then(|palette| palette.matches(cx).get(index).map(|item| item.id));
        if let Some(id) = chosen {
            self.run_menu(id, window, cx);
        }
    }

    /// The sheet, in the middle of the tab over a backdrop that takes the presses around it: a
    /// column for each group of keys, scrolling when they don't fit. It has the keyboard.
    fn render_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.menus.sheet_open {
            return None;
        }
        if self.menus.sheet.is_none() {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let groups = guarded("ui.shortcuts", shortcuts::sheet);
            self.menus.sheet = Some(Sheet { focus, groups });
        }
        let sheet = self.menus.sheet.as_ref()?;
        let theme = cx.theme();
        let columns: Vec<AnyElement> = (sheet.groups.iter())
            .map(|group| sheet_column(group, cx))
            .collect();
        Some(
            div()
                .id("board-sheet-backdrop")
                // The presses around the sheet are its own: one closes it, and none reaches what
                // is under it.
                .occlude()
                .absolute()
                .inset_0()
                .p_6()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::black().opacity(0.45))
                .child(
                    v_flex()
                        .id("board-sheet")
                        .debug_selector(|| "board-sheet".into())
                        .track_focus(&sheet.focus)
                        .w_full()
                        .max_w(px(1040.))
                        .max_h_full()
                        .overflow_hidden()
                        .rounded(theme.radius_lg)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                            let keystroke = &event.keystroke;
                            if keystroke.key == "escape" && !keystroke.modifiers.modified() {
                                view.close_menus(cx);
                                cx.stop_propagation();
                            }
                        }))
                        .on_mouse_down_out(cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            view.close_menus(cx);
                        }))
                        .child(
                            div()
                                .flex_shrink_0()
                                .px_4()
                                .py_3()
                                .border_b_1()
                                .border_color(theme.border)
                                .text_sm()
                                .font_medium()
                                .child("Keyboard shortcuts"),
                        )
                        .child(
                            div()
                                .id("board-sheet-groups")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .p_4()
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .items_start()
                                        .gap_x_8()
                                        .gap_y_6()
                                        .children(columns),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The context menu, where it opened, over everything. Its popup has the keyboard.
    fn render_context_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let at = self.menus.context_menu_at?;
        let menu = match &self.menus.context_menu {
            Some(popup) => popup.menu.clone(),
            None => {
                let nodes = (self.board())
                    .map(|board| guarded("ui.contextMenu", || menu::context_menu(&board.session)))
                    .unwrap_or_default();
                if nodes.is_empty() {
                    self.menus.context_menu_at = None;
                    return None;
                }
                let menu = to_popup(&nodes, cx.entity().downgrade(), window, cx);
                let closed =
                    cx.subscribe_in(&menu, window, |view, menu, _: &DismissEvent, _, cx| {
                        view.context_menu_closed(menu, cx);
                    });
                let keyboard = menu.focus_handle(cx);
                keyboard.focus(window, cx);
                self.menus.context_menu = Some(ContextPopup {
                    menu: menu.clone(),
                    keyboard,
                    _closed: closed,
                });
                menu
            }
        };
        let placed = anchored()
            .position(at.position)
            .snap_to_window_with_margin(px(8.))
            .child(menu);
        Some(
            deferred(placed)
                .with_priority(POPUP_PRIORITY)
                .into_any_element(),
        )
    }

    /// The context menu's popup `menu` closed itself (an item ran, Escape, a press outside). The
    /// menu is closed, unless another opened since, and the keyboard goes back to the canvas if
    /// the popup still has it.
    fn context_menu_closed(&mut self, menu: &Entity<PopupMenu>, cx: &mut Context<Self>) {
        if self
            .menus
            .context_menu
            .as_ref()
            .is_some_and(|popup| popup.menu == *menu)
        {
            self.close_menus(cx);
        }
    }
}

/// Fills `menu`, the one the top bar's ≡ button drops, with the main menu of `view`'s canvas as
/// it is now ([`menu::main_menu`]), its checks among it. Closed, it gives the canvas the keyboard.
pub(super) fn fill_main_menu(
    menu: PopupMenu,
    view: &WeakEntity<BoardView>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(canvas) = view.upgrade() else {
        return menu;
    };
    let (nodes, keyboard) = {
        let canvas = canvas.read(cx);
        let nodes = (canvas.board())
            .map(|board| guarded("ui.mainMenu", || menu::main_menu(&board.session)))
            .unwrap_or_default();
        (nodes, canvas.focus.clone())
    };
    add_items(menu.action_context(keyboard), &nodes, view, window, cx)
}

/// A popup of `nodes` for `view`'s canvas: an item runs its command ([`BoardView::run_menu`]),
/// and a submenu opens beside its item.
fn to_popup(
    nodes: &[MenuNode],
    view: WeakEntity<BoardView>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    PopupMenu::build(window, cx, |menu, window, cx| {
        add_items(menu, nodes, &view, window, cx)
    })
}

/// `menu` with `nodes` added: an item for each command, a popup for each submenu
/// ([`to_popup`]), and the lines between them. A long menu scrolls.
fn add_items(
    mut menu: PopupMenu,
    nodes: &[MenuNode],
    view: &WeakEntity<BoardView>,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    for node in nodes {
        menu = match node {
            MenuNode::Item {
                id,
                label,
                shortcut,
                checked,
                enabled,
            } => menu.item(
                menu_item(id, label, shortcut.as_deref(), view.clone())
                    .checked(*checked == Some(true))
                    .disabled(!*enabled),
            ),
            MenuNode::Submenu { label, children } => {
                let submenu = to_popup(children, view.clone(), window, cx);
                menu.item(PopupMenuItem::submenu(label.clone(), submenu))
            }
            MenuNode::Separator => menu.separator(),
        };
    }
    menu.scrollable(nodes.len() > SCROLLS_PAST)
}

/// The item of command `id` called `label`, which runs it in `view`'s canvas. One with keys shows
/// them on the right (ruling P6-C15).
fn menu_item(
    id: &'static str,
    label: &str,
    keys: Option<&str>,
    view: WeakEntity<BoardView>,
) -> PopupMenuItem {
    let item = match keys {
        Some(keys) => {
            let label = SharedString::from(label.to_owned());
            let keys = SharedString::from(keys.to_owned());
            PopupMenuItem::element(move |_, cx| {
                h_flex()
                    .debug_selector(move || format!("board-menu-{id}"))
                    .w_full()
                    .gap_6()
                    .justify_between()
                    .child(label.clone())
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(keys.clone()),
                    )
            })
        }
        None => PopupMenuItem::new(label.to_owned()),
    };
    item.on_click(move |_, window, cx| {
        let _ = view.update(cx, |view, cx| view.run_menu(id, window, cx));
    })
}

/// Row `index` of the palette's matches, `item`, `highlighted` when Enter runs it: its label, the
/// menu it is in (its path without the label, so the label shows once), and its keys. The pointer
/// moving over it highlights it, and a click runs it.
fn palette_row(
    index: usize,
    item: &ActionItem,
    highlighted: bool,
    cx: &Context<BoardView>,
) -> AnyElement {
    let id = item.id;
    let theme = cx.theme();
    h_flex()
        .id(("board-palette-row", index))
        .debug_selector(move || format!("board-palette-{id}"))
        .w_full()
        .h(px(PALETTE_ROW))
        .px_2()
        .gap_2()
        .rounded(theme.radius)
        .text_sm()
        .when(highlighted, |row| {
            row.bg(theme.accent).text_color(theme.accent_foreground)
        })
        .on_mouse_move(cx.listener(move |view, _: &MouseMoveEvent, _, cx| {
            view.highlight_palette_match(index, cx);
        }))
        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
            view.run_palette_match(index, window, cx);
        }))
        .child(div().min_w_0().truncate().child(item.label.clone()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(menu_part(&item.path).to_owned()),
        )
        .when_some(item.shortcut.clone(), |row, keys| {
            row.child(keycap(keys, cx))
        })
        .into_any_element()
}

/// The sheet's column of `group`: its name, then a row for each command, with its label and its
/// keys, each alternative a key of its own.
fn sheet_column(group: &ShortcutGroup, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let name = group.name.clone();
    let rows = group.rows.iter().map(|row| {
        let id = row.id;
        let keys = (row.keys.split(" or ")).map(|keys| keycap(keys.to_owned(), cx));
        h_flex()
            .debug_selector(move || format!("board-sheet-{id}"))
            .w_full()
            .gap_2()
            .justify_between()
            .text_sm()
            .child(div().min_w_0().truncate().child(row.label.clone()))
            .child(h_flex().flex_shrink_0().gap_1().children(keys))
    });
    v_flex()
        .debug_selector(move || format!("board-sheet-{name}"))
        .w(px(SHEET_COLUMN))
        .gap_1()
        .child(
            div()
                .pb_1()
                .text_xs()
                .font_medium()
                .text_color(theme.muted_foreground)
                .child(group.name.clone()),
        )
        .children(rows)
        .into_any_element()
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

/// The menus that `path`, an action's path (`Object › Group selection`), runs through: the path
/// without the action itself, which the palette's row names already. Empty for a path of one name.
fn menu_part(path: &str) -> &str {
    path.rsplit_once(PATH_SEPARATOR)
        .map_or("", |(menus, _)| menus)
}

/// `model()`, built behind the engine's guard as every model a frame builds is (D15): a panic in
/// it is logged, and leaves the empty `T`, with no toast.
fn guarded<T: Default>(name: &str, model: impl FnOnce() -> T) -> T {
    guard::run(name, || Ok(model())).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use figcraft_doc::NodeId;
    use figcraft_doc::geom::Rect;
    use gpui_kit::{
        Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, point, px,
    };
    use sawit_board::menu::context_menu;
    use serde_json::{Value, json};

    use super::super::BoardView;
    use super::super::tests::{
        Scratch, chrome_canvas, click, clipboard_turn, focus_canvas, layers, run_op, view_state,
    };
    use super::{MenuAt, menu_part};

    /// Puts a sticky note with its top left corner at page point `at`, selected, as an agent's
    /// command does, and returns it.
    fn sticky_at(view: &Entity<BoardView>, at: (f64, f64), cx: &mut TestAppContext) -> NodeId {
        let op = json!({
            "op": "command", "command": "board.sticky", "params": {"x": at.0, "y": at.1},
        });
        let made = run_op(view, op, cx);
        figcraft_engine::params::id_of(&made["result"]["id"]).unwrap()
    }

    /// The op that runs engine command `command` with no params.
    fn command(command: &str) -> Value {
        json!({"op": "command", "command": command})
    }

    fn selection(view: &Entity<BoardView>, cx: &TestAppContext) -> Vec<NodeId> {
        view.read_with(cx, |view, _| view.board().unwrap().session.selection())
    }

    /// The box of `layer` on the page.
    fn bounds_of(view: &Entity<BoardView>, layer: NodeId, cx: &TestAppContext) -> Rect {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            doc.world_bounds(layer).unwrap()
        })
    }

    /// Where page point `page` is in the window.
    fn on_screen(view: &Entity<BoardView>, page: (f64, f64), cx: &TestAppContext) -> Point<Pixels> {
        let canvas = view.read_with(cx, |view, _| view.bounds);
        let (x, y) = view_state(view, cx).page_to_screen(page.0, page.1);
        canvas.origin + point(px(x as f32), px(y as f32))
    }

    /// The middle of `layer` in the window.
    fn middle_of(view: &Entity<BoardView>, layer: NodeId, cx: &TestAppContext) -> Point<Pixels> {
        let bounds = bounds_of(view, layer, cx);
        let middle = ((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0);
        on_screen(view, middle, cx)
    }

    /// Whether `a` and `b` are the same place in the window, but for rounding.
    fn same_place(a: Point<Pixels>, b: Point<Pixels>) -> bool {
        (f32::from(a.x) - f32::from(b.x)).abs() < 0.01
            && (f32::from(a.y) - f32::from(b.y)).abs() < 0.01
    }

    /// A press and release of the right button at `at` in the window.
    fn right_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    }

    /// Runs menu item `id` as a click on it does.
    fn run_menu(view: &Entity<BoardView>, id: &'static str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| view.update(cx, |view, cx| view.run_menu(id, window, cx)));
    }

    /// Which of the palette, the sheet and the context menu are open.
    fn open_menus(view: &Entity<BoardView>, cx: &TestAppContext) -> (bool, bool, Option<MenuAt>) {
        view.read_with(cx, |view, _| {
            let menus = &view.menus;
            (menus.palette_open, menus.sheet_open, menus.context_menu_at)
        })
    }

    /// Nothing open: the palette, the sheet and the context menu are closed.
    const CLOSED: (bool, bool, Option<MenuAt>) = (false, false, None);

    fn canvas_has_keyboard(view: &Entity<BoardView>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| view.read(cx).focus.is_focused(window))
    }

    /// What is typed in the palette's search field, while it shows.
    fn palette_query(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<String> {
        view.read_with(cx, |view, cx| {
            let palette = view.menus.palette.as_ref()?;
            Some(palette.query.read(cx).value().to_string())
        })
    }

    /// Which of the palette's matches Enter runs.
    fn palette_highlight(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<usize> {
        view.read_with(cx, |view, _| {
            view.menus.palette.as_ref().map(|palette| palette.highlight)
        })
    }

    /// The tool the canvas's pointer events go to.
    fn tool(view: &Entity<BoardView>, cx: &TestAppContext) -> &'static str {
        view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
    }

    fn toast(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<String> {
        view.read_with(cx, |view, _| view.toast.as_ref().map(ToString::to_string))
    }

    #[gpui_kit::test]
    fn menu_runs_engine_command(cx: &mut TestAppContext) {
        let tmp = Scratch::new("menu-runs");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
        assert_eq!(selection(&view, &cx), [sticky]);
        run_menu(&view, "edit.duplicate", &mut cx);
        assert_eq!(layers(&view, &cx).len(), 2);
        assert_eq!(toast(&view, &cx), None);
        // A command Sawit carries out itself says it is coming, as its shortcut does, until Plan
        // 6 Task 3 carries it out (ruling P6-C1). The engine never runs it.
        run_menu(&view, "file.export", &mut cx);
        assert_eq!(toast(&view, &cx).as_deref(), Some("Coming soon"));
        assert_eq!(layers(&view, &cx).len(), 2);
    }

    #[gpui_kit::test]
    fn cmd_k_opens_palette(cx: &mut TestAppContext) {
        let tmp = Scratch::new("cmd-k");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("cmd-k");
        assert_eq!(open_menus(&view, &cx), (true, false, None));
        // It shows, and its search field has the keyboard (ruling P6-C3).
        assert!(cx.debug_bounds("board-palette").is_some());
        assert!(!canvas_has_keyboard(&view, &mut cx));
        assert_eq!(palette_query(&view, &cx).as_deref(), Some(""));
        // Escape closes it, and the canvas has the keyboard back.
        cx.simulate_keystrokes("escape");
        assert_eq!(open_menus(&view, &cx), CLOSED);
        assert!(cx.debug_bounds("board-palette").is_none());
        assert!(canvas_has_keyboard(&view, &mut cx));
        // ⌘/ opens it too: 480 px wide, centred in the tab, under its top bar.
        cx.simulate_keystrokes("cmd-/");
        assert_eq!(open_menus(&view, &cx), (true, false, None));
        let palette = cx.debug_bounds("board-palette").expect("the palette");
        assert_eq!(palette.size.width, px(480.));
        assert_eq!(palette.center().x, px(400.));
        assert_eq!(palette.top(), px(64.));
        // A press outside closes it, and the canvas under it never gets the press.
        run_op(&view, command("tool.sticky"), &mut cx);
        cx.simulate_click(point(px(20.), px(560.)), Modifiers::default());
        assert_eq!(open_menus(&view, &cx), CLOSED);
        assert!(layers(&view, &cx).is_empty());
    }

    #[gpui_kit::test]
    fn palette_enter_runs_first_result(cx: &mut TestAppContext) {
        let tmp = Scratch::new("palette-enter");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
        let at = bounds_of(&view, sticky, &cx);
        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("cmd-k d u p");
        assert_eq!(palette_query(&view, &cx).as_deref(), Some("dup"));
        // Duplicate matches best: it is first, and highlighted.
        assert!(cx.debug_bounds("board-palette-edit.duplicate").is_some());
        assert_eq!(palette_highlight(&view, &cx), Some(0));
        // The arrows move the highlight, and stop at the first match; the sticky stays put.
        cx.simulate_keystrokes("down");
        assert_eq!(palette_highlight(&view, &cx), Some(1));
        cx.simulate_keystrokes("up up");
        assert_eq!(palette_highlight(&view, &cx), Some(0));
        assert_eq!(bounds_of(&view, sticky, &cx), at);
        // Enter runs it: the sticky is duplicated, and the palette closes.
        cx.simulate_keystrokes("enter");
        assert_eq!(layers(&view, &cx).len(), 2);
        assert_eq!(open_menus(&view, &cx), CLOSED);
        assert!(cx.debug_bounds("board-palette").is_none());
        assert!(canvas_has_keyboard(&view, &mut cx));
        assert_eq!(toast(&view, &cx), None);
    }

    /// Ruling P6-C3: what is typed in the palette is its query, not the canvas's keys.
    #[gpui_kit::test]
    fn typing_in_palette_keeps_tool(cx: &mut TestAppContext) {
        let tmp = Scratch::new("palette-typing");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        focus_canvas(&view, &mut cx);
        let before = tool(&view, &cx);
        cx.simulate_keystrokes("cmd-k u p");
        assert_eq!(tool(&view, &cx), before);
        assert_eq!(palette_query(&view, &cx).as_deref(), Some("up"));
        // Closed, the palette gives the canvas the keyboard back, and its keys are its own again.
        cx.simulate_keystrokes("escape");
        assert!(canvas_has_keyboard(&view, &mut cx));
        cx.simulate_keystrokes("u");
        assert_eq!(tool(&view, &cx), "highlighter");
    }

    #[gpui_kit::test]
    fn right_click_selects_target(cx: &mut TestAppContext) {
        let tmp = Scratch::new("right-click");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
        run_op(&view, command("select.none"), &mut cx);
        assert!(selection(&view, &cx).is_empty());

        // On a sticky that isn't selected: it is, and the menu is the selection's.
        let on_sticky = middle_of(&view, sticky, &cx);
        right_click(&mut cx, on_sticky);
        assert_eq!(selection(&view, &cx), [sticky]);
        assert!(open_menus(&view, &cx).2.is_some());
        assert!(cx.debug_bounds("board-menu-edit.delete").is_some());

        // On a layer that is selected: the whole selection stays.
        let other = sticky_at(&view, (200.0, 0.0), &mut cx);
        let both =
            json!({"op": "command", "command": "select.set", "params": {"ids": [sticky, other]}});
        run_op(&view, both, &mut cx);
        right_click(&mut cx, on_sticky);
        assert_eq!(selection(&view, &cx), [sticky, other]);

        // On empty canvas, with stickies selected: nothing stays selected, and the menu is the
        // empty canvas's four items, where the pointer is (ruling P6-C10).
        let empty = on_screen(&view, (100.0, -150.0), &cx);
        right_click(&mut cx, empty);
        assert!(selection(&view, &cx).is_empty());
        let at = open_menus(&view, &cx).2.expect("a context menu");
        assert!(same_place(at.position, empty), "{at:?}");
        let items = view.read_with(&cx, |view, _| {
            context_menu(&view.board().unwrap().session).len()
        });
        assert_eq!(items, 4);
        for selector in [
            "board-menu-edit.paste",
            "board-menu-select.all",
            "board-menu-view.zoomFit",
            "board-menu-view.toggleUi",
        ] {
            assert!(
                cx.debug_bounds(selector).is_some(),
                "{selector} isn't drawn"
            );
        }
        assert!(cx.debug_bounds("board-menu-edit.delete").is_none());
    }

    /// Ruling P6-C4: the context menu's Paste here pastes the copy where the menu opened, through
    /// the keyboard's copy and paste.
    #[gpui_kit::test]
    fn paste_here_pastes_where_the_menu_opened(cx: &mut TestAppContext) {
        let _turn = clipboard_turn();
        let tmp = Scratch::new("paste-here");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        sticky_at(&view, (-100.0, 0.0), &mut cx);
        run_menu(&view, "edit.copy", &mut cx);
        // Right of the sticky, on empty canvas.
        let here = on_screen(&view, (150.0, 80.0), &cx);
        right_click(&mut cx, here);
        click(&mut cx, "board-menu-edit.paste");
        let [_, pasted] = layers(&view, &cx)[..] else {
            panic!("layers {:?}", layers(&view, &cx))
        };
        let bounds = bounds_of(&view, pasted, &cx);
        assert!(
            (bounds.x0 - 150.0).abs() < 1e-3 && (bounds.y0 - 80.0).abs() < 1e-3,
            "{bounds:?}"
        );
        assert_eq!(open_menus(&view, &cx), CLOSED);
        assert!(canvas_has_keyboard(&view, &mut cx));
        assert_eq!(toast(&view, &cx), None);
    }

    /// Ruling P6-C6: while the UI is hidden, the context menu and the palette still show, and the
    /// empty canvas's menu brings the UI back.
    #[gpui_kit::test]
    fn the_menus_show_while_the_ui_is_hidden(cx: &mut TestAppContext) {
        let tmp = Scratch::new("menus-ui-hidden");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let hidden = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| {
                view.board().unwrap().session.ds().unwrap().view.ui_hidden
            })
        };
        view.update(&mut cx, |view, cx| {
            view.choose("view.toggleUi", json!({}), cx)
        });
        cx.run_until_parked();
        assert!(hidden(&cx));
        assert!(cx.debug_bounds("board-top-bar").is_none());

        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("cmd-k");
        assert!(cx.debug_bounds("board-palette").is_some());
        cx.simulate_keystrokes("escape");

        let empty = on_screen(&view, (100.0, -150.0), &cx);
        right_click(&mut cx, empty);
        click(&mut cx, "board-menu-view.toggleUi");
        cx.run_until_parked();
        assert!(!hidden(&cx));
        assert!(cx.debug_bounds("board-top-bar").is_some());
    }

    /// Ruling P6-C7: an agent's Escape closes what is open, as the key does in the window, and
    /// the canvas never sees it.
    #[gpui_kit::test]
    fn a_key_ops_escape_closes_the_open_menu(cx: &mut TestAppContext) {
        let tmp = Scratch::new("escape-op");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
        let escape = json!({"op": "key", "key": "Escape"});
        for (open, shown) in [
            ("app.actions", "board-palette"),
            ("help.shortcuts", "board-sheet"),
            ("view.contextMenu", "board-menu-edit.delete"),
        ] {
            let reply = run_op(&view, command(open), &mut cx);
            assert_eq!(reply["ok"], json!(true), "{reply}");
            cx.run_until_parked();
            assert!(
                cx.debug_bounds(shown).is_some(),
                "{open}: {shown} isn't drawn"
            );
            let reply = run_op(&view, escape.clone(), &mut cx);
            assert_eq!(reply, json!({"ok": true, "handled": true}), "{open}");
            cx.run_until_parked();
            assert_eq!(open_menus(&view, &cx), CLOSED, "{open}");
            assert!(cx.debug_bounds(shown).is_none(), "{open}: {shown} is drawn");
            assert_eq!(selection(&view, &cx), [sticky], "{open}");
            assert!(canvas_has_keyboard(&view, &mut cx), "{open}");
        }
        // With nothing open, Escape is the canvas's: it deselects.
        let reply = run_op(&view, escape, &mut cx);
        assert_eq!(reply, json!({"ok": true, "handled": true}));
        assert!(selection(&view, &cx).is_empty());
    }

    /// Ruling P6-C17: ⇧F10 opens the context menu under the selection, or in the middle of the
    /// canvas with nothing selected.
    #[gpui_kit::test]
    fn the_context_menu_key_opens_it_under_the_selection(cx: &mut TestAppContext) {
        let tmp = Scratch::new("menu-key");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("shift-f10");
        let at = open_menus(&view, &cx).2.expect("a context menu");
        let bounds = bounds_of(&view, sticky, &cx);
        let corner = on_screen(&view, (bounds.x0, bounds.y1), &cx);
        assert!(
            same_place(at.position, corner + point(px(0.), px(8.))),
            "{at:?}"
        );
        assert!(cx.debug_bounds("board-menu-edit.delete").is_some());
        // The menu has the keyboard: Escape closes it and gives the keyboard back to the canvas.
        cx.simulate_keystrokes("escape");
        assert_eq!(open_menus(&view, &cx), CLOSED);
        assert!(canvas_has_keyboard(&view, &mut cx));

        run_op(&view, command("select.none"), &mut cx);
        cx.simulate_keystrokes("shift-f10");
        let at = open_menus(&view, &cx).2.expect("a context menu");
        let canvas = view.read_with(&cx, |view, _| view.bounds);
        assert!(
            same_place(at.position, canvas.center()),
            "{at:?} in {canvas:?}"
        );
        assert!(cx.debug_bounds("board-menu-view.toggleUi").is_some());
    }

    #[gpui_kit::test]
    fn the_main_menu_comes_first_and_runs_its_items(cx: &mut TestAppContext) {
        let tmp = Scratch::new("main-menu");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let bar = cx.debug_bounds("board-top-bar").expect("the top bar");
        let button = cx
            .debug_bounds("board-main-menu")
            .expect("the main menu's button");
        let zoom = cx.debug_bounds("board-zoom").expect("the zoom menu");
        assert!(
            button.left() - bar.left() < px(16.),
            "{button:?} in {bar:?}"
        );
        assert!(button.right() < zoom.left());
        // The keyboard walks it: Help is its last menu, and Keyboard shortcuts Help's one item.
        click(&mut cx, "board-main-menu");
        cx.run_until_parked();
        cx.simulate_keystrokes("up right enter");
        assert_eq!(open_menus(&view, &cx), (false, true, None));
        assert!(cx.debug_bounds("board-sheet").is_some());
    }

    #[gpui_kit::test]
    fn the_shortcuts_sheet_lists_the_keys_and_closes(cx: &mut TestAppContext) {
        let tmp = Scratch::new("sheet");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("ctrl-shift-?");
        assert_eq!(open_menus(&view, &cx), (false, true, None));
        // In the middle of the tab, 24 px from its edges at most: its rows scroll inside it.
        let sheet = cx.debug_bounds("board-sheet").expect("the sheet");
        assert_eq!(sheet.center(), point(px(400.), px(300.)));
        assert_eq!(sheet.size.width, px(800. - 48.));
        assert!(sheet.size.height <= px(600. - 48.), "{sheet:?}");
        assert!(cx.debug_bounds("board-sheet-Essential").is_some());
        assert!(cx.debug_bounds("board-sheet-app.actions").is_some());
        // Escape closes it, and gives the canvas the keyboard back.
        cx.simulate_keystrokes("escape");
        assert_eq!(open_menus(&view, &cx), CLOSED);
        assert!(cx.debug_bounds("board-sheet").is_none());
        assert!(canvas_has_keyboard(&view, &mut cx));
        // So does a press outside it, which the canvas under it never gets.
        let steps = view.read_with(&cx, |view, _| view.board().unwrap().session.undo_depth());
        run_op(&view, command("tool.sticky"), &mut cx);
        run_op(&view, command("help.shortcuts"), &mut cx);
        cx.run_until_parked();
        let sheet = cx.debug_bounds("board-sheet").expect("the sheet");
        let outside = point(sheet.left() - px(8.), sheet.top() - px(8.));
        cx.simulate_click(outside, Modifiers::default());
        assert_eq!(open_menus(&view, &cx), CLOSED);
        assert!(layers(&view, &cx).is_empty());
        assert_eq!(
            view.read_with(&cx, |view, _| view.board().unwrap().session.undo_depth()),
            steps
        );
    }

    #[test]
    fn a_palette_row_names_its_menu_beside_the_label() {
        assert_eq!(menu_part("Object › Group selection"), "Object");
        assert_eq!(menu_part("Edit › Copy as › Copy as CSS"), "Edit › Copy as");
        assert_eq!(menu_part("Keyboard shortcuts"), "");
    }
}
