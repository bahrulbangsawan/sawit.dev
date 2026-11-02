//! The canvas's frames: asked of the render thread before each paint, for the view and the
//! document as they are then; taken in as they come, if they show the page on show; and let go of
//! when the canvas goes off screen or closes.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use figcraft_doc::color::Color;
use figcraft_render::{Options, View};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_board::DocKind;
use sawit_board::overlay::{self, Composition};
use sawit_board::view::{self, PREVIEW_QUALITY, Viewport};

use super::paint::{self, Painting};
use super::worker::{Frame, Job};
use super::{BoardView, overlay_paint};

impl BoardView {
    /// The canvas went off screen, where it draws no frames: it lets go of what it keeps to draw
    /// the next ones (CANVAS-UI-9). The textures of its newest frame and of those replaced since
    /// its last paint leave the sprite atlas of `window`, where it was painted, and the render
    /// thread forgets the render it kept ([`Worker::trim`]): each is as large as the canvas and
    /// its overscan, at the screen's density. So the next render after the tab comes back is
    /// drawn whole. The newest frame's image stays, so that the canvas shows at once when its tab
    /// comes back, and that paint puts its texture back.
    pub(super) fn let_go_of_frames(&mut self, window: &mut Window) {
        let painted_here = self.window == Some(window.window_handle());
        let images = (self.frame.as_ref().map(|frame| frame.image.clone()))
            .into_iter()
            .chain(self.stale.drain(..));
        for image in images {
            if painted_here {
                let _ = window.drop_image(image);
            }
        }
        self.worker.trim();
    }

    /// A frame from the render thread replaces the one shown, if it is of the document and page
    /// on show. One of another page (the page changed while it rendered) would never be painted:
    /// it is dropped, and the frame shown stays until the page's own comes.
    pub(super) fn show_frame(&mut self, frame: Frame, cx: &mut Context<Self>) {
        // Which request the frame answers, to follow a stutter from its request to its paint, and
        // whether it was drawn whole.
        log::trace!(
            "canvas frame of request {:016x} ({:?})",
            frame.key,
            frame.work
        );
        let on_show = self.board().and_then(|board| board.session.ds().ok());
        if !on_show.is_some_and(|ds| frame.shows(ds.uid, ds.page)) {
            return;
        }
        self.agents_rendered(frame.key);
        if let Some(old) = self.frame.replace(frame) {
            self.stale.push(old.image);
        }
        cx.notify();
    }

    /// The view is going: its images leave the window's sprite atlas with it.
    pub(super) fn release_images(&mut self, cx: &mut App) {
        let images: Vec<_> = (self.frame.take().map(|frame| frame.image))
            .into_iter()
            .chain(self.stale.drain(..))
            .collect();
        if let Some(window) = self.window
            && !images.is_empty()
        {
            let _ = window.update(cx, |_, window, _| {
                for image in images {
                    let _ = window.drop_image(image);
                }
            });
        }
    }

