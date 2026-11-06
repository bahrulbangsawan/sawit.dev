//! The prototype player (Present, `proto.present`, the Prototype tab's buttons): the frame on show
//! alone over the canvas, fitted to the room, with an overlay over it when one is open. A click on
//! a layer that reacts to clicks runs its first action (`sawit_board::design::prototype::Player`);
//! Back, ← and Escape go back or close it.

use std::sync::Arc;

use figcraft_doc::{Document, NodeId};
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::prototype::{Played, Player};

use super::BoardView;

/// The scale the frames are rendered at, for a sharp picture on a Retina screen.
const RENDER_SCALE: f64 = 2.0;
const PAD: f32 = 32.;
const BAR: f32 = 44.;

/// The player while it shows.
pub(super) struct PlayerUi {
    player: Player,
    /// The frame on show and its overlay, rendered, each with its box on the page.
    pictures: Vec<(Arc<RenderImage>, [f64; 4])>,
    /// The document the pictures are of.
    doc: Option<Arc<Document>>,
    focus: FocusHandle,
    /// It takes the keyboard with the first frame it is drawn in.
    focus_pending: bool,
    /// Where the frame was drawn last, in the window, and its page box: a click maps through it.
    drawn: Option<(Bounds<Pixels>, [f64; 4])>,
}

/// `id` rendered, as an image for GPUI, and its box on the page.
fn picture(doc: &Document, id: NodeId) -> Option<(Arc<RenderImage>, [f64; 4])> {
    let b = doc.world_bounds(id)?;
    let pixmap = figcraft_render::render_node(doc, id, RENDER_SCALE)?;
    let (w, h) = (pixmap.width(), pixmap.height());
    // Premultiplied RGBA over white, as BGRA.
    let mut bgra = Vec::with_capacity(pixmap.data().len());
    for px in pixmap.data().as_chunks::<4>().0 {
        let a = u16::from(px[3]);
        let over = |c: u8| (u16::from(c) + (255 - a)).min(255) as u8;
        bgra.extend_from_slice(&[over(px[2]), over(px[1]), over(px[0]), 255]);
    }
    let pixels = image::RgbaImage::from_raw(w, h, bgra)?;
    Some((
        Arc::new(RenderImage::new([image::Frame::new(pixels)])),
        [b.x0, b.y0, b.x1, b.y1],
    ))
}

impl BoardView {
    /// Presents the prototype from `start` (a frame, or a layer in one), or from the selected
    /// frame, the page's first flow, or its first frame.
    pub(super) fn open_player(&mut self, start: Option<NodeId>, cx: &mut Context<Self>) {
        let Some(ds) = self.board().and_then(|b| b.session.ds().ok()) else {
            return;
        };
        let doc = ds.doc.clone();
        let start = start
            .or_else(|| ds.selection.first().copied())
            .or_else(|| {
                sawit_board::design::prototype::flows(&doc, ds.page)
                    .first()
                    .map(|f| f.0)
            })
            .or_else(|| doc.top_frames(ds.page).first().copied());
        let Some(player) = start.and_then(|s| Player::new(&doc, s)) else {
            self.show_toast("There's no frame to present", cx);
            return;
        };
        self.close_menus(cx);
        self.player = Some(PlayerUi {
            player,
            pictures: Vec::new(),
            doc: None,
            focus: cx.focus_handle(),
            focus_pending: true,
            drawn: None,
        });
        cx.notify();
    }

    pub(super) fn close_player(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.player.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn player_click(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(doc) = self
            .board()
            .and_then(|b| b.session.ds().ok())
            .map(|ds| ds.doc.clone())
        else {
            return;
        };
        let Some(ui) = self.player.as_mut() else {
            return;
        };
        let Some((bounds, page)) = ui.drawn else {
            return;
        };
        let fx = f64::from((at.x - bounds.origin.x) / bounds.size.width);
        let fy = f64::from((at.y - bounds.origin.y) / bounds.size.height);
        if !(0.0..=1.0).contains(&fx) || !(0.0..=1.0).contains(&fy) {
            return;
        }
        let x = page[0] + fx * (page[2] - page[0]);
        let y = page[1] + fy * (page[3] - page[1]);
        match ui.player.click(&doc, x, y) {
            Played::Moved => {
                ui.doc = None;
                cx.notify();
            }
            Played::Url(url) => cx.open_url(&url),
            Played::Nothing => {}
        }
    }

    fn player_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ui) = self.player.as_mut() else {
            return;
        };
        if ui.player.overlay.take().is_some() {
            ui.doc = None;
        } else if let Some(back) = ui.player.history.pop() {
            ui.player.frame = back;
            ui.doc = None;
        } else {
            self.close_player(window, cx);
            return;
        }
        cx.notify();
    }
}

