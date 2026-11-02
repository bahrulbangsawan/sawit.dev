//! The work items' tests: their states, ids and words, and the palette they are drawn in.

use std::time::Duration;

use super::work::*;
use super::{LIGHT_PAGE, PALETTE};

const fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// WCAG's contrast ratio of two `#rrggbb` colours.
fn contrast(a: &str, b: &str) -> f64 {
    let luminance = |hex: &str| {
        let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
        let channel = |shift: u32| {
            let c = f64::from((value >> shift) & 0xff) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    };
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Twelve colours, each its own, each carrying white text and reading on the light page at
/// 4.5:1 or more.
#[test]
fn the_palette_has_twelve_colours_that_read() {
    assert_eq!(PALETTE.len(), 12);
    for (i, colour) in PALETTE.iter().enumerate() {
        assert!(contrast(colour, "#ffffff") >= 4.5, "{colour} under white");
        assert!(contrast(colour, LIGHT_PAGE) >= 4.5, "{colour} on the page");
        assert!(!PALETTE[..i].contains(colour), "{colour} twice");
    }
}

#[test]
fn statuses_are_read_as_progress_names_them() {
    assert_eq!(WorkStatus::parse("working", None), Ok(WorkStatus::Working));
    assert_eq!(WorkStatus::parse(" Done ", None), Ok(WorkStatus::Done));
    assert_eq!(
        WorkStatus::parse("failed", Some(" Couldn't load the logo ")),
        Ok(WorkStatus::Failed("Couldn't load the logo".into()))
    );
    assert_eq!(
        WorkStatus::parse("failed", Some("")),
        Ok(WorkStatus::Failed("no reason given".into()))
    );
    let wrong = WorkStatus::parse("finished", None).unwrap_err();
    assert!(wrong.contains("working, done or failed"), "{wrong}");
    assert_eq!(WorkStatus::Failed("x".into()).name(), "failed");
}

/// A component is the only kind built now; a screen is refused with what to do instead.
#[test]
fn only_components_are_begun_for_now() {
    assert_eq!(check_kind(None), Ok("component"));
    assert_eq!(check_kind(Some("Component")), Ok("component"));
    let screen = check_kind(Some("screen")).unwrap_err();
    assert!(screen.contains("isn't built yet"), "{screen}");
    assert!(check_kind(Some("page")).is_err());
}

#[test]
fn work_ids_are_short_and_differ() {
    let one = new_work_id(1);
    assert!(one.starts_with("w_") && one.len() == 10, "{one}");
    assert_ne!(one, new_work_id(2));
    assert_eq!(one, new_work_id(1));
}

/// A running work stalls after a minute of quiet; a done one keeps the agent's name for 2 s, then
/// its check fades over 1.5 s; a failed one stays.
#[test]
fn rows_follow_their_work() {
    let working = WorkStatus::Working;
    assert_eq!(
        row_look(&working, ms(0), ms(59_000)),
        RowLook::Working { stalled: false }
    );
    assert_eq!(
        row_look(&working, ms(0), STALL_AFTER),
        RowLook::Working { stalled: true }
    );
    let done = WorkStatus::Done;
    assert_eq!(
        row_look(&done, ms(1_900), ms(0)),
        RowLook::Done {
            name: true,
            check: 1.0
        }
    );
    let RowLook::Done { name, check } = row_look(&done, ms(2_750), ms(0)) else {
        panic!("still done");
    };
    assert!(!name && check > 0.4 && check < 0.6, "{check}");
    assert_eq!(row_look(&done, ms(3_500), ms(0)), RowLook::Gone);
    let failed = WorkStatus::Failed("no".into());
    assert_eq!(row_look(&failed, ms(600_000), ms(600_000)), RowLook::Failed);
}

#[test]
fn state_changes_are_said_in_sentences() {
    assert_eq!(
        started("Codex", "Sign-in form"),
        "Codex started Sign-in form"
    );
    assert_eq!(
        finished("Codex", "Sign-in form"),
        "Codex finished Sign-in form"
    );
    assert_eq!(
        failed(
            "Gemini CLI",
            "Testimonial",
            "Couldn't load the avatar image."
        ),
        "Gemini CLI couldn't finish Testimonial: couldn't load the avatar image"
    );
    assert_eq!(
        failed("Codex", "Nav", "API timed out"),
        "Codex couldn't finish Nav: API timed out"
    );
    assert_eq!(failed("Codex", "Nav", " "), "Codex couldn't finish Nav");
    assert_eq!(stalled("Codex", "Nav"), "Codex stalled on Nav");
}

#[test]
fn the_roster_counts_what_is_on() {
    let tally = Tally {
        building: 3,
        done: 1,
        failed: 1,
        idle: 2,
        ..Tally::default()
    };
    assert_eq!(tally.words(), "3 building · 1 done · 1 failed");
    let stalled = Tally {
        stalled: 1,
        active: 4,
        ..Tally::default()
    };
    assert_eq!(stalled.words(), "1 stalled");
    let free = Tally {
        active: 2,
        ..Tally::default()
    };
    assert_eq!(free.words(), "2 agents at work");
    assert_eq!(Tally::default().words(), "");
}
