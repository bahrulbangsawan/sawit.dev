//! mesurer 0.2.7's MP4 export muxes the file itself (`core/mp4-boxes.ts`), and writes two headers
//! short: `mvhd` by 28 bytes and `tkhd` by 4, their matrices stopping at 8 of 9 numbers and
//! `mvhd` without its `pre_defined`. ffmpeg reads such a file; AVFoundation (QuickTime, Quick Look)
//! refuses it as damaged. [`repair_mp4`] writes the two headers whole and moves the chunk offsets
//! with them; any other file is left as it is.

/// The identity matrix of `mvhd` and `tkhd`, in 16.16 and 2.30 fixed point.
const UNITY: [u32; 9] = [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x4000_0000];
/// Boxes whose payload is boxes, on the way to `mvhd`, `tkhd` and `stco`.
const CONTAINERS: [&[u8; 4]; 7] = [
    b"moov", b"trak", b"mdia", b"minf", b"stbl", b"dinf", b"edts",
];
/// A version 0 `mvhd`'s payload, and mesurer's.
const MVHD: usize = 100;
const MVHD_SHORT: usize = MVHD - 28;
/// A version 0 `tkhd`'s payload, and mesurer's.
const TKHD: usize = 84;
const TKHD_SHORT: usize = TKHD - 4;

/// The boxes in `bytes`, each its type and payload, or `None` when they don't tile it (64-bit
/// and to-the-end sizes included: mesurer writes neither).
fn boxes(bytes: &[u8]) -> Option<Vec<([u8; 4], &[u8])>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let head = bytes.get(at..at + 8)?;
        let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        if size < 8 {
            return None;
        }
        let kind = [head[4], head[5], head[6], head[7]];
        out.push((kind, bytes.get(at + 8..at + size)?));
        at += size;
    }
    Some(out)
}

fn push_box(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
}

fn unity() -> impl Iterator<Item = u8> {
    UNITY.into_iter().flat_map(u32::to_be_bytes)
}

/// `payload` of a container with mesurer's short headers made whole; `fixed` counts them.
fn rewrite(payload: &[u8], fixed: &mut usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(payload.len() + 32);
    for (kind, inner) in boxes(payload)? {
        let whole: Vec<u8>;
        let body = match &kind {
            b"mvhd" if inner.len() == MVHD_SHORT && inner[0] == 0 => {
                // Version and flags, times, scale, duration, rate, volume and reserved (36);
                // the matrix; pre_defined; the next track's id.
                whole = inner[..36]
                    .iter()
                    .copied()
                    .chain(unity())
                    .chain([0; 24])
                    .chain(inner[MVHD_SHORT - 4..].iter().copied())
                    .collect();
                *fixed += 1;
                &whole[..]
            }
            b"tkhd" if inner.len() == TKHD_SHORT && inner[0] == 0 => {
                // Version and flags up to the reserved after the volume (40); the matrix; width
                // and height.
                whole = inner[..40]
                    .iter()
                    .copied()
                    .chain(unity())
                    .chain(inner[TKHD_SHORT - 8..].iter().copied())
                    .collect();
                *fixed += 1;
                &whole[..]
            }
            k if CONTAINERS.contains(&k) => {
                whole = rewrite(inner, fixed)?;
                &whole[..]
            }
            _ => inner,
        };
        push_box(&mut out, &kind, body);
    }
    Some(out)
}

/// `payload` with each `stco` offset at or past `from` moved by `by`.
fn move_chunks(payload: &[u8], from: u32, by: u32) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(payload.len());
    for (kind, inner) in boxes(payload)? {
        let moved: Vec<u8>;
        let body = match &kind {
            b"stco" => {
                let count = u32::from_be_bytes(inner.get(4..8)?.try_into().ok()?) as usize;
                let table = inner.get(8..8 + count * 4)?;
                moved = inner[..8]
                    .iter()
                    .copied()
                    .chain(table.as_chunks::<4>().0.iter().flat_map(|o| {
                        let o = u32::from_be_bytes(*o);
                        (if o >= from { o + by } else { o }).to_be_bytes()
                    }))
                    .chain(inner[8 + count * 4..].iter().copied())
                    .collect();
                &moved[..]
            }
            k if CONTAINERS.contains(&k) => {
                moved = move_chunks(inner, from, by)?;
                &moved[..]
            }
            _ => inner,
        };
        push_box(&mut out, &kind, body);
    }
    Some(out)
}

/// mesurer's MP4 with its `mvhd` and `tkhd` whole, and the chunks after them where they moved;
/// `None` for a file that doesn't need it (any other MP4, any other file).
pub fn repair_mp4(bytes: &[u8]) -> Option<Vec<u8>> {
    let top = boxes(bytes)?;
    let mut out = Vec::with_capacity(bytes.len() + 32);
    let mut fixed = 0;
    let mut at = 0;
    for (kind, payload) in &top {
        let end = at + 8 + payload.len();
        if kind == b"moov" {
            let whole = rewrite(payload, &mut fixed)?;
            let by = u32::try_from(whole.len() - payload.len()).ok()?;
            let whole = move_chunks(&whole, u32::try_from(end).ok()?, by)?;
            push_box(&mut out, kind, &whole);
        } else {
            push_box(&mut out, kind, payload);
        }
        at = end;
    }
    (fixed > 0).then_some(out)
}
