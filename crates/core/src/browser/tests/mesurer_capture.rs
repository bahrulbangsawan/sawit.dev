//! Lane 1 (mesurer-capture): tests of what Sawit does with mesurer's captures and colours: the
//! pure helpers (`mesurer::files`) here, the glue's overrides in `glue.rs`.

use std::path::Path;

use crate::browser::mesurer::files::{
    AVC_LEVELS, Container, FrameSize, avc_codec_for, avc_fits, captures_dir, captures_folder,
    fixed_name, frame_size, png_size, save_capture, shown_folder, sniff, srgb_hex,
};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x01\xe0\0\0\x00\x50\x08\x06\0\0\0";
const GIF: &[u8] = b"GIF89a\x01\0\x01\0";
const MP4: &[u8] = b"\0\0\0\x18ftypmp42\0\0\0\0isommp42";
const MOV: &[u8] = b"\0\0\0\x14ftypqt  \0\0\0\0qt  ";
const WEBM: &[u8] = b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81\x01\x42\x82\x84webm\x42\x87";
const MKV: &[u8] = b"\x1a\x45\xdf\xa3\x9f\x42\x82\x88matroska";

/// Each container is known by its first bytes, and a name gets the extension they call for:
/// WebKit's MP4 named `.webm` by mesurer is saved `.mp4`; a name that is right already, or bytes
/// Sawit doesn't know, keep the name; a name with no extension takes its MIME type's.
#[test]
fn downloads_are_named_for_what_their_bytes_are() {
    assert_eq!(sniff(PNG), Some(Container::Png));
    assert_eq!(sniff(GIF), Some(Container::Gif));
    assert_eq!(sniff(b"\xff\xd8\xff\xe0"), Some(Container::Jpeg));
    assert_eq!(sniff(MP4), Some(Container::Mp4));
    assert_eq!(sniff(MOV), Some(Container::Mov));
    assert_eq!(sniff(WEBM), Some(Container::Webm));
    assert_eq!(sniff(MKV), Some(Container::Mkv));
    assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(Container::Webp));
    assert_eq!(sniff(b"hello"), None);
    assert_eq!(sniff(b""), None);

    let cases = [
        (
            "mesurer-recording-1.webm",
            "video/mp4",
            MP4,
            "mesurer-recording-1.mp4",
        ),
        (
            "mesurer-recording-1.mp4",
            "video/mp4",
            MP4,
            "mesurer-recording-1.mp4",
        ),
        (
            "mesurer-recording-1.webm",
            "video/webm",
            WEBM,
            "mesurer-recording-1.webm",
        ),
        (
            "mesurer-recording-1.mp4",
            "",
            WEBM,
            "mesurer-recording-1.webm",
        ),
        (
            "mesurer-recording-1.gif",
            "image/gif",
            GIF,
            "mesurer-recording-1.gif",
        ),
        ("clip.webm", "", GIF, "clip.gif"),
        (
            "mesurer-2026-11-03-101112.png",
            "image/png",
            PNG,
            "mesurer-2026-11-03-101112.png",
        ),
        ("shot.PNG", "", PNG, "shot.PNG"),
        ("photo.jpeg", "", b"\xff\xd8\xff\xdb", "photo.jpeg"),
        ("take.mp4", "", MOV, "take.mov"),
        ("no-extension", "", PNG, "no-extension.png"),
        ("v1.2.final", "", MP4, "v1.2.mp4"),
        ("notes.txt", "text/plain", b"hi", "notes.txt"),
        ("notes", "text/plain;charset=utf-8", b"hi", "notes.txt"),
        ("blob", "", b"??", "blob"),
    ];
    for (name, mime, bytes, want) in cases {
        assert_eq!(fixed_name(name, mime, bytes), want, "{name} {mime}");
    }
}

/// A name from the page can't leave the captures folder, hide, or run on.
#[test]
fn download_names_are_made_safe() {
    assert_eq!(fixed_name("../../etc/passwd", "", b""), "passwd");
    assert_eq!(fixed_name("a\\b\\..\\c.png", "", PNG), "c.png");
    assert_eq!(fixed_name(".hidden.png", "", PNG), "hidden.png");
    assert_eq!(fixed_name("..", "", PNG), "mesurer-capture.png");
    assert_eq!(fixed_name("", "image/png", PNG), "mesurer-capture.png");
    assert_eq!(fixed_name("", "", b""), "mesurer-capture");
    assert_eq!(fixed_name("a:b\u{7}\n.png", "", PNG), "ab.png");
    let long = format!("{}.webm", "x".repeat(300));
    let fixed = fixed_name(&long, "", MP4);
    assert!(fixed.chars().count() <= 120, "{fixed}");
    assert!(fixed.ends_with("xxx.mp4"), "{fixed}");
}

