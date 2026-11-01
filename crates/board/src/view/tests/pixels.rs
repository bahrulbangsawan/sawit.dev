//! The pixels: the dot grid, the page's colour on screen, the image for the GPU, pixel preview
//! and soft-proof sRGB.

use super::*;

/// A pixmap of one row of premultiplied RGBA pixels.
fn pixmap(pixels: &[[u8; 4]]) -> Pixmap {
    let mut pixmap = Pixmap::new(pixels.len() as u32, 1).unwrap();
    pixmap.data_mut().copy_from_slice(pixels.as_flattened());
    pixmap
}

/// `width` × `height` px of the page from `origin`, at `scale` px per page unit.
fn page_area(origin: (f64, f64), scale: f64, width: u32, height: u32) -> View {
    View {
        origin,
        scale,
        width,
        height,
    }
}

/// One distinct opaque BGRA colour for each pixel of a `width` × `height` image.
fn numbered(width: u32, height: u32) -> Vec<u8> {
    (0..width * height)
        .flat_map(|n| [n as u8, 10, 20, 255])
        .collect()
}

/// The pixel at (`x`, `y`) of a BGRA image `width` px wide.
fn bgra_at(bgra: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    bgra[at..at + 4].try_into().unwrap()
}

#[test]
fn dots_double_until_far_enough() {
    assert_eq!(dots(0.25, [255; 3]).unwrap().step, 96.0);
    assert_eq!(dots(1.0, [255; 3]).unwrap().step, 24.0);
}

#[test]
fn dots_are_dark_on_a_light_page_and_light_on_a_dark_one() {
    let light = dots(1.0, [255; 3]).unwrap();
    assert_eq!(light.color.to_rgba8(), [0, 0, 0, 46]); // black at 18%
    assert_eq!(light.faded.to_rgba8(), [0, 0, 0, 33]); // (24 - 14) / 14 of that
    assert_eq!(light.size, 1.5);
    let dark = dots(1.0, [30; 3]).unwrap();
    assert_eq!(dark.color.to_rgba8(), [255, 255, 255, 56]); // white at 22%
    assert_eq!(dark.faded.to_rgba8(), [255, 255, 255, 40]);
    // Luma decides, not the average of the channels: this green is light.
    assert_eq!(dots(1.0, [0, 200, 0]).unwrap().color.to_rgba8()[0], 0);
    assert_eq!(dots(1.0, [0, 0, 255]).unwrap().color.to_rgba8()[0], 255);
}

#[test]
fn dots_size_is_a_point_to_three_on_screen_and_they_fade_in_between_gaps() {
    // Size in page units: 1.5√zoom device px, kept between 1 and 3, over the zoom.
    assert_eq!(dots(0.25, [255; 3]).unwrap().size, 4.0);
    assert_eq!(dots(16.0, [255; 3]).unwrap().size, 3.0 / 16.0);
    // From 28 px apart on screen the dots in between show in full.
    let far_apart = dots(2.0, [255; 3]).unwrap();
    assert_eq!(far_apart.faded, far_apart.color);
    // At the minimum gap they have just faded out.
    let nearest = dots(14.0 / 24.0, [255; 3]).unwrap();
    assert_eq!(nearest.step, 24.0);
    assert_eq!(nearest.faded.to_rgba8()[3], 0);
}

#[test]
fn dots_are_none_without_a_usable_zoom() {
    for zoom in [
        0.0,
        -1.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e-320,
    ] {
        assert!(dots(zoom, [255; 3]).is_none(), "zoom {zoom}");
    }
}

