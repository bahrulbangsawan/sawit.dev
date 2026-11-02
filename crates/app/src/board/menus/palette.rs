//! The Actions palette (⌘K, ⌘/): its search field over the main menu's commands, its best matches,
//! and the keys that move its highlight and run it.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::actions::{self, ActionItem};

use super::{
    PALETTE_RESULTS, PALETTE_ROW, PALETTE_TOP, PALETTE_WIDTH, PATH_SEPARATOR, Palette, SEARCH_ICON,
    guarded, keycap, live_actions,
};
use crate::board::BoardView;

impl Palette {
    /// The commands that match the query, best first: [`PALETTE_RESULTS`] at most. They are
    /// searched for again only once the query is another than the last time, not for each frame
    /// drawn and each key that moves the highlight.
    fn matches(&mut self, cx: &App) -> &[ActionItem] {
        let query = self.query.read(cx).value();
        let searched = (self.searched.take()).filter(|(searched, _)| searched.as_str() == &*query);
        let (_, found) = self.searched.insert(searched.unwrap_or_else(|| {
            let mut found = actions::search(&self.actions, &query);
            found.truncate(PALETTE_RESULTS);
            (query.to_string(), found)
        }));
        found
    }
}

impl BoardView {
    /// The palette, centred under the top of the tab, over a backdrop that takes the presses
    /// around it: its search field, which has the keyboard, and its best matches.
    pub(super) fn render_palette(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.menus.palette_open {
            return None;
        }
        if self.menus.palette.is_none() {
            let mut actions = (self.board())
                .map(|board| guarded("ui.actions", || actions::all_actions(&board.session)))
                .unwrap_or_default();
            live_actions(&mut actions, cx);
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
                searched: None,
                highlight: 0,
                _query_changed: query_changed,
            });
        }
        let palette = self.menus.palette.as_mut()?;
        let highlight = palette.highlight;
        let rows: Vec<AnyElement> = (palette.matches(cx).iter().enumerate())
            .map(|(index, item)| palette_row(index, item, index == highlight, cx))
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
        let Some(palette) = &mut self.menus.palette else {
            return;
        };
        let last = palette.matches(cx).len().saturating_sub(1);
        let index = palette.highlight.saturating_add_signed(step).min(last);
        self.highlight_palette_match(index, cx);
    }

    /// Runs the palette's match `index` as from the main menu ([`Self::run_menu`]), which closes
    /// the palette.
    fn run_palette_match(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = (self.menus.palette.as_mut())
            .and_then(|palette| palette.matches(cx).get(index).map(|item| item.id));
        if let Some(id) = chosen {
            self.run_menu(id, window, cx);
        }
    }
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

/// The menus that `path`, an action's path (`Object › Group selection`), runs through: the path
/// without the action itself, which the palette's row names already. Empty for a path of one name.
pub(super) fn menu_part(path: &str) -> &str {
    path.rsplit_once(PATH_SEPARATOR)
        .map_or("", |(menus, _)| menus)
}
