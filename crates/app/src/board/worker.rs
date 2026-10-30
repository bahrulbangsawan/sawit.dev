//! The canvas's render thread. It draws the page with FigCraft's [`Retained`] renderer, which
//! re-renders only what changed since its last frame, and hands each frame to the UI as a GPUI
//! image. The newest request wins: one made while another waits replaces it, so the thread never
//! works through a backlog of views the canvas has already left.

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use figcraft_doc::color::ColorProfile;
use figcraft_doc::{Document, NodeId};
use figcraft_render::{Options, Pixmap, Retained, View, retained};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::RenderImage;
use sawit_board::view::{self, screen_rgb, to_bgra};

/// What to render: a view of a page of a document, and how to draw it.
pub struct Job {
    /// The document as it is now: a snapshot, which later edits leave alone.
    pub doc: Arc<Document>,
    /// The document's [`figcraft_engine::DocState::uid`], which names it for good (each edit
    /// makes a new `doc`): with `page`, what the frame is an image of.
    pub uid: u64,
    pub page: NodeId,
    /// The page area to draw and its size in device px (`sawit_board::view::render_view`).
    pub view: View,
    pub opts: Options,
    /// The page's colour as the document holds it, in the document's colour profile
    /// (`sawit_board::view::page_rgb`). It shows wherever the render leaves a pixel less than
    /// opaque, converted to sRGB with the pixels. Its colour on screen, which fills the canvas
    /// around the frames and which the dot grid's light/dark test reads, is
    /// `sawit_board::view::screen_rgb(doc.color_profile, page_rgb)`.
    pub page_rgb: [u8; 3],
    /// Soft-proof sRGB: stripes over the colours the screen can't show
    /// (`sawit_board::view::mark_outside_srgb`). Only a document in another profile has any.
    pub proof: bool,
    /// Pixel preview: the page is rendered at this many pixels per page unit, and each of them
    /// is shown as the block of `view`'s pixels it covers (`sawit_board::view::magnify`). `None`
    /// renders `view` itself.
    pub pixels: Option<f64>,
    /// The caller's name for this request, handed back with its frame.
    pub key: u64,
}

/// A rendered job.
pub struct Frame {
    /// The page area the image shows: the job's view snapped outwards to whole device px, one
    /// more each way (`figcraft_render::retained::snap`). Place the image by this view, not by
    /// the one requested.
    pub view: View,
    /// The pixels: BGRA, opaque, `view.width` × `view.height`.
    pub image: Arc<RenderImage>,
    /// The [`Job::key`] of the request this answers.
    pub key: u64,
    /// What the image is of: the [`Job::uid`] and [`Job::page`] of that request.
    pub uid: u64,
    pub page: NodeId,
}

impl Frame {
    /// Whether this is an image of page `page` of the document whose uid is `uid`. The canvas
    /// paints only an image of the page on show, as FigCraft's own canvas does: one of another
    /// page or file, moved over this one, would show content that isn't there.
    pub fn shows(&self, uid: u64, page: NodeId) -> bool {
        self.uid == uid && self.page == page
    }
}

/// The render thread of one canvas (named `canvas-render`). It ends when the worker drops, once
/// it has finished the job it is on; dropping the worker doesn't wait for that.
pub struct Worker {
    slot: Arc<Slot>,
}

impl Worker {
    /// Starts the thread. Frames come on the receiver in the order of their requests, and the
    /// receiver ends when the thread does.
    ///
    /// If the system can't start a thread, the error is logged and the receiver ends at once: the
    /// canvas gets no frames, and nothing panics.
    pub fn spawn() -> (Worker, UnboundedReceiver<Frame>) {
        let slot = Arc::new(Slot::default());
        let (frames, receiver) = unbounded();
        let jobs = slot.clone();
        let spawned = std::thread::Builder::new()
            .name("canvas-render".into())
            .spawn(move || run(&jobs, &frames));
        if let Err(err) = spawned {
            log::error!("the canvas render thread didn't start: {err}");
        }
        (Worker { slot }, receiver)
    }