#[test]
fn page_rgb_is_the_pages_background() {
    let (mut doc, page) = page_with(&[(0.0, 0.0, 10.0, 10.0)]);
    // A new page has FigCraft's default background, #F5F5F5.
    assert_eq!(page_rgb(&doc, page), [0xF5; 3]);
    if let Some(NodeKind::Page { background, .. }) = doc.node_mut(page).map(|n| &mut n.kind) {
        *background = Color::from_rgba8(30, 40, 50, 128);
    }
    assert_eq!(page_rgb(&doc, page), [30, 40, 50]);
    // Not a page, or no such node: white.
    let layer = doc.node(page).unwrap().children[0];
    assert_eq!(page_rgb(&doc, layer), [255; 3]);
    assert_eq!(page_rgb(&doc, NodeId(9999)), [255; 3]);
}

#[test]
fn screen_rgb_leaves_srgb_alone_and_converts_display_p3() {
    // An sRGB document's colours are the screen's already: they come back as they are.
    for v in 0..=255 {
        let rgb = [v, 255 - v, v / 2];
        assert_eq!(screen_rgb(ColorProfile::Srgb, rgb), rgb);
    }
    // A Display P3 colour is converted to sRGB, where this orange has more red and less blue.
    // Within 2 a channel: the colour is rounded to 8 bits on the way in and on the way out.
    let shown = screen_rgb(ColorProfile::DisplayP3, [200, 100, 50]);
    let srgb: [u8; 3] = [215, 93, 31];
    assert!(
        shown.iter().zip(srgb).all(|(s, e)| s.abs_diff(e) <= 2),
        "{shown:?}, not about {srgb:?}"
    );
}

#[test]
fn to_bgra_composites_and_swaps() {
    let opaque_red = pixmap(&[[255, 0, 0, 255]]);
    assert_eq!(to_bgra(&opaque_red, [10, 20, 30]), [0, 0, 255, 255]);
    let transparent = pixmap(&[[0, 0, 0, 0]]);
    assert_eq!(to_bgra(&transparent, [10, 20, 30]), [30, 20, 10, 255]);
}

#[test]
fn to_bgra_shows_the_page_through_the_alpha_that_is_left() {
    // Premultiplied: red at half alpha is (128, 0, 0, 128), and half the page shows through.
    let row = pixmap(&[[128, 0, 0, 128], [0, 0, 0, 0], [10, 20, 30, 255]]);
    assert_eq!(
        to_bgra(&row, [255, 255, 255]),
        [127, 127, 255, 255, 255, 255, 255, 255, 30, 20, 10, 255]
    );
    // A pixel can't hold more than its alpha: a broken one is clamped, not wrapped.
    assert_eq!(to_bgra(&pixmap(&[[255, 255, 255, 0]]), [200; 3]), [255; 4]);
}

#[test]
fn a_frame_renders_to_an_opaque_image_of_its_size_with_dots_on_the_page() {
    let (doc, page) = page_with(&[]);
    let rgb = page_rgb(&doc, page);
    let vp = Viewport {
        width: 100.0,
        height: 80.0,
        scale_factor: 1.0,
    };
    let frame = render_view(&view(0.0, 0.0, 1.0, 100.0, 80.0), vp, 1.0).unwrap();
    let opts = figcraft_render::Options {
        background: Some(Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255)),
        dots: dots(1.0, rgb),
        ..Default::default()
    };
    let pixmap = figcraft_render::render_view(&doc, page, &frame, &opts).unwrap();
    assert_eq!(
        (pixmap.width(), pixmap.height()),
        (frame.width, frame.height)
    );

    let bgra = to_bgra(&pixmap, rgb);
    assert_eq!(bgra.len(), (frame.width * frame.height * 4) as usize);
    let (pixels, _) = bgra.as_chunks::<4>();
    assert!(pixels.iter().all(|px| px[3] == 255));
    // An empty light page is the page colour with sparse, darker dots on it.
    let page_px = [rgb[2], rgb[1], rgb[0], 255];
    let darker = pixels.iter().filter(|px| px[2] < rgb[0]).count();
    let other = pixels.iter().filter(|px| **px != page_px).count();
    assert!(darker > 0, "no dots");
    assert_eq!(darker, other, "everything but the page colour is a dot");
    assert!(other < pixels.len() / 10, "{other} of {}", pixels.len());
}