/// Captures go to `<project>/.nyawit/browser/captures` (made, with `.nyawit/`'s ignore file), or
/// `~/Downloads/sawit-captures` with no project, under free names; never through a link.
#[test]
fn the_captures_folder_is_the_projects_and_refuses_links() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let p = project.path();
    let folder = captures_folder(Some(p), home.path()).unwrap();
    assert_eq!(folder, p.join(".nyawit/browser/captures"));
    assert!(folder.is_dir() && p.join(".nyawit/.gitignore").is_file());
    assert_eq!(
        shown_folder(Some(p), home.path(), &folder),
        ".nyawit/browser/captures"
    );

    let first = save_capture(Some(p), home.path(), "a.webm", "video/mp4", MP4).unwrap();
    let second = save_capture(Some(p), home.path(), "a.webm", "video/mp4", MP4).unwrap();
    assert_eq!(first, folder.join("a.mp4"));
    assert_eq!(second, folder.join("a-2.mp4"));
    assert_eq!(std::fs::read(&first).unwrap(), MP4);

    // No project: the home's Downloads.
    let none = captures_folder(None, home.path()).unwrap();
    assert_eq!(none, home.path().join("Downloads/sawit-captures"));
    assert_eq!(captures_dir(None, home.path()), none);
    assert_eq!(
        shown_folder(None, home.path(), &none),
        "~/Downloads/sawit-captures"
    );
    assert_eq!(
        shown_folder(None, Path::new("/elsewhere"), &none),
        none.display().to_string()
    );

    // A linked captures folder, or a linked folder above it, is refused, and nothing lands there.
    let elsewhere = tempfile::tempdir().unwrap();
    let linked = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(linked.path().join(".nyawit/browser")).unwrap();
    std::os::unix::fs::symlink(
        elsewhere.path(),
        linked.path().join(".nyawit/browser/captures"),
    )
    .unwrap();
    let refused = save_capture(Some(linked.path()), home.path(), "a.png", "", PNG);
    assert!(refused.unwrap_err().contains("link"));
    let above = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), above.path().join(".nyawit")).unwrap();
    assert!(
        captures_folder(Some(above.path()), home.path())
            .unwrap_err()
            .contains("link")
    );
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}

/// A sampled colour, in sRGB components, as `#rrggbb`: rounded, clamped, never NaN.
#[test]
fn colours_are_srgb_hex() {
    assert_eq!(srgb_hex(1., 0., 0.), "#ff0000");
    assert_eq!(srgb_hex(0., 0., 0.), "#000000");
    assert_eq!(srgb_hex(1., 1., 1.), "#ffffff");
    assert_eq!(srgb_hex(0.2, 0.4, 0.6), "#336699");
    assert_eq!(srgb_hex(0.501, 0.499, 0.0019), "#807f00");
    assert_eq!(srgb_hex(1.2, -0.3, f64::NAN), "#ff0000");
}

/// A recording's frame is the view at the display's scale, brought down to `max_width` (between
/// 16 and 3840) when narrower, its proportions kept; the snapshot is asked for in points.
#[test]
fn frames_are_the_view_at_most_max_width() {
    let frame = |w, h, s, m| frame_size(w, h, s, m);
    assert_eq!(
        frame(1280., 800., 2., Some(1920)),
        FrameSize {
            points: 960.,
            width: 1920,
            height: 1200
        }
    );
    assert_eq!(
        frame(1280., 800., 2., None),
        FrameSize {
            points: 1280.,
            width: 2560,
            height: 1600
        }
    );
    assert_eq!(
        frame(1280., 800., 1., Some(1920)),
        FrameSize {
            points: 1280.,
            width: 1280,
            height: 800
        }
    );
    assert_eq!(
        frame(1280., 800., 2., Some(640)),
        FrameSize {
            points: 320.,
            width: 640,
            height: 400
        }
    );
    // Too small or too big a cap, and a view with no size.
    assert_eq!(frame(1280., 800., 2., Some(0)).width, 16);
    assert_eq!(frame(4000., 1000., 2., Some(100_000)).width, 3840);
    assert_eq!(
        frame(0., f64::NAN, 0., Some(640)),
        FrameSize {
            points: 1.,
            width: 1,
            height: 1
        }
    );
    assert_eq!(frame(1001., 333., 2., Some(1000)).height, 333);
}

