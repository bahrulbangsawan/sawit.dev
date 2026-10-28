//! Video player: AVPlayer decodes (and plays the sound); each display frame
//! we take the newest decoded frame and paint it with GPUI's `surface()`.
//! Plays what macOS plays (MP4/MOV/M4V with H.264/HEVC/ProRes…).

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState, SliderValue};
use gpui_kit::component::*;
use gpui_kit::*;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_av_foundation::{AVPlayer, AVPlayerItem, AVPlayerItemStatus, AVPlayerItemVideoOutput};
use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_media::CMTime;
use objc2_core_video::{
    CVPixelBuffer, kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

use super::image::file_name;
use super::toolbar;

/// Seek bar resolution (the slider can't change its range later).
const SEEK_STEPS: f32 = 1000.;

struct Player {
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    output: Retained<AVPlayerItemVideoOutput>,
}

pub struct VideoView {
    pub path: PathBuf,
    player: Option<Player>,
    frame: Option<CFRetained<CVPixelBuffer>>,
    playing: bool,
    muted: bool,
    /// The user is dragging the seek bar; playback doesn't move it meanwhile.
    seeking: bool,
    seek: Entity<SliderState>,
    error: Option<String>,
    _seek_events: Subscription,
}

/// `CFString` constants used as `NSString` keys (toll-free bridged).
fn key(s: &CFString) -> &NSString {
    // SAFETY: CFString and NSString are toll-free bridged.
    unsafe { &*(s as *const CFString as *const NSString) }
}

fn open(path: &std::path::Path, mtm: MainThreadMarker) -> Option<Player> {
    let url = NSURL::from_file_path(path)?;
    // Bi-planar 4:2:0 full range, IOSurface-backed: what GPUI's Metal
    // renderer paints without conversion.
    let format = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange);
    let metal = NSNumber::new_bool(true);
    let iosurface = NSDictionary::<NSString, AnyObject>::new();
    let attributes = unsafe {
        NSDictionary::<NSString, AnyObject>::from_slices(
            &[
                key(kCVPixelBufferPixelFormatTypeKey),
                key(kCVPixelBufferMetalCompatibilityKey),
                key(kCVPixelBufferIOSurfacePropertiesKey),
            ],
            &[
                &**format as &AnyObject,
                &**metal as &AnyObject,
                &**iosurface as &AnyObject,
            ],
        )
    };
    unsafe {
        let item = AVPlayerItem::playerItemWithURL(&url, mtm);
        let output = AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
            AVPlayerItemVideoOutput::alloc(),
            Some(&attributes),
        );
        item.addOutput(&output);
        let player = AVPlayer::playerWithPlayerItem(Some(&item), mtm);
        Some(Player {
            player,
            item,
            output,
        })
    }
}

fn seconds(t: CMTime) -> Option<f64> {
    let s = unsafe { t.seconds() };
    s.is_finite().then_some(s)
}

/// "1:05" or "1:02:03".
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).floor() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

impl VideoView {
    pub fn new(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let seek = cx.new(|_| SliderState::new().min(0.).max(SEEK_STEPS).default_value(0.));
        let events = cx.subscribe_in(&seek, window, |this, _, event: &SliderEvent, _, cx| {
            let (value, done) = match event {
                SliderEvent::Change(v) => (v, false),
                SliderEvent::Release(v) => (v, true),
            };
            this.seeking = !done;
            if let SliderValue::Single(v) = value {
                this.seek_to(*v / SEEK_STEPS);
            }
            cx.notify();
        });
        let player = MainThreadMarker::new().and_then(|mtm| open(&path, mtm));
        let error = player
            .is_none()
            .then(|| "Video tidak bisa dibuka.".to_string());
        Self {
            path,
            player,
            frame: None,
            playing: false,
            muted: false,
            seeking: false,
            seek,
            error,
            _seek_events: events,
        }
    }

    pub fn title(&self) -> String {
        file_name(&self.path)
    }

    fn duration(&self) -> Option<f64> {
        self.player
            .as_ref()
            .and_then(|p| seconds(unsafe { p.item.duration() }))
            .filter(|d| *d > 0.0)
    }

    fn position(&self) -> f64 {
        self.player
            .as_ref()
            .and_then(|p| seconds(unsafe { p.player.currentTime() }))
            .unwrap_or(0.0)
    }

