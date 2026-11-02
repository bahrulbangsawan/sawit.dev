//! The System tab's tests: the export block's lines and the radius samples.

use super::export::export_lines;
use super::sample_radius;
use crate::browser::design::{ExportDone, ExportShown};
use gpui_kit::px;
use std::path::PathBuf;

fn shown(outcome: Option<Result<ExportDone, String>>, done: usize, total: usize) -> ExportShown {
    ExportShown {
        folder: PathBuf::from("/p/.nyawit/design/a.dev"),
        done,
        total,
        outcome,
    }
}

#[test]
fn export_text_follows_the_job() {
    assert_eq!(
        export_lines(&shown(None, 18, 40)),
        ["Downloading assets… 18 / 40"]
    );
    let done = ExportDone {
        files: 30,
        failed: 12,
        skipped_videos: 0,
        screenshot_error: Some("off screen".into()),
    };
    let lines = export_lines(&shown(Some(Ok(done)), 40, 40));
    assert_eq!(lines[0], "Exported 30 files to /p/.nyawit/design/a.dev");
    assert_eq!(lines[1], "12 of 40 assets couldn't be downloaded");
    assert_eq!(lines[2], "No screenshot: off screen");
    // Skipped videos are said apart, not counted as failures.
    let done = ExportDone {
        files: 30,
        failed: 0,
        skipped_videos: 3,
        screenshot_error: None,
    };
    let lines = export_lines(&shown(Some(Ok(done)), 40, 40));
    assert_eq!(lines[1..], ["3 videos weren't included"]);
    let done = ExportDone {
        files: 30,
        failed: 2,
        skipped_videos: 1,
        screenshot_error: None,
    };
    let lines = export_lines(&shown(Some(Ok(done)), 40, 40));
    assert_eq!(
        lines[1..],
        [
            "2 of 39 assets couldn't be downloaded",
            "1 video wasn't included"
        ]
    );
}

#[test]
fn radius_samples_stay_readable() {
    assert_eq!(sample_radius("6px"), px(6.));
    assert_eq!(sample_radius("980px"), px(14.));
    assert_eq!(sample_radius("50%"), px(14.));
}
