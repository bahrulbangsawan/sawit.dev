//! The palette and the semantic colors: the page's colors merged, clustered and ranked by use,
//! and the roles they take.

use super::Ramp;
use crate::site_design::color::{self, Rgba, ramp};
use crate::site_design::scan::{ColorRole, ColorUse, Scan};

const CHROMA: f64 = 0.03;
const CLUSTER: f64 = 0.02;
const HUE_GAP: f64 = 30.0;

/// A color with its uses summed: all of them, as a button background, as link text.
#[derive(Clone, Copy)]
struct Seen {
    color: Rgba,
    count: u32,
    button_bg: u32,
    link_text: u32,
}

fn chroma(c: Rgba) -> f64 {
    c.to_oklch().1
}

fn hue_gap(a: Rgba, b: Rgba) -> f64 {
    let d = (a.to_oklch().2 - b.to_oklch().2).abs() % 360.0;
    d.min(360.0 - d)
}

/// Parsed, fully transparent values dropped, exact duplicates merged; most-used first.
fn merged<'a>(uses: impl Iterator<Item = &'a ColorUse>) -> Vec<Seen> {
    let mut out: Vec<(String, Seen)> = Vec::new();
    for u in uses {
        let Some(color) = color::parse(&u.value) else {
            continue;
        };
        if color.a <= 0.0 {
            continue;
        }
        let button_bg = if u.role == ColorRole::Background {
            u.on_button
        } else {
            0
        };
        let link_text = if u.role == ColorRole::Text {
            u.on_link
        } else {
            0
        };
        let hex = color.to_hex();
        match out.iter_mut().find(|(h, _)| *h == hex) {
            Some((_, s)) => {
                s.count += u.count;
                s.button_bg += button_bg;
                s.link_text += link_text;
            }
            None => out.push((
                hex,
                Seen {
                    color,
                    count: u.count,
                    button_bg,
                    link_text,
                },
            )),
        }
    }
    let mut out: Vec<Seen> = out.into_iter().map(|(_, s)| s).collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.count));
    out
}

/// Alpha as a byte, the precision `to_hex` keeps.
fn alpha8(c: Rgba) -> u8 {
    (c.a.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Opaque enough to be a palette role: a translucent tint isn't the color the page shows.
fn opaque(c: Rgba) -> bool {
    c.a >= 0.99
}

/// Merges colors of equal alpha within ΔE_OK < 0.02 into the most-used one; most-used first.
fn clustered(seen: Vec<Seen>) -> Vec<Seen> {
    let mut out: Vec<Seen> = Vec::new();
    for s in seen {
        match out.iter_mut().find(|c| {
            alpha8(c.color) == alpha8(s.color) && color::delta_e(c.color, s.color) < CLUSTER
        }) {
            Some(c) => {
                c.count += s.count;
                c.button_bg += s.button_bg;
                c.link_text += s.link_text;
            }
            None => out.push(s),
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.count));
    out
}

fn in_role(scan: &Scan, role: ColorRole) -> Vec<Seen> {
    clustered(merged(scan.colors.iter().filter(|u| u.role == role)))
}

/// The least contrast `foreground` needs against `background`.
const FG_CONTRAST: f64 = 3.0;

/// Text colors, most-used first. "Most-used" is by text length (the samples'
/// `chars`) inside `main`, the scope `body-md` uses, so neither links nor
/// footer fine print make the foreground. With nothing in `main` all samples
/// count; without samples it falls back to element counts.
fn text_colors(scan: &Scan) -> Vec<Seen> {
    let any_main = scan.texts.iter().any(|t| t.in_main && t.chars > 0);
    let weighted: Vec<ColorUse> = scan
        .texts
        .iter()
        .filter(|t| t.chars > 0 && (t.in_main || !any_main))
        .map(|t| ColorUse {
            value: t.color.clone(),
            role: ColorRole::Text,
            count: t.chars,
            on_button: 0,
            on_link: 0,
        })
        .collect();
    if weighted.is_empty() {
        in_role(scan, ColorRole::Text)
    } else {
        clustered(merged(weighted.iter()))
    }
}

/// The first entry with the largest positive key.
fn most<'a>(items: &[&'a Seen], key: impl Fn(&Seen) -> u32) -> Option<&'a Seen> {
    let mut best: Option<&Seen> = None;
    for s in items {
        if key(s) > 0 && best.is_none_or(|b| key(s) > key(b)) {
            best = Some(s);
        }
    }
    best
}

pub(super) struct Colors {
    pub(super) palette: Vec<Ramp>,
    pub(super) semantic: Vec<(String, Rgba)>,
    pub(super) primary_buttons: u32,
    pub(super) primary_links: u32,
    pub(super) dark: bool,
    pub(super) seen: usize,
}