#[test]
fn pixel_preview_renders_the_screens_area_at_its_own_resolution() {
    // 400 × 300 device px at 8 px per page unit: 50 × 37.5 page units.
    let screen = page_area((-10.25, 5.5), 8.0, 400, 300);
    let one = pixel_view(&screen, 1.0).unwrap();
    assert_eq!(one.origin, screen.origin);
    assert_eq!(one.scale, 1.0);
    // One more each way: the render snaps its origin down onto its own pixels.
    assert_eq!((one.width, one.height), (51, 39));
    let two = pixel_view(&screen, 2.0).unwrap();
    assert_eq!((two.scale, two.width, two.height), (2.0, 101, 76));
    // Only coarser than the screen: at 8 px per unit and up the screen shows the pixels.
    for pixels in [8.0, 16.0, 0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(pixel_view(&screen, pixels).is_none(), "{pixels}");
    }
    assert!(pixel_view(&page_area((0.0, 0.0), f64::NAN, 400, 300), 1.0).is_none());
}

#[test]
fn magnify_shows_each_pixel_as_a_block() {
    // A 3 × 2 image at 1 px per unit, over a screen at 4: each pixel is a 4 × 4 block.
    let (from, to) = (
        page_area((0.0, 0.0), 1.0, 3, 2),
        page_area((0.0, 0.0), 4.0, 12, 8),
    );
    let image = numbered(3, 2);
    let screen = magnify(&image, &from, &to);
    assert_eq!(screen.len(), 12 * 8 * 4);
    for y in 0..8 {
        for x in 0..12 {
            assert_eq!(
                bgra_at(&screen, 12, x, y),
                bgra_at(&image, 3, x / 4, y / 4),
                "({x}, {y})"
            );
        }
    }
}

#[test]
fn magnify_takes_the_pixel_under_each_screen_pixels_centre() {
    // The screen starts half a unit into the image and runs past its right side: its first
    // two columns are still under the image's first pixel, and its last ones take the
    // image's last column.
    let (from, to) = (
        page_area((0.0, 0.0), 1.0, 3, 1),
        page_area((0.5, 0.0), 4.0, 12, 2),
    );
    let image = numbered(3, 1);
    let screen = magnify(&image, &from, &to);
    let columns: Vec<u8> = (0..12).map(|x| bgra_at(&screen, 12, x, 1)[0]).collect();
    assert_eq!(columns, [0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2]);
    // Nothing to lay over: no pixels, and no panic.
    assert!(magnify(&image, &from, &page_area((0.0, 0.0), 4.0, 0, 0)).is_empty());
    let short = magnify(&image[..4], &from, &to);
    assert_eq!(short.len(), 12 * 2 * 4);
}

#[test]
fn soft_proof_stripes_what_srgb_cannot_show() {
    // Display P3's own green is past sRGB's; this orange is in both ([`screen_rgb`]'s test).
    let (green, orange) = ([0, 255, 0, 255], [200, 100, 50, 255]);
    let mut row = vec![green; 16];
    row.extend([orange; 4]);
    let pixmap = pixmap(&row);
    let clean = to_bgra(&pixmap, [255; 3]);

    let mut proofed = clean.clone();
    mark_outside_srgb(&mut proofed, &pixmap, ColorProfile::DisplayP3);
    for x in 0..16 {
        // Diagonal stripes, four pixels on and four off along a row.
        let expected = if x % 8 < 4 {
            PROOF_GREY
        } else {
            bgra_at(&clean, 20, x, 0)
        };
        assert_eq!(bgra_at(&proofed, 20, x, 0), expected, "{x}");
    }
    // What sRGB can show is left alone, and so is all of an sRGB document.
    assert_eq!(proofed[16 * 4..], clean[16 * 4..]);
    let mut srgb = clean.clone();
    mark_outside_srgb(&mut srgb, &pixmap, ColorProfile::Srgb);
    assert_eq!(srgb, clean);
}
