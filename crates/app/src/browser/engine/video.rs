//! Native video: a recording's frames ([`super::capture::Pixels`], WebKit's CGImages) written
//! by AVFoundation's `AVAssetWriter`, which hands them to the Mac's hardware encoder
//! (VideoToolbox): H.264 in MP4, or HEVC in MOV. Each frame keeps the time it was taken at, so
//! the video plays at the speed things happened, whatever rate WebKit managed. GIFs are encoded
//! in Rust (the `gif` crate). The writer runs on its own thread; the main thread only takes the
//! pictures.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus,
    AVFileTypeMPEG4, AVFileTypeQuickTimeMovie, AVMediaTypeVideo, AVVideoAverageBitRateKey,
    AVVideoCodecKey, AVVideoCodecTypeH264, AVVideoCodecTypeHEVC, AVVideoCompressionPropertiesKey,
    AVVideoExpectedSourceFrameRateKey, AVVideoHeightKey, AVVideoMaxKeyFrameIntervalKey,
    AVVideoWidthKey,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext};
use objc2_core_media::CMTime;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use sawit_core::browser::capture::VideoFormat;

use super::capture::Pixels;

/// What the writer is told at the start.
#[derive(Clone, Debug)]
pub struct VideoSpec {
    pub path: PathBuf,
    pub format: VideoFormat,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Bits per second; `None` lets the encoder pick from the size.
    pub bitrate: Option<u32>,
}

/// A frame and when it was taken, from the recording's start.
pub struct Frame {
    pub pixels: Pixels,
    pub at: Duration,
}

/// What the writer made.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    pub path: PathBuf,
    pub frames: u32,
    pub duration: Duration,
    pub bytes: u64,
}

/// The writer's thread: frames in, a file out.
pub struct VideoWriter {
    frames: mpsc::SyncSender<Option<Frame>>,
    done: mpsc::Receiver<Result<Written, String>>,
}