/// The player, while it shows, over the whole canvas tab.
pub(super) fn render_player(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Option<AnyElement> {
    let doc = view.board()?.session.ds().ok()?.doc.clone();
    let ui = view.player.as_mut()?;
    if std::mem::take(&mut ui.focus_pending) {
        window.focus(&ui.focus, cx);
    }
    let fresh = ui.doc.as_ref().is_some_and(|d| Arc::ptr_eq(d, &doc));
    if !fresh {
        ui.pictures = std::iter::once(ui.player.frame)
            .chain(ui.player.overlay)
            .filter_map(|id| picture(&doc, id))
            .collect();
        ui.doc = Some(doc.clone());
    }
    let name = doc
        .node(ui.player.frame)
        .map(|n| n.name.clone())
        .unwrap_or_default();
    let pictures = ui.pictures.clone();
    let can_back = !ui.player.history.is_empty() || ui.player.overlay.is_some();
    let entity = cx.entity().downgrade();
    let stage = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let Some((frame, page)) = pictures.first() else {
                return;
            };
            let (pw, ph) = ((page[2] - page[0]) as f32, (page[3] - page[1]) as f32);
            let room = (
                f32::from(bounds.size.width) - 2.0 * PAD,
                f32::from(bounds.size.height) - 2.0 * PAD,
            );
            let scale = (room.0 / pw).min(room.1 / ph).clamp(0.01, 2.0);
            let size = size(px(pw * scale), px(ph * scale));
            let origin = point(
                bounds.origin.x + (bounds.size.width - size.width) / 2.0,
                bounds.origin.y + (bounds.size.height - size.height) / 2.0,
            );
            let at = Bounds::new(origin, size);
            let _ = window.paint_image(at, at, Corners::default(), frame.clone(), 0, false);
            for (image, b) in pictures.iter().skip(1) {
                let x = origin.x + px(((b[0] - page[0]) as f32) * scale);
                let y = origin.y + px(((b[1] - page[1]) as f32) * scale);
                let s = size_of(b, scale);
                let o = Bounds::new(point(x, y), s);
                let _ = window.paint_image(o, o, Corners::default(), image.clone(), 0, false);
            }
            let page = *page;
            let _ = entity.update(cx, |view, _| {
                if let Some(ui) = view.player.as_mut() {
                    ui.drawn = Some((at, page));
                }
            });
        },
    )
    .size_full();
    let ui = view.player.as_ref()?;
    Some(
        v_flex()
            .id("player")
            .debug_selector(|| "player".into())
            .occlude()
            .absolute()
            .inset_0()
            .bg(gpui_kit::rgb(0x1e1e1e))
            .track_focus(&ui.focus)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => view.close_player(window, cx),
                    "left" | "backspace" => view.player_back(window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                h_flex()
                    .h(px(BAR))
                    .px_3()
                    .gap_2()
                    .items_center()
                    .text_color(gpui_kit::rgb(0xffffff))
                    .child(div().flex_1().truncate().child(name))
                    .child(
                        Button::new("player-back")
                            .ghost()
                            .small()
                            .label("Back")
                            .disabled(!can_back)
                            .on_click(
                                cx.listener(|view, _, window, cx| view.player_back(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("player-close")
                            .ghost()
                            .small()
                            .label("Close")
                            .on_click(
                                cx.listener(|view, _, window, cx| view.close_player(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("player-stage")
                    .flex_1()
                    .min_h_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, event: &MouseDownEvent, _, cx| {
                            view.player_click(event.position, cx)
                        }),
                    )
                    .child(stage),
            )
            .into_any_element(),
    )
}

fn size_of(b: &[f64; 4], scale: f32) -> gpui_kit::Size<Pixels> {
    size(
        px(((b[2] - b[0]) as f32) * scale),
        px(((b[3] - b[1]) as f32) * scale),
    )
}
