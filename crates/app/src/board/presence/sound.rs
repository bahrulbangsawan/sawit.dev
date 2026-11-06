//! The build's sounds (30f), off unless the user turns them on: short, quiet clicks made at
//! runtime (no audio files, no new dependency), panned to where the cursor is. A click when an
//! element is placed, a softer tick while text is typed, a double click when an element snaps.
//! Sawit's own: a decaying noise burst through a one-pole filter, written as a 16-bit stereo WAV
//! and played by AppKit's NSSound.

use std::cell::RefCell;
use std::time::Instant;

use figcraft_engine::ViewState;
use gpui_kit::App;

use super::replay::script::Kind;
use crate::board::BoardView;

/// How many sounds one frame plays at most.
const PER_FRAME: usize = 3;

impl super::replay::Replays {
    /// The sounds the replays make between the last call and `now`: a click as an element is
    /// placed, a tick as typing starts, a snap as a lane catches up all at once; each with the
    /// page point it comes from.
    pub fn sounds(&mut self, now: Instant) -> Vec<(Sound, (f64, f64))> {
        let since = self.heard.replace(now).unwrap_or(now);
        let mut out = std::mem::take(&mut self.snapped);
        for lane in &self.lanes {
            for t in &lane.tasks {
                if t.task.cover || matches!(t.task.kind, Kind::Fade) {
                    continue;
                }
                let sound = match t.task.kind {
                    Kind::Text { .. } => Sound::Tick,
                    _ => Sound::Click,
                };
                // A box clicks where it lands; text ticks as typing starts.
                let at = if sound == Sound::Tick { t.start } else { t.end };
                if at > since && at <= now {
                    out.push((sound, (t.task.rect[0], t.task.rect[1])));
                }
            }
        }
        out
    }
}

impl BoardView {
    /// Plays the build's sounds since the last frame, when the user turned them on, panned to
    /// where on the canvas (`view`) each comes from. Silent under reduced motion and with the
    /// cursors hidden; the replays' clock moves on either way.
    pub(in crate::board) fn play_build_sounds(&mut self, view: &ViewState, now: Instant, cx: &App) {
        let events = self.agents.replays.sounds(now);
        let on = crate::agents::build_sound(cx)
            && crate::agents::shown(cx)
            && !crate::agents::reduce_motion(cx);
        if !on || events.is_empty() {
            return;
        }
        for (sound, (x, y)) in events.into_iter().take(PER_FRAME) {
            let (sx, _) = view.page_to_screen(x, y);
            let pan = (sx / view.vw.max(1.0) * 2.0 - 1.0).clamp(-1.0, 1.0) as f32 * 0.8;
            play(sound, pan);
        }
    }
}

/// What made the sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::board) enum Sound {
    Click,
    Tick,
    Snap,
}

/// The sample rate, the loudest a sound gets (of full scale), and how many play at once at most.
const RATE: u32 = 44_100;
const PEAK: f32 = 0.18;
const AT_ONCE: usize = 8;

/// `sound` as a WAV file, panned to `pan` (−1 left, 0 centre, 1 right).
pub(in crate::board) fn wav(sound: Sound, pan: f32) -> Vec<u8> {
    let (ms, bright, gap): (f32, f32, Option<f32>) = match sound {
        Sound::Click => (9.0, 0.55, None),
        Sound::Tick => (4.0, 0.8, None),
        Sound::Snap => (8.0, 0.6, Some(45.0)),
    };
    let peak = if sound == Sound::Tick {
        PEAK * 0.45
    } else {
        PEAK
    };
    let burst = (RATE as f32 * ms / 1000.0) as usize;
    let second = gap.map(|g| (RATE as f32 * g / 1000.0) as usize);
    let total = second.map_or(burst, |s| s + burst);
    let mut mono = vec![0f32; total];
    // A small xorshift: the same click every time, no randomness crate.
    let mut seed: u32 = 0x9e37_79b9;
    let mut noise = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let mut write = |at: usize, mono: &mut [f32]| {
        let mut low = 0f32;
        for i in 0..burst {
            let decay = (-(i as f32) / (burst as f32 / 5.0)).exp();
            low += bright * (noise() - low);
            mono[at + i] += low * decay * peak;
        }
    };
    write(0, &mut mono);
    if let Some(s) = second {
        write(s, &mut mono);
    }
    let pan = pan.clamp(-1.0, 1.0);
    let (left, right) = (((1.0 - pan) / 2.0).sqrt(), ((1.0 + pan) / 2.0).sqrt());
    let mut pcm = Vec::with_capacity(total * 4);
    for v in mono {
        for gain in [left, right] {
            let s = (v * gain).clamp(-1.0, 1.0);
            pcm.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
        }
    }
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(&pcm);
    out
}

#[cfg(all(target_os = "macos", not(test)))]
thread_local! {
    /// The sounds playing: NSSound plays only while it is held.
    static PLAYING: RefCell<Vec<objc2::rc::Retained<objc2_app_kit::NSSound>>> = const { RefCell::new(Vec::new()) };
}

/// Plays `sound` panned to `pan`, on the UI thread.
#[cfg(all(target_os = "macos", not(test)))]
pub(in crate::board) fn play(sound: Sound, pan: f32) {
    use objc2::AllocAnyThread as _;
    let bytes = wav(sound, pan);
    let data = objc2_foundation::NSData::with_bytes(&bytes);
    let Some(player) = objc2_app_kit::NSSound::initWithData(objc2_app_kit::NSSound::alloc(), &data)
    else {
        return;
    };
    player.play();
    PLAYING.with(|playing| {
        let mut playing = playing.borrow_mut();
        if playing.len() >= AT_ONCE {
            playing.remove(0);
        }
        playing.push(player);
    });
}

/// Tests stay silent.
#[cfg(any(not(target_os = "macos"), test))]
pub(in crate::board) fn play(_sound: Sound, _pan: f32) {
    let _ = RefCell::new(());
    let _ = AT_ONCE;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_is_a_short_quiet_stereo_wav_panned_to_its_side() {
        let wav = wav(Sound::Click, -1.0);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        let samples: Vec<i16> = wav[44..]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let right_max = samples
            .iter()
            .skip(1)
            .step_by(2)
            .map(|s| s.unsigned_abs())
            .max()
            .unwrap();
        let left_max = samples
            .iter()
            .step_by(2)
            .map(|s| s.unsigned_abs())
            .max()
            .unwrap();
        assert_eq!(right_max, 0, "all the way left");
        assert!(
            left_max > 0 && (left_max as f32) < i16::MAX as f32 * 0.3,
            "quiet: {left_max}"
        );
        assert!(samples.len() / 2 < RATE as usize / 50, "under 20 ms");
        assert!(
            super::wav(Sound::Snap, 0.0).len() > wav.len(),
            "a snap clicks twice"
        );
    }
}