impl VideoWriter {
    /// Starts writing `spec`'s file.
    pub fn start(spec: VideoSpec) -> Result<VideoWriter, String> {
        // A few frames of slack: a slow encoder drops frames rather than holding memory.
        let (frames, rx) = mpsc::sync_channel::<Option<Frame>>(8);
        let (done_tx, done) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("sawit-video".into())
            .spawn(move || {
                let result = match spec.format {
                    VideoFormat::Gif => write_gif(&spec, rx, ready_tx),
                    _ => write_av(&spec, rx, ready_tx),
                };
                let _ = done_tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        ready_rx
            .recv()
            .map_err(|_| "The video writer stopped before it started.".to_string())??;
        Ok(VideoWriter { frames, done })
    }

    /// Hands a frame over; `false` when the writer is behind and the frame was dropped.
    pub fn push(&self, frame: Frame) -> bool {
        self.frames.try_send(Some(frame)).is_ok()
    }

    /// Ends the file and waits for it (off the main thread).
    pub fn finish(self) -> Result<Written, String> {
        let _ = self.frames.send(None);
        self.done
            .recv_timeout(Duration::from_secs(120))
            .map_err(|_| "The video took too long to finish.".to_string())?
    }
}

fn cm(at: Duration) -> CMTime {
    unsafe { CMTime::new(at.as_micros() as i64, 1_000_000) }
}

fn write_av(
    spec: &VideoSpec,
    frames: mpsc::Receiver<Option<Frame>>,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<Written, String> {
    let fail = |ready: &mpsc::Sender<Result<(), String>>, e: String| {
        let _ = ready.send(Err(e.clone()));
        Err(e)
    };
    let _ = std::fs::remove_file(&spec.path);
    let (writer, input, adaptor) = match objc2::rc::autoreleasepool(|_| open_writer(spec)) {
        Ok(w) => w,
        Err(e) => return fail(&ready, e),
    };
    let _ = ready.send(Ok(()));
    let mut count = 0u32;
    let mut last = Duration::ZERO;
    let mut started = false;
    while let Ok(Some(frame)) = frames.recv() {
        objc2::rc::autoreleasepool(|_| {
            // Times only move forward: a frame stamped at or before the last is moved on.
            let at = if count > 0 && frame.at <= last {
                last + Duration::from_millis(1)
            } else {
                frame.at
            };
            let Some(buffer) = pixel_buffer(&frame.pixels, spec.width, spec.height) else {
                return;
            };
            if !started {
                unsafe { writer.startSessionAtSourceTime(cm(at)) };
                started = true;
            }
            // Wait (briefly) for the encoder to take more.
            for _ in 0..500 {
                if unsafe { input.isReadyForMoreMediaData() } {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            if unsafe { adaptor.appendPixelBuffer_withPresentationTime(&buffer, cm(at)) } {
                count += 1;
                last = at;
            }
        });
    }
    if count == 0 {
        unsafe { writer.cancelWriting() };
        return Err("The recording has no frames.".into());
    }
    unsafe {
        input.markAsFinished();
        writer.endSessionAtSourceTime(cm(
            last + Duration::from_millis(1000 / u64::from(spec.fps.max(1)))
        ));
    }
    let (tx, rx) = mpsc::channel();
    let handler = block2::RcBlock::new(move || {
        let _ = tx.send(());
    });
    unsafe { writer.finishWritingWithCompletionHandler(&handler) };
    rx.recv_timeout(Duration::from_secs(60))
        .map_err(|_| "The encoder didn't finish the file.".to_string())?;
    if unsafe { writer.status() } != AVAssetWriterStatus::Completed {
        let why = unsafe { writer.error() }
            .map(|e| e.localizedDescription().to_string())
            .unwrap_or_else(|| "unknown error".into());
        return Err(format!("The video couldn't be written: {why}"));
    }
    Ok(Written {
        path: spec.path.clone(),
        frames: count,
        duration: last,
        bytes: std::fs::metadata(&spec.path).map(|m| m.len()).unwrap_or(0),
    })
}

type Opened = (
    Retained<AVAssetWriter>,
    Retained<AVAssetWriterInput>,
    Retained<AVAssetWriterInputPixelBufferAdaptor>,
);

fn open_writer(spec: &VideoSpec) -> Result<Opened, String> {
    unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&spec.path.to_string_lossy()));
        let file_type = match spec.format {
            VideoFormat::Mov => AVFileTypeQuickTimeMovie,
            _ => AVFileTypeMPEG4,
        }
        .ok_or("This Mac has no such video file type.")?;
        let writer =
            AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type).map_err(|e| {
                format!(
                    "The video file couldn't be made: {}",
                    e.localizedDescription()
                )
            })?;
        let codec = match spec.format {
            VideoFormat::Mov => AVVideoCodecTypeHEVC,
            _ => AVVideoCodecTypeH264,
        }
        .ok_or("This Mac has no such codec.")?;
        let pixels = f64::from(spec.width) * f64::from(spec.height);
        // About 0.1 bits a pixel a frame for screen content: sharp text, small files.
        let bitrate = spec
            .bitrate
            .unwrap_or_else(|| (pixels * f64::from(spec.fps) * 0.12).clamp(1.5e6, 60e6) as u32);
        let compression: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
            &[
                AVVideoAverageBitRateKey.ok_or("no key")?,
                AVVideoExpectedSourceFrameRateKey.ok_or("no key")?,
                AVVideoMaxKeyFrameIntervalKey.ok_or("no key")?,
            ],
            &[
                NSNumber::new_u32(bitrate).as_ref() as &AnyObject,
                NSNumber::new_u32(spec.fps).as_ref(),
                NSNumber::new_u32(spec.fps * 2).as_ref(),
            ],
        );
        let settings: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
            &[
                AVVideoCodecKey.ok_or("no key")?,
                AVVideoWidthKey.ok_or("no key")?,
                AVVideoHeightKey.ok_or("no key")?,
                AVVideoCompressionPropertiesKey.ok_or("no key")?,
            ],
            &[
                codec.as_ref() as &AnyObject,
                NSNumber::new_u32(spec.width).as_ref(),
                NSNumber::new_u32(spec.height).as_ref(),
                compression.as_ref(),
            ],
        );
        let media = AVMediaTypeVideo.ok_or("no video media type")?;
        let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
            media,
            Some(&settings),
        );
        input.setExpectsMediaDataInRealTime(true);
        if !writer.canAddInput(&input) {
            return Err(format!(
                "The encoder can't take {}×{} {}.",
                spec.width,
                spec.height,
                spec.format.codec()
            ));
        }
        writer.addInput(&input);
        let adaptor =
            AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
                &input, None,
            );
        if !writer.startWriting() {
            let why = writer
                .error()
                .map(|e| e.localizedDescription().to_string())
                .unwrap_or_default();
            return Err(format!("The encoder didn't start: {why}"));
        }
        Ok((writer, input, adaptor))
    }
}

