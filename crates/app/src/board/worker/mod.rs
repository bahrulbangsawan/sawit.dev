//! The canvas's render thread. It draws the page with FigCraft's [`Retained`] renderer, which
//! re-renders only what changed since its last frame, and hands each frame to the UI as a GPUI
//! image. The newest request wins: one made while another waits replaces it, so the thread never
//! works through a backlog of views the canvas has already left. A canvas whose tab is hidden
//! has the thread forget the render it keeps ([`Worker::trim`]), a page's worth of pixels.

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use figcraft_doc::color::ColorProfile;
use figcraft_doc::{Document, NodeId};
use figcraft_render::{Options, Pixmap, Retained, View, Work, retained};
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
    /// What the render took: the whole page, or only what changed since the render the thread
    /// kept ([`Retained`]).
    pub work: Work,
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

    /// Has the thread forget the render it keeps to build the next one on, once it is done with
    /// what is asked of it now: a canvas whose tab is hidden draws no frames, and that render is
    /// a page's worth of pixels (CANVAS-UI-9). The next render is then drawn whole. A job asked
    /// for after this, the canvas being shown again, keeps the render instead.
    pub fn trim(&self) {
        self.slot.trim();
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
    /// The thread is to forget the render it keeps ([`Worker::trim`]).
    trim: bool,
    /// The worker has dropped: the thread is to end.
    closed: bool,
}

/// What the thread is asked to do next.
enum Asked {
    Render(Box<Job>),
    /// Forget the render kept ([`Worker::trim`]).
    Trim,
}

impl Slot {
    /// Makes `job` the one waiting, in place of any job that was. A trim asked for before it is
    /// off: the canvas draws again, and its next frames build on the render kept.
    fn put(&self, job: Job) {
        let replaced = {
            let mut state = self.lock();
            state.trim = false;
            state.job.replace(job)
        };
        self.changed.notify_one();
        // Outside the lock: the last hold on an old document may be a large free.
        drop(replaced);
    }

    /// Asks the thread to forget the render it keeps, after the job waiting, if any.
    fn trim(&self) {
        self.lock().trim = true;
        self.changed.notify_one();
    }

    /// Waits for something to do and takes it: the job waiting first, then a trim. `None` once
    /// the slot is closed: nothing more comes.
    fn take(&self) -> Option<Asked> {
        let state = self.lock();
        let mut state = self
            .changed
            .wait_while(state, |s| s.job.is_none() && !s.trim && !s.closed)
            .unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return None;
        }
        if let Some(job) = state.job.take() {
            return Some(Asked::Render(Box::new(job)));
        }
        state.trim = false;
        Some(Asked::Trim)
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
/// or the frames' receiver does (nobody shows them any more). A trim forgets the render kept.
fn run(slot: &Slot, frames: &UnboundedSender<Frame>) {
    let mut retained = Retained::default();
    while let Some(asked) = slot.take() {
        let job = match asked {
            Asked::Render(job) => job,
            Asked::Trim => {
                retained.clear();
                continue;
            }
        };
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
    let (drawn, (width, height), bgra, work) = guarded(retained, |retained| {
        let asked = coarse.as_ref().unwrap_or(&job.view);
        let (drawn, pixmap, work) = retained.render(&job.doc, job.page, asked, &job.opts)?;
        let bgra = screen_bgra(pixmap, job.doc.color_profile, job.page_rgb, job.proof);
        Some((drawn, (pixmap.width(), pixmap.height()), bgra, work))
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
        work,
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
mod tests;