    /// Asks for `job` to be rendered. The newest request wins: a job still waiting for the thread
    /// is replaced, never queued, so frames come only for the job being rendered and the newest.
    pub fn request(&self, job: Job) {
        self.slot.put(job);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.slot.close();
    }
}

/// Where a job waits for the thread: one job at most, the newest.
#[derive(Default)]
struct Slot {
    state: Mutex<Waiting>,
    /// Signalled when a job is put in, or the slot closes.
    changed: Condvar,
}

#[derive(Default)]
struct Waiting {
    job: Option<Job>,
    /// The worker has dropped: the thread is to end.
    closed: bool,
}

impl Slot {
    /// Makes `job` the one waiting, in place of any job that was.
    fn put(&self, job: Job) {
        let replaced = self.lock().job.replace(job);
        self.changed.notify_one();
        // Outside the lock: the last hold on an old document may be a large free.
        drop(replaced);
    }

    /// Waits for a job and takes it. `None` once the slot is closed: no more jobs come.
    fn take(&self) -> Option<Job> {
        let state = self.lock();
        let mut state = self
            .changed
            .wait_while(state, |s| s.job.is_none() && !s.closed)
            .unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return None;
        }
        state.job.take()
    }

    /// Ends the thread's wait for good, and drops the job waiting, if any.
    fn close(&self) {
        let dropped = {
            let mut state = self.lock();
            state.closed = true;
            state.job.take()
        };
        self.changed.notify_all();
        drop(dropped);
    }

    /// The state, even after a panic elsewhere while it was held: each change to it is a single
    /// step, so it is never left half made.
    fn lock(&self) -> MutexGuard<'_, Waiting> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The thread: renders each job it takes from `slot` and sends the frame, until the worker drops
/// or the frames' receiver does (nobody shows them any more).
fn run(slot: &Slot, frames: &UnboundedSender<Frame>) {
    let mut retained = Retained::default();
    while let Some(job) = slot.take() {
        let Some(frame) = render(&mut retained, &job) else {
            continue;
        };
        if frames.unbounded_send(frame).is_err() {
            break;
        }
    }
}

/// Renders `job` into a frame, building on the last render `retained` kept. `None` when the view
/// can't be drawn (not finite, or too large to allocate), and when the renderer panics.
fn render(retained: &mut Retained, job: &Job) -> Option<Frame> {
    let started = Instant::now();
    // Pixel preview renders the job's page area coarser, then lays it over the job's pixels.
    let coarse = job
        .pixels
        .and_then(|pixels| view::pixel_view(&job.view, pixels));
    let (drawn, (width, height), bgra) = guarded(retained, |retained| {
        let asked = coarse.as_ref().unwrap_or(&job.view);
        let (drawn, pixmap, _) = retained.render(&job.doc, job.page, asked, &job.opts)?;
        let bgra = screen_bgra(pixmap, job.doc.color_profile, job.page_rgb, job.proof);
        Some((drawn, (pixmap.width(), pixmap.height()), bgra))
    })?;
    let (shown, (width, height), bgra) = match coarse {
        Some(_) => {
            // Over the job's own pixels, snapped outwards to whole device pixels as a render's
            // view is, so the frame is placed as any other.
            let (screen, _) = retained::snap(&job.view)?;
            let bgra = view::magnify(&bgra, &drawn, &screen);
            let size = (screen.width, screen.height);
            (screen, size, bgra)
        }
        None => (drawn, (width, height), bgra),
    };
    // As `viewers/pdf.rs` wraps its pages: GPUI takes the bytes as they are, BGRA.
    let pixels = image::RgbaImage::from_raw(width, height, bgra)?;
    let image = Arc::new(RenderImage::new([image::Frame::new(pixels)]));
    // How long the job took, from the render to the image.
    let millis = started.elapsed().as_secs_f64() * 1000.0;
    log::debug!("canvas render {millis:.1} ms");
    Some(Frame {
        view: shown,
        image,
        key: job.key,
        uid: job.uid,
        page: job.page,
    })
}

