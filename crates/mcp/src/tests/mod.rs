//! The server as an agent meets it: its tools and what they say, and the helpers every tool's
//! answer goes through.

use crate::{clip, utc_time};

mod app_control;
mod canvas;
mod server;

#[test]
fn clip_cuts_on_char_boundary() {
    assert_eq!(clip("abc".into(), 10), "abc");
    let cut = clip("ééé".into(), 3);
    assert!(cut.starts_with("é\n"), "{cut}");
    assert!(cut.contains("2 of 6 bytes"));
}

#[test]
fn utc_dates() {
    assert_eq!(utc_time(0), "1970-01-01 00:00");
    assert_eq!(utc_time(1_700_000_000), "2023-11-14 22:13");
    assert_eq!(utc_time(951_782_400), "2000-02-29 00:00");
}
