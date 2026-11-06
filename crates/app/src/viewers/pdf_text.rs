//! A PDF's text, for ⌘F: PDFKit reads each page's text layer, and the rectangles a match covers on
//! its page. The pages themselves are drawn by CoreGraphics (`sawit_core::pdf`); PDFKit is part of
//! macOS, linked here and called through the Objective-C runtime, so it adds no crate. A page
//! scanned to an image has no text layer, and finds nothing.

use std::path::Path;

use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSRange, NSRect, NSString, NSURL};
use sawit_core::find::{Regex, find_all};

#[link(name = "PDFKit", kind = "framework")]
unsafe extern "C" {}

/// PDFKit's crop box, the part of the page the viewer shows.
const CROP_BOX: isize = 1;

/// A match: its page (0-based), and the rectangles it covers there as fractions of the page shown,
/// from its top left: `(x, y, width, height)`.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfHit {
    pub page: usize,
    pub rects: Vec<(f32, f32, f32, f32)>,
}

/// Every match of `re` in the PDF at `path`, page by page. `None` when PDFKit can't open it.
pub fn find_in_pdf(path: &Path, re: &Regex) -> Option<Vec<PdfHit>> {
    let class = AnyClass::get(c"PDFDocument")?;
    let url = NSURL::from_file_path(path)?;
    let doc: Option<Retained<AnyObject>> = unsafe {
        let doc: Allocated<AnyObject> = msg_send![class, alloc];
        msg_send![doc, initWithURL: &*url]
    };
    let doc = doc?;
    let count: isize = unsafe { msg_send![&*doc, pageCount] };
    let mut hits = Vec::new();
    for index in 0..count.max(0) {
        let page: Option<Retained<AnyObject>> = unsafe { msg_send![&*doc, pageAtIndex: index] };
        let Some(page) = page else { continue };
        let text: Option<Retained<NSString>> = unsafe { msg_send![&*page, string] };
        let Some(text) = text.map(|t| t.to_string()) else {
            continue;
        };
        let crop: NSRect = unsafe { msg_send![&*page, boundsForBox: CROP_BOX] };
        let rotation: isize = unsafe { msg_send![&*page, rotation] };
        for range in find_all(&text, re) {
            let (start, len) = utf16_range(&text, range);
            let selection: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*page, selectionForRange: NSRange::new(start, len)] };
            let rects = selection
                .map(|selection| line_rects(&selection, &page, crop, rotation))
                .unwrap_or_default();
            hits.push(PdfHit {
                page: index as usize,
                rects,
            });
        }
    }
    Some(hits)
}

/// `range` of `text` (bytes) as NSString's UTF-16 start and length.
fn utf16_range(text: &str, range: std::ops::Range<usize>) -> (usize, usize) {
    let start = text[..range.start].encode_utf16().count();
    let len = text[range].encode_utf16().count();
    (start, len)
}

/// The rectangles of `selection` on `page`, a line each, as fractions of the page shown. A page
/// turned a quarter is left without: its rectangles would need turning too.
fn line_rects(
    selection: &AnyObject,
    page: &AnyObject,
    crop: NSRect,
    rotation: isize,
) -> Vec<(f32, f32, f32, f32)> {
    if rotation.rem_euclid(360) != 0 || crop.size.width <= 0.0 || crop.size.height <= 0.0 {
        return Vec::new();
    }
    let lines: Option<Retained<objc2_foundation::NSArray<AnyObject>>> =
        unsafe { msg_send![selection, selectionsByLine] };
    let lines: Vec<Retained<AnyObject>> = lines.map(|l| l.to_vec()).unwrap_or_default();
    lines
        .iter()
        .map(|line| {
            let r: NSRect = unsafe { msg_send![&**line, boundsForPage: page] };
            let (w, h) = (crop.size.width, crop.size.height);
            let x = (r.origin.x - crop.origin.x) / w;
            let top = 1.0 - (r.origin.y + r.size.height - crop.origin.y) / h;
            (
                x as f32,
                top as f32,
                (r.size.width / w) as f32,
                (r.size.height / h) as f32,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::utf16_range;

    #[test]
    fn ranges_count_utf16_units() {
        // "é" is two bytes and one unit; "😀" four bytes and two units.
        let text = "é😀 needle";
        let start = text.find("needle").unwrap();
        assert_eq!(utf16_range(text, start..start + 6), (4, 6));
    }
}
