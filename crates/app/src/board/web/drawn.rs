//! Whether a snapshot shows the page whole (B17): a picture taken while the page was still drawing
//! itself in (an entrance animation, a reveal on a timer) is its own background colour from some
//! line down, where the page has content. The picture's rows are read once, off the main thread,
//! as it is converted ([`Rows::of`]); the page's probe says how far down its content goes
//! ([`super::settle::PROBE`]'s `contentBottom`); a band of flat rows over the lower part of that
//! content is a half-drawn picture ([`Rows::whole`]), which is taken again.

use serde_json::{Value, json};

/// Where the band that is looked at starts, as a share of the content's height: what is above is
/// the part most pages draw first (a status bar, a header), and below it a page always has more
/// than background.
const BAND_FROM: f64 = 0.45;
/// The share of flat rows in the band past which the picture is half-drawn. A page whose lower
/// half is mostly empty still has a button or a line of text there: a few rows in a hundred.
const FLAT_LIMIT: f64 = 0.985;
/// How far two pixels' channels may differ (summed) and still count as the same colour: the
/// snapshot's own dithering and gradients' banding.
const SAME: u32 = 12;
/// The columns looked at in a row: every this-many pixels.
const STEP: usize = 3;
/// How many times a half-drawn snapshot is taken again after a load, each a settle later
/// ([`RETAKE_AFTER`] × the retake's number): a page that reveals itself on timers gets a few
/// seconds; past that its picture is kept as it is.
pub(in crate::board) const RETAKES: u32 = 3;
pub(in crate::board) const RETAKE_AFTER: std::time::Duration =
    std::time::Duration::from_millis(700);

/// Which rows of a picture are flat (one colour edge to edge), top to bottom.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::board) struct Rows {
    pub flat: Vec<bool>,
}

impl Rows {
    /// The rows of `pixels`, a `width` × `height` picture of 4-byte pixels (BGRA or RGBA alike).
    pub fn of(pixels: &[u8], width: usize, height: usize) -> Rows {
        if width == 0 || pixels.len() < width * height * 4 {
            return Rows::default();
        }
        let flat = (0..height)
            .map(|y| {
                let row = &pixels[y * width * 4..(y + 1) * width * 4];
                let first = &row[..4];
                row.as_chunks::<4>()
                    .0
                    .iter()
                    .step_by(STEP)
                    .all(|px| near(px, first))
            })
            .collect();
        Rows { flat }
    }

    /// Whether the picture shows the page whole, its content `content` of `page` CSS px down
    /// (the page's probe): `None` when it can't be told (no content, no rows).
    pub fn whole(&self, content: f64, page: f64) -> Option<bool> {
        let share = self.flat_share(content, page)?;
        Some(share < FLAT_LIMIT)
    }

    /// The share of flat rows in the band of the content that is looked at.
    pub fn flat_share(&self, content: f64, page: f64) -> Option<f64> {
        let rows = self.flat.len();
        if rows == 0 || page < 1.0 || content < 8.0 {
            return None;
        }
        let scale = rows as f64 / page;
        let end = ((content.min(page)) * scale).round() as usize;
        let start = ((content * BAND_FROM) * scale).round() as usize;
        let band = &self.flat[start.min(rows)..end.min(rows)];
        if band.len() < 8 {
            return None;
        }
        Some(band.iter().filter(|flat| **flat).count() as f64 / band.len() as f64)
    }

    /// What the `web` op's status says of the picture, for the content and page sizes the probe
    /// gave.
    pub fn report(&self, probe: Option<&Value>) -> Value {
        let (content, page) = content_of(probe);
        let share = content.and_then(|c| self.flat_share(c, page?));
        let whole = content.and_then(|c| self.whole(c, page?));
        json!({"whole": whole, "flat_share": share.map(|s| (s * 1000.0).round() / 1000.0),
               "content_bottom": content})
    }
}

/// How far down the page's content goes, and how tall its viewport is, from its probe.
pub(in crate::board) fn content_of(probe: Option<&Value>) -> (Option<f64>, Option<f64>) {
    let get = |key: &str| probe.and_then(|p| p.get(key)).and_then(Value::as_f64);
    (get("contentBottom"), get("innerHeight"))
}

fn near(a: &[u8], b: &[u8]) -> bool {
    let d = |i: usize| u32::from(a[i].abs_diff(b[i]));
    d(0) + d(1) + d(2) + d(3) <= SAME
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` × `h` picture of `bg`, with a strip of `ink` every 40 rows down to `drawn` rows.
    fn picture(w: usize, h: usize, drawn: usize) -> Vec<u8> {
        let (bg, ink) = ([14u8, 15, 18, 255], [240u8, 240, 240, 255]);
        let mut pixels = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let text = y < drawn && y % 40 < 14 && x > 20 && x < w - 20 && x % 7 < 4;
                pixels.extend_from_slice(if text { &ink } else { &bg });
            }
        }
        pixels
    }

    #[test]
    fn a_page_drawn_whole_is_whole() {
        let rows = Rows::of(&picture(200, 400, 400), 200, 400);
        assert_eq!(rows.whole(400.0, 400.0), Some(true));
    }

    #[test]
    fn a_page_cut_off_halfway_is_half_drawn() {
        // The content goes all the way down, but the picture stops at a third: the rest is the
        // page's background.
        let rows = Rows::of(&picture(200, 400, 130), 200, 400);
        assert_eq!(rows.whole(400.0, 400.0), Some(false));
    }

    #[test]
    fn a_short_page_is_judged_by_its_content_only() {
        // Content down to 120 px of a 400 px page: the empty rest is no sign of anything.
        let rows = Rows::of(&picture(200, 400, 120), 200, 400);
        assert_eq!(rows.whole(120.0, 400.0), Some(true));
    }

    #[test]
    fn a_sparse_lower_half_with_one_button_is_whole() {
        let (w, h) = (100, 800);
        let mut pixels = picture(w, h, 200);
        // A 48-row button at the bottom.
        for y in 740..788 {
            for x in 10..90 {
                let i = (y * w + x) * 4;
                pixels[i..i + 4].copy_from_slice(&[124, 217, 146, 255]);
            }
        }
        let rows = Rows::of(&pixels, w, h);
        assert_eq!(rows.whole(800.0, 800.0), Some(true));
    }

    #[test]
    fn a_picture_at_twice_the_css_size_maps_its_rows() {
        let rows = Rows::of(&picture(100, 800, 260), 100, 800);
        assert_eq!(
            rows.whole(400.0, 400.0),
            Some(false),
            "260 of 800 px is 130 CSS px"
        );
        assert_eq!(rows.whole(120.0, 400.0), Some(true));
    }

    #[test]
    fn nothing_to_tell_says_none() {
        assert_eq!(Rows::default().whole(400.0, 400.0), None);
        let rows = Rows::of(&picture(10, 10, 10), 10, 10);
        assert_eq!(rows.whole(0.0, 10.0), None);
    }
}