    fn seek_to(&self, fraction: f32) {
        let (Some(p), Some(d)) = (&self.player, self.duration()) else {
            return;
        };
        let target = unsafe { CMTime::with_seconds(d * fraction.clamp(0., 1.) as f64, 600) };
        // Exact seeks so the scrubbed frame is the one shown.
        let zero = unsafe { CMTime::with_seconds(0.0, 600) };
        unsafe {
            p.player
                .seekToTime_toleranceBefore_toleranceAfter(target, zero, zero)
        };
    }

    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &self.player else { return };
        if self.playing {
            unsafe { p.player.pause() };
        } else {
            // At the end, start over.
            if let Some(d) = self.duration()
                && self.position() >= d - 0.05
            {
                self.seek_to(0.);
            }
            unsafe { p.player.play() };
        }
        self.playing = !self.playing;
        cx.notify();
    }

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &self.player {
            self.muted = !self.muted;
            unsafe { p.player.setMuted(self.muted) };
            cx.notify();
        }
    }

    /// Takes the newest decoded frame, if any, and keeps the clock moving.
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &self.player else { return };
        let status = unsafe { p.item.status() };
        if status == AVPlayerItemStatus::Failed {
            self.error = Some(
                "macOS tidak bisa memutar format ini (coba MP4/MOV dengan H.264 atau HEVC).".into(),
            );
            self.playing = false;
            return;
        }
        let now = unsafe { p.player.currentTime() };
        if unsafe { p.output.hasNewPixelBufferForItemTime(now) }
            && let Some(buffer) = unsafe {
                p.output
                    .copyPixelBufferForItemTime_itemTimeForDisplay(now, std::ptr::null_mut())
            }
        {
            self.frame = Some(CFRetained::from(buffer));
        }
        if let Some(d) = self.duration() {
            if self.playing && self.position() >= d - 0.01 {
                self.playing = false;
            }
            if !self.seeking {
                let value = (self.position() / d) as f32 * SEEK_STEPS;
                self.seek.update(cx, |s, cx| s.set_value(value, window, cx));
            }
        }
        // While playing (or until the first frame and duration arrive),
        // render again next frame.
        if self.playing
            || self.seeking
            || self.frame.is_none()
            || status != AVPlayerItemStatus::ReadyToPlay
        {
            window.request_animation_frame();
        }
    }
}

impl Drop for VideoView {
    fn drop(&mut self) {
        // Closing the tab must stop the sound.
        if let Some(p) = &self.player {
            unsafe { p.player.pause() };
        }
    }
}

impl Render for VideoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.error.is_none() {
            self.poll(window, cx);
        }
        let size = self
            .player
            .as_ref()
            .map(|p| unsafe { p.item.presentationSize() });
        let facts = size
            .filter(|s| s.width > 0.0)
            .map(|s| format!("{} × {}", s.width as u32, s.height as u32));
        let bar = toolbar(self.title(), cx).children(facts);
        let picture = match (&self.error, &self.frame) {
            (Some(e), _) => div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(e.clone())
                .into_any_element(),
            (None, Some(frame)) => surface(frame.clone())
                .object_fit(ObjectFit::Contain)
                .size_full()
                .into_any_element(),
            (None, None) => div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Memuat…")
                .into_any_element(),
        };
        let time = format!(
            "{} / {}",
            clock(self.position()),
            self.duration().map(clock).unwrap_or_else(|| "–:––".into())
        );
        let controls = h_flex()
            .h(px(36.))
            .px_3()
            .gap_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("video-play")
                    .ghost()
                    .small()
                    .icon(if self.playing {
                        Lucide::Pause
                    } else {
                        Lucide::Play
                    })
                    .tooltip("Putar / jeda (Spasi)")
                    .disabled(self.error.is_some())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx))),
            )
            .child(div().min_w(px(84.)).child(time))
            .child(div().flex_1().child(Slider::new(&self.seek)))
            .child(
                Button::new("video-mute")
                    .ghost()
                    .small()
                    .icon(if self.muted {
                        Lucide::VolumeX
                    } else {
                        Lucide::Volume2
                    })
                    .tooltip("Bisukan")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
            );
        v_flex()
            .id("video")
            .size_full()
            .bg(cx.theme().background)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "space" {
                    this.toggle_play(cx);
                    cx.stop_propagation();
                }
            }))
            .child(bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui_kit::black())
                    .child(picture),
            )
            .child(controls)
    }
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn clock_reads_like_a_player() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(65.9), "1:05");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(-3.0), "0:00");
    }
}
