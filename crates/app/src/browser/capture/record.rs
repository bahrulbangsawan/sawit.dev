//! Native recordings: frames taken from WebKit's drawing at the size asked for (a device or
//! width × height, a resolution or a max width), each stamped with when it was taken, and
//! written by AVFoundation's hardware encoder ([`super::super::engine::video`]) as MP4 (H.264),
//! MOV (HEVC) or a GIF. One recording per tab at a time; it stops when asked, at its time cap,
//! or when the tab goes away, and the file is finished either way.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::capture::{VideoFormat, resolution};
use sawit_core::browser::{Target, resize_mode};
use serde_json::{Value, json};

use super::super::{BrowserView, OpResult};

mod session;

use session::start;

/// The longest a recording may run.
pub const MAX_SECONDS: u32 = 300;
/// A recording's cap when none is given.
pub const DEFAULT_SECONDS: u32 = 30;
/// The widest GIF, and its longest run: GIFs are big.
const GIF_MAX_WIDTH: u32 = 800;
const GIF_MAX_SECONDS: u32 = 30;
/// How long one frame's capture is waited for.
const FRAME_WAIT: Duration = Duration::from_secs(5);

/// What a recording covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecordMode {
    /// The part of the page on show.
    #[default]
    View,
    /// A scroll through the whole page, top to bottom.
    FullPage,
    /// One element's box.
    Element,
    /// A clip of the view, in CSS px.
    Region,
}

impl RecordMode {
    pub fn parse(text: &str) -> Result<RecordMode, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "view" | "visible" => Ok(RecordMode::View),
            "full_page" | "full-page" | "fullpage" | "page" => Ok(RecordMode::FullPage),
            "element" => Ok(RecordMode::Element),
            "region" => Ok(RecordMode::Region),
            other => Err(format!(
                "Unknown mode “{other}”. Use view, full_page, element or region."
            )),
        }
    }
}

/// What a recording is asked for.
#[derive(Clone, Debug)]
pub struct RecordAsk {
    pub device: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub resolution: Option<String>,
    pub max_width: Option<u32>,
    pub fps: u32,
    pub format: VideoFormat,
    pub mode: RecordMode,
    pub target: Target,
    pub clip: Option<[f64; 4]>,
    /// Full page: CSS px a second, or the whole scroll's length in seconds.
    pub scroll_speed: Option<f64>,
    pub duration: Option<f64>,
    pub max_seconds: u32,
    pub output_path: Option<String>,
}

impl Default for RecordAsk {
    fn default() -> Self {
        RecordAsk {
            device: None,
            width: None,
            height: None,
            resolution: None,
            max_width: None,
            fps: 30,
            format: VideoFormat::Mp4,
            mode: RecordMode::View,
            target: Target::default(),
            clip: None,
            scroll_speed: None,
            duration: None,
            max_seconds: DEFAULT_SECONDS,
            output_path: None,
        }
    }
}

impl RecordAsk {
    /// Checks the numbers before anything starts.
    pub fn check(&self) -> Result<(), String> {
        if !matches!(self.fps, 15 | 24 | 30 | 60) {
            return Err(format!("fps is 15, 24, 30 or 60, not {}.", self.fps));
        }
        if self.max_seconds == 0 || self.max_seconds > MAX_SECONDS {
            return Err(format!("max_seconds is 1 to {MAX_SECONDS}."));
        }
        if let Some(r) = &self.resolution
            && resolution(r).is_none()
        {
            return Err(format!(
                "Unknown resolution “{r}”. Use 360p, 480p, 720p, 1080p, 1440p or 2160p."
            ));
        }
        match self.mode {
            RecordMode::Element if self.target.is_empty() => {
                Err("An element recording needs a ref or a selector.".into())
            }
            RecordMode::Region if self.clip.is_none() => {
                Err("A region recording needs a clip (x, y, width, height).".into())
            }
            _ => Ok(()),
        }
    }
}

/// A recording under way, as the tab keeps it.
pub struct Recording {
    pub started: Instant,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub format: VideoFormat,
    pub mode: RecordMode,
    pub max: Duration,
    stop: Option<oneshot::Sender<()>>,
    waiters: Vec<oneshot::Sender<OpResult>>,
}

/// The tab's recordings: the one under way, and how the last one ended.
#[derive(Default)]
pub struct Recorder {
    pub current: Option<Recording>,
    pub last: Option<OpResult>,
}

impl Recording {
    pub fn status(&self) -> Value {
        json!({
            "recording": true,
            "seconds": (self.started.elapsed().as_secs_f64() * 10.).round() / 10.,
            "path": self.path.display().to_string(),
            "width": self.width,
            "height": self.height,
            "fps": self.fps,
            "format": self.format.extension(),
            "mode": format!("{:?}", self.mode).to_lowercase(),
            "max_seconds": self.max.as_secs(),
        })
    }
}

