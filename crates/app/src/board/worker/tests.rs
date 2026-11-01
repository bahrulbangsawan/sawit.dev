//! Tests of the render thread: its frames, the newest request winning, trims, panics, colour
//! profiles, pixel preview and soft proof.

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
    assert_eq!(key_of(slot.take()), Some(3));
    assert!(slot.lock().job.is_none());
}

/// The key of the job `asked` renders, if it is one.
fn key_of(asked: Option<Asked>) -> Option<u64> {
    match asked {
        Some(Asked::Render(job)) => Some(job.key),
        Some(Asked::Trim) | None => None,
    }
}

/// CANVAS-UI-9: a trim waits for the job waiting, and a job asked for after it calls it off,
/// the canvas being shown again.
#[test]
fn a_trim_comes_after_the_job_waiting_and_a_later_job_calls_it_off() {
    let (doc, page) = board_with_a_sticky();
    let slot = Slot::default();
    slot.put(job(&doc, page, 1));
    slot.trim();
    assert_eq!(key_of(slot.take()), Some(1));
    assert!(matches!(slot.take(), Some(Asked::Trim)));
    assert!(!slot.lock().trim);

    slot.trim();
    slot.put(job(&doc, page, 2));
    assert_eq!(key_of(slot.take()), Some(2));
    assert!(!slot.lock().trim);
}

/// CANVAS-UI-9: a trim has the thread forget the render it keeps, so the next one is drawn
/// whole, where it would have built on it.
#[test]
fn a_trim_has_the_next_render_drawn_whole() {
    let (doc, page) = board_with_a_sticky();
    let (worker, mut frames) = Worker::spawn();
    worker.request(job(&doc, page, 1));
    assert_eq!(next_frame(&mut frames).expect("a frame").work, Work::Full);
    worker.request(job(&doc, page, 2));
    assert_eq!(next_frame(&mut frames).expect("a frame").work, Work::Reused);
    worker.trim();
    // The thread takes the trim before the next job is asked for, which would call it off.
    let deadline = Instant::now() + TIMEOUT;
    while worker.slot.lock().trim {
        assert!(Instant::now() < deadline, "the thread didn't take the trim");
        std::thread::sleep(Duration::from_millis(1));
    }
    worker.request(job(&doc, page, 3));
    assert_eq!(next_frame(&mut frames).expect("a frame").work, Work::Full);
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
    let rectangle = json!({"type": "rectangle", "x": 10.5, "y": 10.5, "width": 20, "height": 20});
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