    /// Each frame, before the paint: tells the engine the theme and the canvas's size, fits the
    /// view to the content the first time, asks for a render when the view or the document isn't
    /// the one asked for last, lays out the overlay and hands the engine its frames' names as they
    /// will be painted, and starts the autosave after an edit. While an input method composes,
    /// the document rendered, and the text the overlay lays out, have the composition in them. What
    /// to paint, if there is a document.
    pub(super) fn prepaint(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Painting> {
        self.bounds = bounds;
        self.window = Some(window.window_handle());
        let dark = cx.theme().mode.is_dark();
        let scale_factor = f64::from(window.scale_factor());
        let now = cx.background_executor().now();
        let zooming = self.zooming_until.is_some_and(|until| until > now);
        let board = self.board.as_mut().ok()?;
        board.session.set_ui_dark(dark);
        board.session.apply_view_prefs();
        let kind = board.kind();
        Composition::refresh(&mut self.composition, &board.session);
        let ds = board.session.ds_mut().ok()?;
        let (width, height) = (f64::from(bounds.size.width), f64::from(bounds.size.height));
        // The chrome is laid out before this, from the view the last paint left: when the canvas
        // has changed size, or the view is fitted now, it is drawn again for the zoom it shows and
        // the context bar's place to catch up. A redraw asked for while painting would be lost,
        // so it is asked for once the paint is over.
        let mut moved = (ds.view.vw, ds.view.vh) != (width, height);
        ds.view.vw = width;
        ds.view.vh = height;
        let (doc, page) = (ds.doc.clone(), ds.page);
        if !self.fitted && width >= 1.0 && height >= 1.0 {
            view::zoom_to_fit(&mut ds.view, &doc, page);
            self.fitted = true;
            moved = true;
        }
        if moved {
            let this = cx.entity_id();
            cx.defer(move |cx| cx.notify(this));
        }
        // The page as the screen shows it; the render takes it as the document holds it, and
        // the worker converts its pixels the same way.
        let screen_rgb = view::screen_rgb(doc.color_profile, view::page_rgb(&doc, page));
        let viewport = Viewport {
            width,
            height,
            scale_factor,
        };
        // Pixel preview's pixels are the point of it: no softer preview of them while zooming.
        let quality = if zooming && ds.view.pixel_preview_scale().is_none() {
            PREVIEW_QUALITY
        } else {
            1.0
        };
        if let Some(wanted) = view::render_view(&ds.view, viewport, quality) {
            let mut job = render_job(ds, self.composition.as_ref(), kind, wanted);
            // Text an agent just wrote shows a word at a time ([`presence`]).
            if let Some(revealing) = self.agents.reveals.document(&job.doc, now) {
                job.doc = revealing;
            }
            let key = job_key(&job, ds.view.zoom, dark);
            let asked = (self.requested.as_ref())
                .is_some_and(|(asked, of)| *asked == key && Arc::ptr_eq(of, &job.doc));
            if !asked {
                self.requested = Some((key, job.doc.clone()));
                self.worker.request(Job { key, ..job });
            }
        }
        // Only an image of the document and page on show: until the page's own frame comes, one
        // of the page shown before stays off screen (FigCraft's canvas: `tex_of == shown`).
        let frame = (self.frame.as_ref())
            .filter(|frame| frame.shows(ds.uid, page))
            .map(|frame| {
                let at = paint::image_bounds(bounds, &frame.view, &ds.view, scale_factor);
                (frame.image.clone(), at)
            });
        // What goes over the frame, its labels measured. The frames' names go back to the engine
        // as they are painted: it takes a press or a hover on one as on its frame.
        // On a panic, guard::run logs it and this frame paints no overlay and no names to press.
        let composition = self.composition.as_ref();
        let shapes = figcraft_engine::guard::run("ui.overlay", || {
            Ok(overlay::overlay_with(&board.session, composition))
        })
        .unwrap_or_default();
        let ds = board.session.ds().ok()?;
        let (shown_view, doc) = (ds.view.clone(), ds.doc.clone());
        let font = &cx.theme().font_family;
        // A frame an agent builds in has its name on the work's row instead ([`presence`]).
        let shapes = self.agents.without_work_names(shapes, now);
        let (overlay, names) = overlay_paint::layout(shapes, font, &ds.doc, &ds.view, window);
        board.session.tool.labels = names;
        // Over it all, the agents at work on the page on show.
        let agents = self.agents_paint(&doc, &shown_view, page, now, window, cx);
        let painting = Painting {
            page: screen_rgb,
            frame,
            stale: std::mem::take(&mut self.stale),
            overlay,
            agents,
        };
        self.autosave(cx);
        Some(painting)
    }
}

/// The render of the page on show in `ds` at `wanted` ([`view::render_view`]), drawn as the View
/// menu has it, as FigCraft's own canvas asks for its renders (ui-egui `canvas/render.rs`):
/// - Outlines ▸ Show outlines draws every layer as its outline, and Show outlines for selection
///   the layers selected; Include hidden layers and Include object bounds add to either.
/// - Soft-proof sRGB stripes the colours of a document in another profile that the screen can't
///   show.
/// - Pixel preview, zoomed in past its resolution, shows the page at 1 or 2 pixels per unit.
///
/// It is drawn on the page's colour, and a board on its dot grid. While an input method composes,
/// what is drawn is `composition`'s document, the composition in the edited text, as FigCraft's
/// canvas draws it: the text shows as it will read once committed. The key is left for
/// [`job_key`] to name.
pub(super) fn render_job(
    ds: &figcraft_engine::DocState,
    composition: Option<&Composition>,
    kind: DocKind,
    wanted: View,
) -> Job {
    let doc = composition.map_or_else(|| ds.doc.clone(), |composition| composition.doc.clone());
    let page_rgb = view::page_rgb(&doc, ds.page);
    let [r, g, b] = page_rgb;
    let shown = &ds.view;
    let opts = Options {
        lod: true,
        magnified_image_pixels: true,
        background: Some(Color::from_rgba8(r, g, b, 255)),
        dots: match kind {
            DocKind::Board => view::dots(shown.zoom, view::screen_rgb(doc.color_profile, page_rgb)),
            DocKind::Design => None,
        },
        outline_mode: shown.outlines,
        outline: figcraft_render::Outline {
            hidden: shown.outline_hidden,
            bounds: shown.outline_bounds,
            // The renderer turns the lines light on a dark page itself.
            light: false,
        },
        outlined: if shown.outlines_selection {
            ds.selection.clone()
        } else {
            Vec::new()
        },
        ..Options::default()
    };
    Job {
        uid: ds.uid,
        page: ds.page,
        view: wanted,
        opts,
        page_rgb,
        proof: shown.proof_srgb && !doc.color_profile.is_srgb(),
        pixels: shown.pixel_preview_scale(),
        doc,
        key: 0,
    }
}

/// The name of render request `job` at `zoom` in the `dark` theme or not: the same for requests
/// that would draw the same frame. Another document (each edit makes one), page, view, way of
/// drawing it (the View menu's outlines, soft proof and pixel preview), set of fonts or theme
/// makes another.
pub(super) fn job_key(job: &Job, zoom: f64, dark: bool) -> u64 {
    let mut hasher = DefaultHasher::new();
    Arc::as_ptr(&job.doc).hash(&mut hasher);
    (job.uid, job.page).hash(&mut hasher);
    let view = &job.view;
    for number in [view.origin.0, view.origin.1, view.scale, zoom] {
        number.to_bits().hash(&mut hasher);
    }
    (view.width, view.height).hash(&mut hasher);
    let opts = &job.opts;
    (opts.outline_mode, opts.outline.hidden, opts.outline.bounds).hash(&mut hasher);
    opts.outlined.hash(&mut hasher);
    (job.proof, job.pixels.map(f64::to_bits)).hash(&mut hasher);
    figcraft_text::layout_generation().hash(&mut hasher);
    dark.hash(&mut hasher);
    hasher.finish()
}