/// Runs `draw` on `retained`, as FigCraft's own render thread does: a panic in the renderer (a
/// bug) costs that frame, not the thread. The image `retained` kept may be half drawn after one,
/// so it is dropped and the next render starts over.
fn guarded<T>(retained: &mut Retained, draw: impl FnOnce(&mut Retained) -> Option<T>) -> Option<T> {
    match std::panic::catch_unwind(AssertUnwindSafe(|| draw(&mut *retained))) {
        Ok(drawn) => drawn,
        Err(_) => {
            log::error!("the canvas renderer panicked; the next frame is rendered from scratch");
            retained.clear();
            None
        }
    }
}

/// `pixmap`, a render of a document whose colours are in `profile`, as BGRA bytes for the screen,
/// which shows sRGB. As in FigCraft's own canvas, a document in another profile has its pixels
/// converted to sRGB (ui-egui `screen_image`) and its page colour too ([`screen_rgb`], as ui-egui
/// `doc_c32` does). With `proof`, the colours sRGB can't show are striped
/// ([`view::mark_outside_srgb`]).
fn screen_bgra(pixmap: &Pixmap, profile: ColorProfile, page_rgb: [u8; 3], proof: bool) -> Vec<u8> {
    if profile.is_srgb() {
        return to_bgra(pixmap, page_rgb);
    }
    let to_srgb = profile.conversion_to(ColorProfile::Srgb);
    let mut converted = pixmap.clone();
    to_srgb.apply_pixels(converted.data_mut());
    let mut bgra = to_bgra(&converted, screen_rgb(profile, page_rgb));
    if proof {
        view::mark_outside_srgb(&mut bgra, pixmap, profile);
    }
    bgra
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use figcraft_doc::color::{Color, ColorProfile};
    use figcraft_doc::{Document, NodeId, NodeKind};
    use figcraft_render::{Options, Retained, View, Work};
    use futures::StreamExt;
    use futures::channel::mpsc::UnboundedReceiver;
    use futures::executor::block_on;
    use futures::future::{self, Either};
    use sawit_board::BoardDoc;
    use sawit_board::view::page_rgb;
    use serde_json::json;

    use super::*;

    /// How long a test waits for the render thread.
    const TIMEOUT: Duration = Duration::from_secs(5);

    /// The next frame the thread sends, or `None` once it has ended (it holds the only sender).
    /// Fails the test when neither happens within [`TIMEOUT`].
    fn next_frame(frames: &mut UnboundedReceiver<Frame>) -> Option<Frame> {
        let (ring, alarm) = futures::channel::oneshot::channel::<()>();
        std::thread::spawn(move || {
            std::thread::sleep(TIMEOUT);
            let _ = ring.send(());
        });
        match block_on(future::select(frames.next(), alarm)) {
            Either::Left((next, _)) => next,
            Either::Right(_) => panic!("nothing from the render thread in {TIMEOUT:?}"),
        }
    }

    /// A board with one sticky note on its page, and the page.
    fn board_with_a_sticky() -> (Arc<Document>, NodeId) {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "hi"}))
            .unwrap();
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        (doc, page)
    }

    /// A request for 400 × 300 device px of `page` from page point (-100, -100) at 100%, drawn on
    /// the page's colour as the canvas draws it.
    fn job(doc: &Arc<Document>, page: NodeId, key: u64) -> Job {
        let rgb = page_rgb(doc, page);
        let [r, g, b] = rgb;
        Job {
            doc: doc.clone(),
            uid: 1,
            page,
            view: View {
                origin: (-100.0, -100.0),
                scale: 1.0,
                width: 400,
                height: 300,
            },
            opts: Options {
                background: Some(Color::from_rgba8(r, g, b, 255)),
                ..Options::default()
            },
            page_rgb: rgb,
            proof: false,
            pixels: None,
            key,
        }
    }

    /// The pixel at (`x`, `y`) of a frame's image, BGRA.
    fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
        let width = frame.image.size(0).width.0 as u32;
        let at = ((y * width + x) * 4) as usize;
        let bytes = frame.image.as_bytes(0).unwrap();
        bytes[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn renders_a_job() {
        let (doc, page) = board_with_a_sticky();
        let (worker, mut frames) = Worker::spawn();
        let mut asked = job(&doc, page, 7);
        asked.uid = 42;
        worker.request(asked);
        let frame = next_frame(&mut frames).expect("a frame");

        assert_eq!(frame.key, 7);
        // It says what it is an image of: the document and the page asked for, and no other.
        assert_eq!((frame.uid, frame.page), (42, page));
        assert!(frame.shows(42, page));
        assert!(!frame.shows(41, page));
        assert!(!frame.shows(42, NodeId(page.0 + 1)));
        // The view drawn is the one asked for, snapped outwards to whole device pixels: it covers
        // the view asked for, with one more pixel each way. The image is that view's size.
        let view = &frame.view;
        assert!(
            view.origin.0 <= -100.0 && view.origin.1 <= -100.0,
            "{view:?}"
        );
        assert_eq!(view.scale, 1.0);
        assert_eq!((view.width, view.height), (401, 301));
        let size = frame.image.size(0);
        assert_eq!(
            (size.width.0, size.height.0),
            (view.width as i32, view.height as i32)
        );

        // What is drawn: the sticky where it is on the page, and the page around it.
        let [r, g, b] = page_rgb(&doc, page);
        let on_page = [b, g, r, 255];
        assert_eq!(pixel(&frame, 5, 5), on_page);
        let sticky = doc.page_content_bounds(page).unwrap();
        let inside = |edge: f64, origin: f64| ((edge + 10.0 - origin) * view.scale) as u32;
        let (x, y) = (
            inside(sticky.x0, view.origin.0),
            inside(sticky.y0, view.origin.1),
        );
        assert_ne!(pixel(&frame, x, y), on_page, "no sticky at ({x}, {y})");
    }

    #[test]
    fn latest_job_wins() {
        let (doc, page) = board_with_a_sticky();
        let (worker, mut frames) = Worker::spawn();
        for key in 1..=3 {
            worker.request(job(&doc, page, key));
        }
        let mut keys = Vec::new();
        while keys.last() != Some(&3) {
            let frame = next_frame(&mut frames).expect("a frame for the last request");
            keys.push(frame.key);
        }
        // Then nothing more: the thread ends once the worker drops, and sends nothing on the way.
        drop(worker);
        while let Some(frame) = next_frame(&mut frames) {
            keys.push(frame.key);
        }
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{keys:?}");
        assert_eq!(keys.last(), Some(&3), "{keys:?}");
    }

    #[test]
    fn a_request_replaces_the_job_waiting_instead_of_queueing_behind_it() {
        let (doc, page) = board_with_a_sticky();
        let slot = Slot::default();
        for key in 1..=3 {
            slot.put(job(&doc, page, key));
        }
        let waiting = slot.lock().job.as_ref().map(|job| job.key);
        assert_eq!(waiting, Some(3));
        assert_eq!(slot.take().map(|job| job.key), Some(3));
        assert!(slot.lock().job.is_none());
    }

    #[test]
    fn the_thread_ends_when_the_worker_drops() {
        let (doc, page) = board_with_a_sticky();
        let (worker, mut frames) = Worker::spawn();
        worker.request(job(&doc, page, 1));
        drop(worker);
        // The job dies with the worker, or the thread finishes it first; then the thread ends.
        let mut keys = Vec::new();
        while let Some(frame) = next_frame(&mut frames) {
            keys.push(frame.key);
        }
        assert!(keys.is_empty() || keys == [1], "{keys:?}");
    }

    #[test]
    fn the_thread_ends_when_nobody_takes_frames() {
        let (doc, page) = board_with_a_sticky();
        let (worker, frames) = Worker::spawn();
        drop(frames);
        worker.request(job(&doc, page, 1));
        // The thread lets go of the slot when it ends.
        let deadline = Instant::now() + TIMEOUT;
        while Arc::strong_count(&worker.slot) > 1 {
            assert!(Instant::now() < deadline, "the thread still runs");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A Display P3 document whose page colour is `rgba` (P3 values), and the page.
    fn p3_page([r, g, b, a]: [u8; 4]) -> (Arc<Document>, NodeId) {
        let mut doc = Document::new("File");
        doc.color_profile = ColorProfile::DisplayP3;
        let page = doc.first_page().unwrap();
        if let Some(NodeKind::Page { background, .. }) = doc.node_mut(page).map(|n| &mut n.kind) {
            *background = Color::from_rgba8(r, g, b, a);
        }
        (Arc::new(doc), page)
    }

    #[test]
    fn a_display_p3_document_is_shown_in_srgb() {
        let [r, g, b, _] = ColorProfile::DisplayP3
            .conversion_to(ColorProfile::Srgb)
            .apply(Color::from_rgba8(200, 100, 50, 255))
            .to_rgba8();
        let srgb = [b, g, r, 255];
        // The P3 values read as sRGB are far from that: even half the difference, which a page
        // colour left unconverted under a half-transparent pixel would make, is more than the
        // step or two that 8-bit conversions round by.
        assert!(
            srgb.iter()
                .zip([50, 100, 200])
                .any(|(s, p)| s.abs_diff(p) > 8)
        );
        let shows_srgb = |frame: &Frame| {
            let drawn = pixel(frame, 5, 5);
            assert!(
                drawn.iter().zip(srgb).all(|(d, s)| d.abs_diff(s) <= 2),
                "{drawn:?}, not {srgb:?}"
            );
        };

        let (worker, mut frames) = Worker::spawn();
        // An opaque page, drawn on its colour as the canvas draws it: the pixels are converted.
        let (doc, page) = p3_page([200, 100, 50, 255]);
        worker.request(job(&doc, page, 1));
        shows_srgb(&next_frame(&mut frames).expect("a frame"));
        // A half-transparent page colour, which the renderer lays itself without a background of
        // the job's: half of each pixel is the page colour behind it, converted as well.
        let (doc, page) = p3_page([200, 100, 50, 128]);
        let mut translucent = job(&doc, page, 2);
        translucent.opts.background = None;
        worker.request(translucent);
        shows_srgb(&next_frame(&mut frames).expect("a frame"));
    }

    /// Whether every pixel of `frame` is the one at the top-left of its `block` × `block` square:
    /// whether the image is made of such blocks.
    fn in_blocks(frame: &Frame, block: u32) -> bool {
        let size = frame.image.size(0);
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        (0..height).all(|y| {
            (0..width).all(|x| pixel(frame, x, y) == pixel(frame, x - x % block, y - y % block))
        })
    }

    /// A design file with one rectangle half a unit off the page's grid, from (10.5, 10.5) to
    /// (30.5, 30.5), and its page.
    fn page_with_a_rectangle() -> (Arc<Document>, NodeId) {
        let mut session = figcraft_engine::Session::new();
        let rectangle =
            json!({"type": "rectangle", "x": 10.5, "y": 10.5, "width": 20, "height": 20});
        session.execute("node.create", &rectangle).unwrap();
        let ds = session.ds().unwrap();
        (ds.doc.clone(), ds.page)
    }

    #[test]
    fn pixel_preview_shows_each_rendered_pixel_as_a_block() {
        let (doc, page) = page_with_a_rectangle();
        // 50 × 37.5 page units from the page origin at 8 device px per unit: the rectangle's left
        // side is half way across a preview pixel, which starts where the image does.
        let view = View {
            origin: (0.0, 0.0),
            scale: 8.0,
            width: 400,
            height: 300,
        };
        let asked = |pixels: Option<f64>, key: u64| Job {
            view: view.clone(),
            pixels,
            ..job(&doc, page, key)
        };
        let (worker, mut frames) = Worker::spawn();
        worker.request(asked(Some(1.0), 1));
        let frame = next_frame(&mut frames).expect("a frame");

        // On the job's own pixels, placed as any frame: its view snapped as a render's is.
        let (snapped, _) = retained::snap(&view).unwrap();
        assert_eq!(frame.view.origin, snapped.origin);
        assert_eq!(frame.view.scale, 8.0);
        assert_eq!((frame.view.width, frame.view.height), (401, 301));
        let size = frame.image.size(0);
        assert_eq!((size.width.0, size.height.0), (401, 301));
        // Each page unit is one pixel of the preview, an 8 × 8 block on the screen: the column
        // the rectangle's side halves is one block of a colour between the two.
        assert!(in_blocks(&frame, 8));
        let [r, g, b] = page_rgb(&doc, page);
        let page_colour = [b, g, r, 255];
        let inside = pixel(&frame, 200, 150);
        assert_ne!(inside, page_colour, "no rectangle");
        let side = pixel(&frame, 84, 150);
        assert!(side != page_colour && side != inside, "{side:?}");

        // The same view without the preview draws the side where it is, half way into a block.
        worker.request(asked(None, 2));
        let frame = next_frame(&mut frames).expect("a frame");
        assert!(!in_blocks(&frame, 8));
        assert_eq!(pixel(&frame, 83, 150), page_colour);
        assert_eq!(pixel(&frame, 84, 150), inside);
    }

    #[test]
    fn soft_proof_stripes_the_colours_srgb_cannot_show() {
        // A Display P3 page in P3's own green, which sRGB can't show.
        let (doc, page) = p3_page([0, 255, 0, 255]);
        let (worker, mut frames) = Worker::spawn();
        let mut proofed = job(&doc, page, 1);
        proofed.proof = true;
        worker.request(proofed);
        let frame = next_frame(&mut frames).expect("a frame");
        // Diagonal stripes: four pixels on and four off along a row, a pixel to the left on the
        // next row.
        assert_eq!(pixel(&frame, 0, 0), view::PROOF_GREY);
        assert_eq!(pixel(&frame, 3, 0), view::PROOF_GREY);
        assert_ne!(pixel(&frame, 4, 0), view::PROOF_GREY);
        assert_eq!(pixel(&frame, 2, 1), view::PROOF_GREY);
        assert_ne!(pixel(&frame, 3, 1), view::PROOF_GREY);
        assert_eq!(pixel(&frame, 7, 1), view::PROOF_GREY);

        // Without the proof the page shows its colour, clipped to sRGB, everywhere.
        worker.request(job(&doc, page, 2));
        let frame = next_frame(&mut frames).expect("a frame");
        assert_ne!(pixel(&frame, 0, 0), view::PROOF_GREY);
        assert_eq!(pixel(&frame, 0, 0), pixel(&frame, 4, 0));
    }

    #[test]
    fn a_renderer_panic_costs_its_frame_and_the_kept_render() {
        let (doc, page) = board_with_a_sticky();
        let job = job(&doc, page, 1);
        let draw = |retained: &mut Retained| {
            let (_, _, work) = retained.render(&job.doc, job.page, &job.view, &job.opts)?;
            Some(work)
        };
        let mut retained = Retained::default();
        assert_eq!(guarded(&mut retained, draw), Some(Work::Full));
        assert_eq!(guarded(&mut retained, draw), Some(Work::Reused));
        let bug = |_: &mut Retained| -> Option<Work> { panic!("a renderer bug") };
        assert_eq!(guarded(&mut retained, bug), None);
        // The image kept from before may be half drawn: the next render starts over.
        assert_eq!(guarded(&mut retained, draw), Some(Work::Full));
    }
}
