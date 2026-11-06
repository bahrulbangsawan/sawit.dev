//! A page loading: the bar fills as WebKit reports, never goes back during one load and never shows
//! empty.

use crate::browser::progress::shown_progress;

#[test]
fn the_bar_only_moves_forward() {
    assert_eq!(
        shown_progress(0.08, 0.),
        0.08,
        "nothing read yet: the floor"
    );
    assert_eq!(shown_progress(0.08, 0.3), 0.3);
    assert_eq!(shown_progress(0.6, 0.3), 0.6, "WebKit's reading went back");
    assert_eq!(shown_progress(0.6, 1.4), 1.0);
}
