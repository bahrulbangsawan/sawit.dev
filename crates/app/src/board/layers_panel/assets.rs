//! The layers panel's Assets tab (View ▸ Panels ▸ Assets, `view.assets`): the file's components
//! and component sets, each a small picture of it and its name, which the filter narrows; a click
//! places an instance of it in the middle of the view (`instance.create`), selected. The pictures
//! are drawn once for each version of the document.

use std::collections::HashMap;
use std::sync::Arc;

use figcraft_doc::{Document, NodeId};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::component::swap_targets;
use serde_json::json;

use crate::board::BoardView;

/// The side of a picture.
const THUMB: f32 = 36.;

/// The pictures of the components, for one version of the document.
#[derive(Default)]
pub(in crate::board) struct Thumbnails {
    doc: Option<Arc<Document>>,
    images: HashMap<NodeId, Arc<RenderImage>>,
}

fn thumbnail(doc: &Document, id: NodeId) -> Option<Arc<RenderImage>> {
    let b = doc.world_bounds(id)?;
    let side = (b.x1 - b.x0).max(b.y1 - b.y0).max(1.0);
    let scale = (f64::from(THUMB) * 2.0 / side).min(4.0);
    let pixmap = figcraft_render::render_node(doc, id, scale)?;
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut bgra = Vec::with_capacity(pixmap.data().len());
    for px in pixmap.data().as_chunks::<4>().0 {
        let a = u16::from(px[3]);
        let over = |c: u8| (u16::from(c) + (255 - a)).min(255) as u8;
        bgra.extend_from_slice(&[over(px[2]), over(px[1]), over(px[0]), 255]);
    }
    let pixels = image::RgbaImage::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
}

impl BoardView {
    /// Places an instance of `component` in the middle of the view, selected.
    fn place_instance(&mut self, component: NodeId, cx: &mut Context<Self>) {
        let Some((x, y)) = self.board().and_then(|b| b.session.ds().ok()).map(|ds| {
            let view = &ds.view;
            view.screen_to_page(view.vw / 2.0, view.vh / 2.0)
        }) else {
            return;
        };
        self.choose(
            "instance.create",
            json!({"component": component, "x": x.round(), "y": y.round()}),
            cx,
        );
    }
}

/// The Assets tab's list: the components whose name holds `filter`.
pub(super) fn render_assets(
    view: &mut BoardView,
    filter: &str,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let Some(doc) = view
        .board()
        .and_then(|b| b.session.ds().ok())
        .map(|ds| ds.doc.clone())
    else {
        return div().into_any_element();
    };
    let fresh = view
        .thumbnails
        .doc
        .as_ref()
        .is_some_and(|d| Arc::ptr_eq(d, &doc));
    if !fresh {
        view.thumbnails = Thumbnails {
            doc: Some(doc.clone()),
            images: HashMap::new(),
        };
    }
    let needle = filter.trim().to_lowercase();
    let components: Vec<(NodeId, String)> = swap_targets(&doc)
        .into_iter()
        .filter(|(_, name)| needle.is_empty() || name.to_lowercase().contains(&needle))
        .collect();
    let theme = cx.theme().clone();
    if components.is_empty() {
        return div()
            .p_3()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(if needle.is_empty() {
                "No components yet. Select layers and Create component (⌥⌘K) to make one."
            } else {
                "No component matches."
            })
            .into_any_element();
    }
    let mut list = v_flex()
        .id("assets-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .p_1();
    for (id, name) in components {
        let image = match view.thumbnails.images.get(&id) {
            Some(image) => Some(image.clone()),
            None => {
                let made = thumbnail(&doc, id);
                if let Some(made) = &made {
                    view.thumbnails.images.insert(id, made.clone());
                }
                made
            }
        };
        let picture = match image {
            Some(image) => img(image)
                .size(px(THUMB))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div().size(px(THUMB)).into_any_element(),
        };
        list = list.child(
            h_flex()
                .id(SharedString::from(format!("asset-{}", id.0)))
                .gap_2()
                .p_1()
                .rounded(px(4.))
                .items_center()
                .hover(|s| s.bg(theme.secondary))
                .cursor_pointer()
                .child(
                    div()
                        .flex_shrink_0()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(gpui_kit::white())
                        .child(picture),
                )
                .child(div().flex_1().min_w_0().truncate().child(name))
                .on_click(cx.listener(move |view, _, _, cx| view.place_instance(id, cx))),
        );
    }
    list.into_any_element()
}
