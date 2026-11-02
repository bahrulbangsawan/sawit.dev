//! How long and how strongly an agent shows on a surface after its last op there, and how its
//! cursor glides.

use std::time::Duration;

/// How long an agent stays on a surface at full strength after its last op.
pub const FADE_AFTER: Duration = Duration::from_secs(5);
/// How long the fade itself takes.
pub const FADE_TIME: Duration = Duration::from_secs(1);
/// How strong a faded agent stays until it goes.
pub const FADED: f32 = 0.4;
/// How long after its last op an agent leaves a surface.
pub const GONE_AFTER: Duration = Duration::from_secs(30);

/// How strong an agent shows on a surface `idle` after its last op there: whole for
/// [`FADE_AFTER`], then down to [`FADED`] over [`FADE_TIME`], and gone (`None`) after
/// [`GONE_AFTER`].
pub fn opacity(idle: Duration) -> Option<f32> {
    if idle >= GONE_AFTER {
        return None;
    }
    if idle <= FADE_AFTER {
        return Some(1.0);
    }
    let into = (idle - FADE_AFTER).as_secs_f32() / FADE_TIME.as_secs_f32();
    if into >= 1.0 {
        return Some(FADED);
    }
    Some(1.0 - (1.0 - FADED) * into)
}

/// Whether the agent seen `idle` ago still shows the words of what it does: only while it is
/// at full strength.
pub fn shows_action(idle: Duration) -> bool {
    idle <= FADE_AFTER
}

/// How far along a glide of `total` is `elapsed` into it, eased out (fast, then settling), from
/// 0 to 1. A glide of no length is over at once, as every glide is under reduced motion.
pub fn glide(elapsed: Duration, total: Duration) -> f64 {
    if total.is_zero() || elapsed >= total {
        return 1.0;
    }
    let t = elapsed.as_secs_f64() / total.as_secs_f64();
    1.0 - (1.0 - t).powi(3)
}