pub(super) fn derive_colors(scan: &Scan) -> Colors {
    let distinct = merged(scan.colors.iter());
    let seen = distinct.len();
    let all = clustered(distinct);
    let candidates = || all.iter().filter(|s| opaque(s.color));
    let chromatic: Vec<&Seen> = candidates().filter(|s| chroma(s.color) >= CHROMA).collect();
    let neutral = candidates().find(|s| chroma(s.color) < CHROMA);
    let primary = most(&chromatic, |s| s.button_bg)
        .or_else(|| most(&chromatic, |s| s.link_text))
        .or_else(|| chromatic.first().copied());
    let mut picked: Vec<Rgba> = primary.iter().map(|p| p.color).collect();
    let mut next = || {
        let p = primary?;
        let found = chromatic.iter().copied().find(|s| {
            !std::ptr::eq(*s, p) && picked.iter().all(|c| hue_gap(s.color, *c) >= HUE_GAP)
        })?;
        picked.push(found.color);
        Some(found)
    };
    let secondary = next();
    let accent = next();

    let palette = [
        ("primary", primary),
        ("secondary", secondary),
        ("accent", accent),
        ("neutral", neutral),
    ]
    .into_iter()
    .filter_map(|(role, s)| {
        let s = s?;
        Some(Ramp {
            role: role.to_string(),
            base: s.color,
            steps: ramp(s.color),
            uses: s.count,
        })
    })
    .collect();

    let background = scan
        .body_background
        .as_deref()
        .and_then(color::parse)
        .filter(|c| c.a > 0.0);
    let texts = text_colors(scan);
    let neutral_of = |v: Vec<Seen>| -> Vec<Rgba> {
        v.into_iter()
            .map(|s| s.color)
            .filter(|c| chroma(*c) < CHROMA)
            .collect()
    };
    // "Second most-used": the most-used neutral that isn't the background / foreground itself.
    let other_than = |v: Vec<Rgba>, taken: &[Option<Rgba>]| {
        v.into_iter().find(|c| {
            taken
                .iter()
                .flatten()
                .all(|f| color::delta_e(*c, *f) >= CLUSTER)
        })
    };
    // The text color *of the background*: the most-used one readable on it
    // (≥ 3:1), so light text on dark hero tiles doesn't win on a white page.
    let foreground = background
        .and_then(|bg| {
            texts
                .iter()
                .find(|s| color::contrast_ratio(s.color, bg) >= FG_CONTRAST)
        })
        .or(texts.first())
        .map(|s| s.color);
    let backgrounds = in_role(scan, ColorRole::Background);
    let first_bg = background.or_else(|| backgrounds.first().map(|s| s.color));
    let muted = other_than(neutral_of(backgrounds), &[first_bg]);
    // Text in the page's or the muted background color (white on buttons) can't be muted text.
    let muted_fg = other_than(neutral_of(texts), &[foreground, first_bg, muted]);
    let border = in_role(scan, ColorRole::Border).first().map(|s| s.color);
    let primary_c = primary.map(|p| p.color);
    let primary_fg = primary_c.map(|p| {
        let on_primary = scan.components.iter().filter_map(|c| {
            let bg = color::parse(&c.style.background)?;
            let fg = color::parse(&c.style.color).filter(|f| f.a > 0.0)?;
            (color::delta_e(bg, p) < CLUSTER).then_some(fg)
        });
        let mut counts: Vec<(Rgba, u32)> = Vec::new();
        for fg in on_primary {
            match counts.iter_mut().find(|(c, _)| c.to_hex() == fg.to_hex()) {
                Some((_, n)) => *n += 1,
                None => counts.push((fg, 1)),
            }
        }
        let best = counts.iter().fold(None::<(Rgba, u32)>, |b, x| match b {
            Some(b) if b.1 >= x.1 => Some(b),
            _ => Some(*x),
        });
        best.map(|(c, _)| c).unwrap_or_else(|| {
            let white = Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 1.0,
            };
            let black = Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 1.0,
            };
            if color::contrast_ratio(white, p) >= color::contrast_ratio(black, p) {
                white
            } else {
                black
            }
        })
    });
    let ring = scan
        .focus_ring
        .as_deref()
        .and_then(color::parse)
        .filter(|c| c.a > 0.0)
        .or(primary_c);

    let semantic = [
        ("background", background),
        ("foreground", foreground),
        ("primary", primary_c),
        ("primary-foreground", primary_fg),
        ("muted", muted),
        ("muted-foreground", muted_fg),
        ("border", border),
        ("ring", ring),
    ]
    .into_iter()
    .filter_map(|(n, c)| Some((n.to_string(), c?)))
    .collect();

    let dark = match (background, foreground) {
        (Some(bg), _) => bg.to_oklch().0 < 0.5,
        (None, Some(fg)) => fg.to_oklch().0 > 0.6,
        _ => false,
    };

    Colors {
        palette,
        semantic,
        primary_buttons: primary.map_or(0, |p| p.button_bg),
        primary_links: primary.map_or(0, |p| p.link_text),
        dark,
        seen,
    }
}