/// A PNG's size comes from its header; other bytes have none.
#[test]
fn png_sizes_come_from_the_header() {
    assert_eq!(png_size(PNG), Some((480, 80)));
    assert_eq!(png_size(&PNG[..20]), None);
    assert_eq!(png_size(GIF), None);
}

/// H.264's levels, by the frame's macroblocks and macroblocks a second: the table, the guard the
/// WebKit probe found (a level 3.0 or 3.1 frame too big fails to encode), and the level mesurer
/// is given instead.
#[test]
fn avc_levels_fit_the_frame() {
    assert_eq!(AVC_LEVELS.len(), 19);
    assert!(
        AVC_LEVELS
            .windows(2)
            .all(|w| w[0].0 < w[1].0 && w[0].1 <= w[1].1)
    );
    // 1280×720 is 3600 macroblocks: 3.1's most at 30 fps.
    assert!(avc_fits(0x1f, 1280, 720, 30.));
    assert!(!avc_fits(0x1f, 1280, 736, 30.));
    assert!(!avc_fits(0x1e, 1280, 720, 30.));
    // 3.0 takes 1620 macroblocks a frame, 40 500 a second: at 30 fps, 1350.
    assert!(avc_fits(0x1e, 720, 480, 30.));
    assert!(!avc_fits(0x1e, 720, 576, 30.));
    assert!(avc_fits(0x1e, 720, 576, 25.));
    assert!(!avc_fits(0x1e, 720, 576, f64::NAN));
    // A side no longer than √(8 × the frame's): 3.0 allows 113 macroblocks across.
    assert!(!avc_fits(0x1e, 16 * 114, 16, 30.));
    assert!(!avc_fits(0x2a + 1, 16, 16, 30.), "no such level");

    // The probe's guard always holds: a frame over 1620 macroblocks never gets 3.0, over 3600 never
    // 3.1 (or below).
    for (w, h) in [
        (640, 480),
        (1140, 500),
        (1280, 800),
        (1920, 1080),
        (2560, 1600),
        (5120, 2880),
    ] {
        let mbs = u32::div_ceil(w, 16) * u32::div_ceil(h, 16);
        for codec in ["avc1.42001E", "avc1.42001F", "avc1.4D001F", "avc1.64001F"] {
            let given = avc_codec_for(codec, w, h, 30.);
            let level = given
                .as_deref()
                .map(|c| u8::from_str_radix(&c[9..], 16).unwrap());
            if let Some(level) = level {
                assert!(
                    !(level <= 0x1e && mbs > 1620) && !(level <= 0x1f && mbs > 3600),
                    "{codec} {w}x{h}"
                );
                assert!(avc_fits(level, w, h, 30.));
                assert_eq!(
                    &given.as_deref().unwrap()[..9],
                    &codec[..9],
                    "the profile is kept"
                );
            }
        }
    }
    assert_eq!(
        avc_codec_for("avc1.42001E", 640, 480, 30.).unwrap(),
        "avc1.42001E"
    );
    assert_eq!(
        avc_codec_for("avc1.42001E", 1280, 720, 30.).unwrap(),
        "avc1.42001f"
    );
    assert_eq!(
        avc_codec_for("avc1.64001F", 1920, 1080, 30.).unwrap(),
        "avc1.640028"
    );
    assert_eq!(
        avc_codec_for("avc1.4D001F", 2560, 1600, 30.).unwrap(),
        "avc1.4D0032"
    );
    assert_eq!(avc_codec_for("avc1.42001E", 8192, 8192, 30.), None);
    assert_eq!(avc_codec_for("vp8", 8192, 8192, 30.).unwrap(), "vp8");
    assert_eq!(
        avc_codec_for("avc1.4200", 8192, 8192, 30.).unwrap(),
        "avc1.4200"
    );
}

mod glue;