/// The full-page scroll: from the top to `travel` CSS px down, after a pause, at `speed` px a
/// second, then a pause at the bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scroll {
    pub travel: f64,
    pub speed: f64,
    pub pause: f64,
}

impl Scroll {
    /// Where the page is at `t` seconds, and whether the scroll is over.
    pub fn at(&self, t: f64) -> (f64, bool) {
        let moving = self.travel / self.speed.max(1.);
        let y = ((t - self.pause).max(0.) * self.speed).min(self.travel);
        (y, t >= self.pause * 2. + moving)
    }

    /// The scroll for a page `travel` px of scrolling: at `speed`, or over `duration` seconds,
    /// else about 600 px a second, within `max` seconds.
    pub fn plan(travel: f64, speed: Option<f64>, duration: Option<f64>, max: f64) -> Scroll {
        let pause = 1.;
        let room = (max - 2. * pause).max(1.);
        let speed = match (speed, duration) {
            (Some(s), _) if s > 0. => s,
            (_, Some(d)) if d > 0. => travel / d.min(room),
            _ => 600.,
        }
        .max(travel / room)
        .max(1.);
        Scroll {
            travel: travel.max(0.),
            speed,
            pause,
        }
    }
}

impl BrowserView {
    pub(in crate::browser) fn record_status(&self) -> Value {
        match &self.recorder.current {
            Some(r) => r.status(),
            None => {
                json!({ "recording": false, "last": self.recorder.last.clone().map(|r| r.unwrap_or_else(|e| json!({ "error": e }))) })
            }
        }
    }

    /// Starts recording; the reply says what (or why not).
    pub(in crate::browser) fn record_start(
        &mut self,
        ask: RecordAsk,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = ask.check() {
            let _ = reply.send(Err(e));
            return;
        }
        if self.recorder.current.is_some() {
            let _ = reply.send(Err("This tab is already recording: stop it first.".into()));
            return;
        }
        let mode = match (ask.device.is_some() || ask.width.is_some() || ask.height.is_some())
            .then(|| resize_mode(ask.device.as_deref(), ask.width, ask.height))
            .transpose()
        {
            Ok(m) => m,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let started = start(&this, ask, mode, cx).await;
            let _ = reply.send(started);
        })
        .detach();
    }

    /// Stops the recording; the reply comes once the file is finished.
    pub(in crate::browser) fn record_stop(
        &mut self,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        match &mut self.recorder.current {
            Some(r) => {
                r.waiters.push(reply);
                if let Some(stop) = r.stop.take() {
                    let _ = stop.send(());
                }
            }
            None => {
                let _ = reply.send(
                    self.recorder
                        .last
                        .clone()
                        .unwrap_or_else(|| Err("Nothing is recording.".into())),
                );
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{RecordAsk, RecordMode, Scroll};

    #[test]
    fn modes_and_numbers_are_checked() {
        assert_eq!(
            RecordMode::parse("full_page").unwrap(),
            RecordMode::FullPage
        );
        assert!(RecordMode::parse("movie").is_err());
        let refused = |ask: RecordAsk| ask.check().is_err();
        assert!(!refused(RecordAsk::default()));
        let fps = RecordAsk {
            fps: 25,
            ..Default::default()
        };
        assert!(refused(fps));
        let long = RecordAsk {
            max_seconds: 301,
            ..Default::default()
        };
        assert!(refused(long));
        let res = RecordAsk {
            resolution: Some("8k".into()),
            ..Default::default()
        };
        assert!(refused(res));
        let element = RecordAsk {
            mode: RecordMode::Element,
            ..Default::default()
        };
        assert!(refused(element));
        let region = RecordAsk {
            mode: RecordMode::Region,
            ..Default::default()
        };
        assert!(refused(region));
    }

    #[test]
    fn the_scroll_pauses_moves_and_ends() {
        let s = Scroll::plan(3000., None, Some(3.), 30.);
        assert_eq!(s.speed, 1000.);
        assert_eq!(s.at(0.5), (0., false));
        assert_eq!(s.at(2.5), (1500., false));
        assert_eq!(s.at(4.5).0, 3000.);
        assert!(s.at(5.).1);
        // Too slow to fit the cap: sped up to end in time.
        let fast = Scroll::plan(10_000., Some(100.), None, 12.);
        assert!(fast.speed >= 1000.);
        // No speed or duration: about 600 px a second.
        assert_eq!(Scroll::plan(1200., None, None, 30.).speed, 600.);
    }
}