/// BGRA, premultiplied first, little-endian: what a `32BGRA` pixel buffer holds.
const BGRA: u32 = 2 | (2 << 12);
/// RGBA, premultiplied last.
const RGBA: u32 = 1;

/// A BGRA pixel buffer of `w` × `h` with `pixels` drawn in it (scaled to fill).
fn pixel_buffer(pixels: &Pixels, w: u32, h: u32) -> Option<CFRetained<CVPixelBuffer>> {
    unsafe {
        let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
        let status = CVPixelBufferCreate(
            None,
            w as usize,
            h as usize,
            kCVPixelFormatType_32BGRA,
            None,
            std::ptr::NonNull::from(&mut out),
        );
        if status != 0 {
            return None;
        }
        let buffer = CFRetained::from_raw(std::ptr::NonNull::new(out)?);
        CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags(0));
        let base = CVPixelBufferGetBaseAddress(&buffer);
        let row = CVPixelBufferGetBytesPerRow(&buffer);
        let space = CGColorSpace::new_device_rgb();
        let context =
            CGBitmapContextCreate(base, w as usize, h as usize, 8, row, space.as_deref(), BGRA);
        if let Some(context) = &context {
            CGContext::draw_image(
                Some(context),
                CGRect::new(
                    CGPoint::new(0., 0.),
                    CGSize::new(f64::from(w), f64::from(h)),
                ),
                Some(pixels.image()),
            );
        }
        CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags(0));
        context.map(|_| buffer)
    }
}

fn write_gif(
    spec: &VideoSpec,
    frames: mpsc::Receiver<Option<Frame>>,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<Written, String> {
    let file = match std::fs::File::create(&spec.path) {
        Ok(f) => f,
        Err(e) => {
            let e = format!("The GIF couldn't be made: {e}");
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    let _ = ready.send(Ok(()));
    let (w, h) = (spec.width as u16, spec.height as u16);
    let mut encoder =
        gif::Encoder::new(std::io::BufWriter::new(file), w, h, &[]).map_err(|e| e.to_string())?;
    let _ = encoder.set_repeat(gif::Repeat::Infinite);
    let mut pending: Option<(Vec<u8>, Duration)> = None;
    let mut count = 0u32;
    let mut last = Duration::ZERO;
    let write = |rgba: &mut Vec<u8>, delay: Duration, enc: &mut gif::Encoder<_>| {
        let mut frame = gif::Frame::from_rgba_speed(w, h, rgba, 10);
        frame.delay = ((delay.as_millis() / 10).max(2)) as u16;
        enc.write_frame(&frame).map_err(|e| e.to_string())
    };
    while let Ok(Some(frame)) = frames.recv() {
        let Some(rgba) = rgba_of(&frame.pixels, spec.width, spec.height) else {
            continue;
        };
        if let Some((mut prev, at)) = pending.take() {
            write(&mut prev, frame.at.saturating_sub(at), &mut encoder)?;
        }
        pending = Some((rgba, frame.at));
        last = frame.at;
        count += 1;
    }
    if let Some((mut prev, _)) = pending {
        write(
            &mut prev,
            Duration::from_millis(1000 / u64::from(spec.fps.max(1))),
            &mut encoder,
        )?;
    }
    drop(encoder);
    if count == 0 {
        return Err("The recording has no frames.".into());
    }
    Ok(Written {
        path: spec.path.clone(),
        frames: count,
        duration: last,
        bytes: std::fs::metadata(&spec.path).map(|m| m.len()).unwrap_or(0),
    })
}

/// `pixels` drawn at `w` × `h` as RGBA bytes.
pub fn rgba_of(pixels: &Pixels, w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let space = CGColorSpace::new_device_rgb();
        let context = CGBitmapContextCreate(
            out.as_mut_ptr().cast(),
            w as usize,
            h as usize,
            8,
            (w * 4) as usize,
            space.as_deref(),
            RGBA,
        )?;
        CGContext::draw_image(
            Some(&context),
            CGRect::new(
                CGPoint::new(0., 0.),
                CGSize::new(f64::from(w), f64::from(h)),
            ),
            Some(pixels.image()),
        );
    }
    Some(out)
}
