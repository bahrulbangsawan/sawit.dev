//! PDF pages for the viewer, rendered with macOS CoreGraphics: page sizes up
//! front, then one page at a time into BGRA pixels at the width shown.

use std::path::Path;

use objc2_core_foundation::{CFRetained, CFURL, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo,
    CGPDFBox, CGPDFDocument, CGPDFPage,
};

use crate::{CoreError, Result};

/// The tallest page we render (pixels), so a huge page can't exhaust memory.
const MAX_HEIGHT: u32 = 8000;

/// One rendered page: `width × height` pixels, BGRA, premultiplied, rows top
/// to bottom (the layout GPUI images use on macOS).
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

fn open(path: &Path) -> Result<CFRetained<CGPDFDocument>> {
    let url = CFURL::from_file_path(path)
        .ok_or_else(|| CoreError::Parse(format!("path PDF tidak valid: {}", path.display())))?;
    let doc = CGPDFDocument::with_url(Some(&url))
        .filter(|d| CGPDFDocument::number_of_pages(Some(d)) > 0)
        .ok_or_else(|| CoreError::Parse("PDF tidak bisa dibuka (rusak atau terkunci).".into()))?;
    Ok(doc)
}

/// Page size in points as displayed (crop box, rotation applied).
fn displayed_size(page: &CGPDFPage) -> (f64, f64) {
    let rect = CGPDFPage::box_rect(Some(page), CGPDFBox::CropBox);
    let (w, h) = (rect.size.width.abs(), rect.size.height.abs());
    if CGPDFPage::rotation_angle(Some(page)).rem_euclid(180) == 90 {
        (h, w)
    } else {
        (w, h)
    }
}

/// Displayed size of every page, in points.
pub fn page_sizes(path: &Path) -> Result<Vec<(f64, f64)>> {
    let doc = open(path)?;
    let count = CGPDFDocument::number_of_pages(Some(&doc));
    // Page numbers are 1-based in CoreGraphics.
    (1..=count)
        .map(|n| {
            CGPDFDocument::page(Some(&doc), n)
                .map(|p| displayed_size(&p))
                .ok_or_else(|| CoreError::Parse(format!("halaman {n} tidak terbaca")))
        })
        .collect()
}

/// Renders page `index` (0-based) `width` pixels wide on white paper.
pub fn render_page(path: &Path, index: usize, width: u32) -> Result<Bitmap> {
    let doc = open(path)?;
    let page = CGPDFDocument::page(Some(&doc), index + 1)
        .ok_or_else(|| CoreError::Parse(format!("halaman {} tidak ada", index + 1)))?;
    let (pw, ph) = displayed_size(&page);
    if pw <= 0.0 || ph <= 0.0 || width == 0 {
        return Err(CoreError::Parse("ukuran halaman tidak valid".into()));
    }
    let scale = width as f64 / pw;
    let height = ((ph * scale).round() as u32).clamp(1, MAX_HEIGHT);
    let (w, h) = (width as usize, height as usize);
    let mut bgra = vec![0u8; w * h * 4];
    let space = CGColorSpace::new_device_rgb();
    // BGRA in memory = 32-bit little-endian, alpha first.
    let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
    // SAFETY: `bgra` outlives the context and has exactly w*h*4 bytes.
    let ctx = unsafe {
        CGBitmapContextCreate(
            bgra.as_mut_ptr().cast(),
            w,
            h,
            8,
            w * 4,
            space.as_deref(),
            info,
        )
    }
    .ok_or_else(|| CoreError::Parse("tidak bisa menyiapkan gambar halaman".into()))?;
    let all = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64));
    CGContext::set_rgb_fill_color(Some(&ctx), 1.0, 1.0, 1.0, 1.0);
    CGContext::fill_rect(Some(&ctx), all);
    CGContext::scale_ctm(Some(&ctx), scale, scale);
    // Maps the crop box (and rotation) onto a page-sized rect at the origin.
    let target = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(pw, ph));
    let transform = CGPDFPage::drawing_transform(Some(&page), CGPDFBox::CropBox, target, 0, true);
    CGContext::concat_ctm(Some(&ctx), transform);
    CGContext::clip_to_rect(
        Some(&ctx),
        CGPDFPage::box_rect(Some(&page), CGPDFBox::CropBox),
    );
    CGContext::draw_pdf_page(Some(&ctx), Some(&page));
    drop(ctx);
    Ok(Bitmap {
        width,
        height,
        bgra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-page PDF: page 1 is 200×100 pt with a black square at the
    /// bottom left, page 2 is 100×200 pt and blank. Offsets are computed so
    /// the cross-reference table is valid.
    fn sample_pdf(path: &std::path::Path) {
        let content = "0 0 0 rg 0 0 50 50 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn reads_page_sizes() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("a.pdf");
        sample_pdf(&file);
        assert_eq!(
            page_sizes(&file).unwrap(),
            vec![(200.0, 100.0), (100.0, 200.0)]
        );
    }

    #[test]
    fn renders_a_page_at_the_requested_width() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("a.pdf");
        sample_pdf(&file);
        let page = render_page(&file, 0, 400).unwrap();
        assert_eq!((page.width, page.height), (400, 200));
        assert_eq!(page.bgra.len(), 400 * 200 * 4);
        let px = |x: usize, y: usize| {
            let i = (y * page.width as usize + x) * 4;
            &page.bgra[i..i + 4]
        };
        // Top-left is white paper; the square sits at the bottom left
        // (PDF y grows upward, image rows grow downward).
        assert_eq!(px(150, 20), [255, 255, 255, 255]);
        assert_eq!(px(20, 180), [0, 0, 0, 255]);
    }

    #[test]
    fn bad_files_and_pages_are_errors() {
        let d = tempfile::tempdir().unwrap();
        let junk = d.path().join("junk.pdf");
        std::fs::write(&junk, "not a pdf").unwrap();
        assert!(page_sizes(&junk).is_err());
        let file = d.path().join("a.pdf");
        sample_pdf(&file);
        assert!(render_page(&file, 9, 100).is_err());
    }
}