/// An MP4 laid out as mesurer 0.2.7's muxer writes it (`core/mp4-boxes.ts`): `mvhd` and `tkhd`
/// short, the one chunk's offset pointing at `mdat`'s payload.
fn mesurer_mp4(samples: &[&[u8]]) -> Vec<u8> {
    let b = |kind: &str, parts: &[&[u8]]| -> Vec<u8> {
        let payload = parts.concat();
        [
            &((8 + payload.len()) as u32).to_be_bytes()[..],
            kind.as_bytes(),
            &payload,
        ]
        .concat()
    };
    let u = |v: u32| v.to_be_bytes();
    let unit = [&u(0x10000)[..], &[0; 12], &u(0x10000), &[0; 12]].concat();
    let ftyp = b("ftyp", &[b"isom", &u(0x200), b"isomiso2avc1mp41"]);
    let mvhd_body = [
        &[0; 4][..],
        &[0; 8],
        &u(90_000),
        &u(9_000),
        &u(0x10000),
        &[1, 0],
        &[0; 10],
        &unit,
        &u(2),
    ]
    .concat();
    let mvhd = b("mvhd", &[&mvhd_body]);
    let tkhd_body = [
        &[0, 0, 0, 7][..],
        &[0; 8],
        &u(1),
        &[0; 4],
        &u(9_000),
        &[0; 8],
        &[0; 8],
        &unit,
        &u(900 << 16),
        &u(370 << 16),
    ]
    .concat();
    let tkhd = b("tkhd", &[&tkhd_body]);
    let sizes: Vec<u8> = samples.iter().flat_map(|s| u(s.len() as u32)).collect();
    let stsz = b("stsz", &[&[0; 4], &u(0), &u(samples.len() as u32), &sizes]);
    let moov_len = |stco: &[u8]| {
        let stbl = b("stbl", &[&stsz, stco]);
        let trak = b("trak", &[&tkhd, &b("mdia", &[&b("minf", &[&stbl])])]);
        b("moov", &[&mvhd, &trak])
    };
    let offset = ftyp.len() + moov_len(&b("stco", &[&[0; 4], &u(1), &u(0)])).len() + 8;
    let moov = moov_len(&b("stco", &[&[0; 4], &u(1), &u(offset as u32)]));
    [ftyp, moov, b("mdat", &[&samples.concat()])].concat()
}

/// mesurer's MP4 gets its `mvhd` (108 bytes) and `tkhd` (92) whole, with the identity matrix,
/// the same times, size and track, and its chunk still at `mdat`'s payload; a file that is right
/// already, or isn't MP4, is left alone. Saved, mesurer's MP4 is the repaired one.
#[test]
fn mesurers_mp4_is_saved_with_whole_headers() {
    use crate::browser::mesurer::files::repair_mp4;
    let broken = mesurer_mp4(&[b"frame-one", b"frame-two"]);
    let fixed = repair_mp4(&broken).expect("it needs repair");
    assert_eq!(fixed.len(), broken.len() + 32);
    let find = |bytes: &[u8], kind: &[u8]| bytes.windows(4).position(|w| w == kind).unwrap() - 4;
    let size = |bytes: &[u8], at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    let mvhd = find(&fixed, b"mvhd");
    assert_eq!(size(&fixed, mvhd), 108);
    assert_eq!(size(&fixed, mvhd + 20), 90_000, "timescale");
    assert_eq!(size(&fixed, mvhd + 24), 9_000, "duration");
    assert_eq!(
        size(&fixed, mvhd + 76),
        0x4000_0000,
        "the matrix's last number"
    );
    assert_eq!(size(&fixed, mvhd + 104), 2, "next track id");
    let tkhd = find(&fixed, b"tkhd");
    assert_eq!(size(&fixed, tkhd), 92);
    assert_eq!(
        size(&fixed, tkhd + 80),
        0x4000_0000,
        "the matrix's last number"
    );
    assert_eq!(
        (size(&fixed, tkhd + 84), size(&fixed, tkhd + 88)),
        (900 << 16, 370 << 16)
    );
    let moov = find(&fixed, b"moov");
    assert_eq!(
        size(&fixed, moov) as usize,
        size(&broken, find(&broken, b"moov")) as usize + 32
    );
    let chunk = size(&fixed, find(&fixed, b"stco") + 16) as usize;
    assert_eq!(&fixed[chunk..chunk + 9], b"frame-one");
    assert_eq!(&fixed[find(&fixed, b"mdat") + 8..], b"frame-oneframe-two");

    assert_eq!(repair_mp4(&fixed), None, "whole already");
    assert_eq!(repair_mp4(PNG), None);
    assert_eq!(repair_mp4(&broken[..broken.len() - 3]), None, "cut short");

    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let saved = save_capture(
        Some(project.path()),
        home.path(),
        "clip.mp4",
        "video/mp4",
        &broken,
    )
    .unwrap();
    assert_eq!(std::fs::read(saved).unwrap(), fixed);
}
