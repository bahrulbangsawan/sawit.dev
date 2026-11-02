//! Drawing the canvas's tab: the top bar, the panels and the canvas between them, the chrome
//! floating over the canvas, and the menus over all of it.

use gpui_kit::*;

use super::{
    BoardView, context_bar, design_panel, events, layers_panel, paint, screens, text_input,
    toolbar, top_bar,
};
use crate::states::Placeholder;

impl Render for BoardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Asked for by the user's way in: the keyboard comes with this frame, which draws the
        // canvas (a canvas that can't be opened has nothing to take it).
        let take_keyboard = std::mem::take(&mut self.wants_keyboard);
        if let Err(error) = &self.board {
            return Placeholder::error("The canvas couldn't be opened.")
                .detail(error.clone())
                .into_any_element();
        }
        self.watch_keyboard(window, cx);
        if take_keyboard {
            let to = self.menu_keyboard().unwrap_or_else(|| self.focus.clone());
            window.focus(&to, cx);
        }
        let chrome = self.settle_chrome();
        // The keys the canvas takes while it edits text are bound in a context of their own.
        let key_context = if self.editing_text() {
            "Board BoardText"
        } else {
            "Board"
        };
        let top = chrome.then(|| top_bar::render_top_bar(self, window, cx));
        // The panels go with the rest of the chrome while the UI is hidden, and come back with it
        // (rulings P4-C8, P5-C18).
        let layers = (chrome && self.show_layers)
            .then(|| layers_panel::render_layers_panel(self, window, cx));
        let canvas = self.render_canvas(chrome, window, cx);
        let design = (chrome && self.show_design)
            .then(|| design_panel::render_design_panel(self, window, cx));
        // Over all of it, whether the UI is hidden or not (ruling P6-C6).
        let menus = self.render_menus(window, cx);
        // A colour picker whose swatch this frame left out ends its scrub.
        self.settle_colour_scrub(cx);
        div()
            .id("board")
            .key_context(key_context)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(Self::on_command))
            .on_key_up(cx.listener(Self::on_key_up))
            .children(top)
            // The canvas is a container of its own between the panels, holding the toolbar and the
            // context bar, so their places and the pointer's stay the canvas's (ruling P4-C14).
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .children(layers)
                    .child(canvas)
                    .children(design),
            )
            .children(menus)
            .into_any_element()
    }
}

impl BoardView {
    /// The canvas, under the top bar and between the layers panel and the Design panel: the
    /// document as rendered and its overlay, and over them, while the `chrome` shows, the context
    /// bar and the toolbar, then the toast. The pointer on the canvas drives the tools
    /// ([`events`]); the chrome floating over it takes its own clicks, so none of them reaches the
    /// tools.
    fn render_canvas(
        &mut self,
        chrome: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        let painted = this.clone();
        let cursor = self.cursor();
        // A screen selected alone has its own bar, with Preview, the downloads and Send to Agent.
        let screen_bar = chrome
            .then(|| screens::render_screen_bar(self, cx))
            .flatten();
        let over_selection = (chrome && screen_bar.is_none())
            .then(|| context_bar::render_context_bar(self, window, cx))
            .flatten();
        let preview = screens::render_preview(self, cx);
        let tools = chrome.then(|| toolbar::render_toolbar(self, window, cx));
        let toast = self.render_toast(chrome, cx);
        let agents = self.render_agents(cx);
        div()
            .id("board-canvas")
            .role(Role::Canvas)
            .aria_label("Canvas")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .cursor(cursor)
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| this.on_scroll(event, cx)))
            .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| this.on_pinch(event, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            // A key pressed with the pointer resting on the canvas isn't the pointer leaving it.
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .on_hover(cx.listener(Self::on_hover))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        this.update(cx, |this, cx| {
                            this.web_bench_step(window, cx);
                            let started = std::time::Instant::now();
                            let painting = this.prepaint(bounds, window, cx);
                            this.web_prepaint_took(started);
                            painting
                        })
                        .ok()
                        .flatten()
                    },
                    move |bounds, painting, window, cx| {
                        events::track_drags(&painted, window);
                        design_panel::track_scrub(&painted, window);
                        text_input::handle_input(&painted, bounds, window, cx);
                        if let Some(painting) = painting {
                            paint::paint(bounds, painting, window, cx);
                        }
                    },
                )
                .size_full(),
            )
            .children(agents)
            .children(screen_bar)
            .children(over_selection)
            .children(tools)
            .children(preview)
            .children(toast)
    }

    /// Settles the chrome before it is drawn. A flyout or the picker that is open closes while
    /// the UI is hidden, and once another kind of file is on show than the chrome was drawn for
    /// (a board became a design file, or the other way round), whose tools aren't the ones it
    /// belonged to (Review Focus 4). Whether the chrome shows: not while the UI is hidden, nor
    /// without a document.
    fn settle_chrome(&mut self) -> bool {
        let Some(board) = self.board() else {
            return false;
        };
        let kind = board.kind();
        let hidden = board.session.ds().is_ok_and(|ds| ds.view.ui_hidden);
        let other_kind = self
            .shown_kind
            .replace(kind)
            .is_some_and(|shown| shown != kind);
        if hidden || other_kind {
            self.open_flyout = None;
            self.picker = None;
        }
        !hidden
    }
}
