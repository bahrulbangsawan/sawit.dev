//! Derives a design system from a [`Scan`]. Everything here traces to something the page showed:
//! a role, token or semantic color with nothing behind it is left out, never defaulted.

use serde::Serialize;

use super::color::{self, Rgba, STEPS, ramp};
use super::scan::{ColorRole, ColorUse, ComponentSample, Scan, TextSample};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Ramp {
    pub role: String,
    pub base: Rgba,
    pub steps: [Rgba; 11],
    pub uses: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TypeToken {
    pub family: String,
    pub size: f64,
    pub weight: u16,
    pub line_height: Option<f64>,
    pub letter_spacing: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComponentToken {
    pub name: String,
    pub props: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Facts {
    pub primary_buttons: u32,
    pub primary_links: u32,
    pub smallest_text: f64,
    pub dark: bool,
    pub colors_seen: usize,
    pub body: Option<TypeToken>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Tokens {
    pub site: String,
    pub url: String,
    pub date: String,
    pub palette: Vec<Ramp>,
    pub semantic: Vec<(String, Rgba)>,
    pub families: Vec<(String, String)>,
    pub type_roles: Vec<(String, TypeToken)>,
    pub type_scale: Vec<(String, f64)>,
    pub spacing_base: Option<f64>,
    pub spacing: Vec<(String, f64)>,
    pub radius: Vec<(String, String)>,
    pub shadows: Vec<(String, String)>,
    pub breakpoints: Vec<u32>,
    pub variables: Vec<(String, String)>,
    pub components: Vec<ComponentToken>,
    pub facts: Facts,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub color: Rgba,
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    pub spread: f64,
    pub inset: bool,
}

const CHROMA: f64 = 0.03;
const CLUSTER: f64 = 0.02;
const HUE_GAP: f64 = 30.0;
const MAX_VARIABLES: usize = 200;
const BODY_TAGS: [&str; 5] = ["p", "li", "dd", "td", "span"];
const HEADINGS: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];
const SPACING_NAMES: [&str; 7] = ["xs", "sm", "md", "lg", "xl", "2xl", "3xl"];
const RADIUS_NAMES: [&str; 4] = ["sm", "md", "lg", "xl"];
const SHADOW_NAMES: [&str; 3] = ["sm", "md", "lg"];
const GENERICS: [&str; 13] = [
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
];

/// `https://www.apple.com/x` → `Apple`; `localhost:5173` → `Localhost`.
pub fn site_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.trim_start_matches('[').split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    let host = host.strip_prefix("www.").unwrap_or(host);
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    let is_ip = labels.len() == 4 && labels.iter().all(|l| l.parse::<u8>().is_ok());
    let n = labels.len();
    let name = if is_ip || n < 2 {
        host
    } else if n >= 3
        && labels[n - 1].len() == 2
        && ["co", "com", "org", "net", "gov", "ac", "edu"].contains(&labels[n - 2])
    {
        labels[n - 3]
    } else {
        labels[n - 2]
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The first shadow of a `box-shadow` list.
pub fn parse_box_shadow(s: &str) -> Option<Shadow> {
    let first = split_top(s, |c| c == ',').into_iter().next()?;
    let mut lens = Vec::new();
    let mut color = None;
    let mut inset = false;
    for tok in split_top(first, char::is_whitespace) {
        if tok.eq_ignore_ascii_case("inset") {
            inset = true;
        } else if let Some(v) = px(tok) {
            lens.push(v);
        } else if color.is_none() {
            color = Some(color::parse(tok)?);
        } else {
            return None;
        }
    }
    if !(2..=4).contains(&lens.len()) {
        return None;
    }
    Some(Shadow {
        color: color?,
        x: lens[0],
        y: lens[1],
        blur: lens.get(2).copied().unwrap_or(0.0),
        spread: lens.get(3).copied().unwrap_or(0.0),
        inset,
    })
}

pub fn derive(scan: &Scan, date: &str) -> Tokens {
    let colors = derive_colors(scan);
    let text = derive_type(scan);
    let (spacing_base, spacing) = derive_spacing(scan);
    let radius = derive_radius(scan);
    let mut variables = scan.variables.clone();
    variables.sort_by(|a, b| a.0.cmp(&b.0));
    variables.truncate(MAX_VARIABLES);
    let mut breakpoints: Vec<u32> = scan
        .breakpoints
        .iter()
        .copied()
        .filter(|b| *b > 0)
        .collect();
    breakpoints.sort_unstable();
    breakpoints.dedup();

    let mut t = Tokens {
        site: site_name(&scan.url),
        url: scan.url.clone(),
        date: date.to_string(),
        palette: colors.palette,
        semantic: colors.semantic,
        families: text.families,
        type_roles: text.roles,
        type_scale: text.scale,
        spacing_base,
        spacing,
        radius,
        shadows: derive_shadows(scan),
        breakpoints,
        variables,
        components: Vec::new(),
        facts: Facts {
            primary_buttons: colors.primary_buttons,
            primary_links: colors.primary_links,
            smallest_text: text.smallest,
            dark: colors.dark,
            colors_seen: colors.seen,
            body: text.body,
        },
    };
    t.components = derive_components(scan, &t);
    t
}

// ---------- colors ----------

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

struct Colors {
    palette: Vec<Ramp>,
    semantic: Vec<(String, Rgba)>,
    primary_buttons: u32,
    primary_links: u32,
    dark: bool,
    seen: usize,
}

fn derive_colors(scan: &Scan) -> Colors {
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

// ---------- type ----------

struct Type {
    families: Vec<(String, String)>,
    roles: Vec<(String, TypeToken)>,
    scale: Vec<(String, f64)>,
    smallest: f64,
    body: Option<TypeToken>,
}

/// Size key for "distinct sizes": hundredths of a px.
fn size_key(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

fn heaviest<'a>(it: impl Iterator<Item = &'a TextSample>) -> Option<&'a TextSample> {
    it.fold(None, |best: Option<&TextSample>, s| match best {
        Some(b) if b.chars >= s.chars => Some(b),
        _ => Some(s),
    })
}

/// Stack entries, unquoted, with whether each was quoted.
fn stack(family: &str) -> Vec<(String, bool)> {
    split_top(family, |c| c == ',')
        .into_iter()
        .map(|e| {
            let quoted = e.starts_with('"') || e.starts_with('\'');
            (e.trim_matches(['"', '\'']).trim().to_string(), quoted)
        })
        .filter(|(e, _)| !e.is_empty())
        .collect()
}

fn last_generic(family: &str) -> Option<String> {
    stack(family)
        .into_iter()
        .rev()
        .find(|(e, quoted)| !quoted && GENERICS.contains(&e.to_ascii_lowercase().as_str()))
        .map(|(e, _)| e.to_ascii_lowercase())
}

fn type_token(s: &TextSample) -> TypeToken {
    let family = s
        .used_family
        .clone()
        .filter(|f| !f.trim().is_empty())
        .or_else(|| stack(&s.family).into_iter().next().map(|(e, _)| e))
        .unwrap_or_default();
    let ratio = |v: f64| (s.size > 0.0).then(|| v / s.size);
    TypeToken {
        family,
        size: s.size,
        weight: s.weight,
        line_height: s
            .line_height
            .filter(|v| *v > 0.0)
            .and_then(ratio)
            .map(|r| (r * 100.0).round() / 100.0),
        letter_spacing: s
            .letter_spacing
            .and_then(ratio)
            .map(|r| (r * 1000.0).round() / 1000.0)
            .filter(|r| *r != 0.0)
            .map(|r| format!("{}em", num(r, 3))),
    }
}

fn derive_type(scan: &Scan) -> Type {
    let texts: Vec<&TextSample> = scan.texts.iter().filter(|s| s.size > 0.0).collect();
    let tagged = |tags: &[&str]| -> Option<&TextSample> {
        heaviest(
            texts
                .iter()
                .copied()
                .filter(|s| tags.contains(&s.tag.as_str())),
        )
    };
    let body_main = heaviest(
        texts
            .iter()
            .copied()
            .filter(|s| s.in_main && BODY_TAGS.contains(&s.tag.as_str())),
    );
    let body = body_main.or_else(|| tagged(&BODY_TAGS));

    let mut picks: Vec<(&str, &TextSample)> = Vec::new();
    for (role, tags) in [
        ("display", &["h1"][..]),
        ("headline-lg", &["h2"][..]),
        ("headline-md", &["h3"][..]),
        ("title", &["h4"][..]),
    ] {
        if let Some(s) = tagged(tags) {
            picks.push((role, s));
        }
    }
    if let Some(b) = body {
        let body_size = size_key(b.size);
        let plain = || {
            texts
                .iter()
                .copied()
                .filter(|s| !HEADINGS.contains(&s.tag.as_str()))
        };
        let above = plain()
            .map(|s| size_key(s.size))
            .filter(|k| *k > body_size)
            .min();
        let below = plain()
            .map(|s| size_key(s.size))
            .filter(|k| *k < body_size)
            .max();
        let at =
            |k: Option<i64>| k.and_then(|k| heaviest(plain().filter(|s| size_key(s.size) == k)));
        if let Some(s) = at(above) {
            picks.push(("body-lg", s));
        }
        picks.push(("body-md", b));
        if let Some(s) = at(below) {
            picks.push(("body-sm", s));
        }
    }
    for (role, tags) in [
        ("label", &["button", "label"][..]),
        ("caption", &["small", "figcaption"][..]),
    ] {
        if let Some(s) = tagged(tags) {
            picks.push((role, s));
        }
    }
    let roles: Vec<(String, TypeToken)> = picks
        .iter()
        .map(|(r, s)| (r.to_string(), type_token(s)))
        .collect();
    let body_token = body.map(type_token);

    let mut sizes: Vec<f64> = Vec::new();
    for s in &texts {
        if !sizes.iter().any(|v| size_key(*v) == size_key(s.size)) {
            sizes.push(s.size);
        }
    }
    sizes.sort_by(|a, b| a.total_cmp(b));
    let mut scale = Vec::new();
    if let Some(i) = body.and_then(|b| sizes.iter().position(|v| size_key(*v) == size_key(b.size)))
    {
        let below: Vec<(&str, f64)> = ["sm", "xs"]
            .into_iter()
            .zip(sizes[..i].iter().rev().copied())
            .collect();
        scale.extend(below.into_iter().rev().map(|(n, v)| (n.to_string(), v)));
        scale.push(("base".to_string(), sizes[i]));
        scale.extend(
            ["lg", "xl", "2xl", "3xl", "4xl"]
                .into_iter()
                .zip(sizes[i + 1..].iter().copied())
                .map(|(n, v)| (n.to_string(), v)),
        );
    }

    let mut families = Vec::new();
    if let (Some(b), Some(bt)) = (body, &body_token) {
        let role = match last_generic(&b.family).as_deref() {
            Some("serif" | "ui-serif") => "serif",
            Some("monospace" | "ui-monospace") => "mono",
            _ => "sans",
        };
        families.push((role.to_string(), b.family.clone()));
        if let Some(h1) = tagged(&["h1"]).filter(|h1| type_token(h1).family != bt.family) {
            families.push(("display".to_string(), h1.family.clone()));
        }
    }
    if !families.iter().any(|(r, _)| r == "mono") {
        let mut stacks: Vec<(&str, u64)> = Vec::new();
        for s in texts.iter().filter(|s| {
            s.family
                .trim()
                .trim_end_matches(['"', '\''])
                .to_ascii_lowercase()
                .ends_with("monospace")
        }) {
            match stacks.iter_mut().find(|(f, _)| *f == s.family) {
                Some((_, n)) => *n += u64::from(s.chars),
                None => stacks.push((&s.family, u64::from(s.chars))),
            }
        }
        let best = stacks.iter().fold(None::<(&str, u64)>, |b, x| match b {
            Some(b) if b.1 >= x.1 => Some(b),
            _ => Some(*x),
        });
        if let Some((f, _)) = best {
            families.push(("mono".to_string(), f.to_string()));
        }
    }

    Type {
        families,
        roles,
        scale,
        smallest: texts.iter().map(|s| s.size).reduce(f64::min).unwrap_or(0.0),
        body: body_token,
    }
}

// ---------- layout ----------

fn derive_spacing(scan: &Scan) -> (Option<f64>, Vec<(String, f64)>) {
    let mut vals: Vec<(f64, u32)> = Vec::new();
    for &(v, n) in &scan.spacing {
        if v <= 0.0 || n == 0 || !v.is_finite() {
            continue;
        }
        match vals.iter_mut().find(|(x, _)| *x == v) {
            Some((_, c)) => *c += n,
            None => vals.push((v, n)),
        }
    }
    let total: u64 = vals.iter().map(|(_, n)| u64::from(*n)).sum();
    if total == 0 {
        return (None, Vec::new());
    }
    let share = |m: f64| -> u64 {
        vals.iter()
            .filter(|(v, _)| (v / m).fract() == 0.0)
            .map(|(_, n)| u64::from(*n))
            .sum()
    };
    let base = if share(4.0) * 10 >= total * 6 {
        Some(4.0)
    } else if share(8.0) * 10 >= total * 6 {
        Some(8.0)
    } else {
        let mut small: Vec<&(f64, u32)> = vals.iter().filter(|(v, _)| *v <= 8.0).collect();
        small.sort_by_key(|a| std::cmp::Reverse(a.1));
        small.first().map(|(v, _)| *v)
    };
    vals.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut top: Vec<f64> = vals
        .iter()
        .take(SPACING_NAMES.len())
        .map(|(v, _)| *v)
        .collect();
    top.sort_by(|a, b| a.total_cmp(b));
    let scale = SPACING_NAMES
        .iter()
        .zip(top)
        .map(|(n, v)| (n.to_string(), v))
        .collect();
    (base, scale)
}

enum Radius {
    Px(f64),
    Full,
}

fn parse_radius(s: &str) -> Option<Radius> {
    let first = split_top(s, char::is_whitespace).into_iter().next()?;
    if let Some(p) = first.strip_suffix('%') {
        let p: f64 = p.trim().parse().ok()?;
        return (p >= 50.0).then_some(Radius::Full);
    }
    let v = px(first)?;
    if v >= 500.0 {
        Some(Radius::Full)
    } else {
        Some(Radius::Px(v))
    }
}

fn derive_radius(scan: &Scan) -> Vec<(String, String)> {
    let mut full = false;
    let mut vals: Vec<(f64, u32)> = Vec::new();
    for (s, n) in &scan.radii {
        match parse_radius(s) {
            Some(Radius::Full) => full = true,
            Some(Radius::Px(v)) if v > 0.0 => match vals.iter_mut().find(|(x, _)| *x == v) {
                Some((_, c)) => *c += n,
                None => vals.push((v, *n)),
            },
            _ => {}
        }
    }
    vals.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut top: Vec<f64> = vals
        .iter()
        .take(RADIUS_NAMES.len())
        .map(|(v, _)| *v)
        .collect();
    top.sort_by(|a, b| a.total_cmp(b));
    let mut out: Vec<(String, String)> = RADIUS_NAMES
        .iter()
        .zip(top)
        .map(|(n, v)| (n.to_string(), format!("{}px", num(v, 2))))
        .collect();
    if full {
        out.push(("full".to_string(), "9999px".to_string()));
    }
    out
}

fn derive_shadows(scan: &Scan) -> Vec<(String, String)> {
    let mut vals: Vec<(&str, u32)> = Vec::new();
    for (s, n) in &scan.shadows {
        let s = s.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("none") {
            continue;
        }
        match vals.iter_mut().find(|(x, _)| *x == s) {
            Some((_, c)) => *c += n,
            None => vals.push((s, *n)),
        }
    }
    vals.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut top: Vec<(&str, f64)> = vals
        .iter()
        .take(SHADOW_NAMES.len())
        .map(|(s, _)| (*s, parse_box_shadow(s).map_or(0.0, |x| x.blur)))
        .collect();
    top.sort_by(|a, b| a.1.total_cmp(&b.1));
    SHADOW_NAMES
        .iter()
        .zip(top)
        .map(|(n, (s, _))| (n.to_string(), s.to_string()))
        .collect()
}

// ---------- components ----------

fn color_ref(t: &Tokens, c: Rgba, text_on: Option<&str>) -> String {
    let hex = c.to_hex();
    let is_fg = |n: &str| n.ends_with("foreground");
    let mut names: Vec<&str> = Vec::new();
    match text_on {
        Some(bg) => {
            let paired = format!("{bg}-foreground");
            names.extend(
                t.semantic
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .filter(|n| *n == paired),
            );
            names.extend(
                t.semantic
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .filter(|n| is_fg(n)),
            );
            names.extend(t.semantic.iter().map(|(n, _)| n.as_str()));
        }
        None => {
            names.extend(
                t.semantic
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .filter(|n| !is_fg(n)),
            );
            names.extend(t.semantic.iter().map(|(n, _)| n.as_str()));
        }
    }
    for n in names {
        if t.semantic.iter().any(|(m, v)| m == n && v.to_hex() == hex) {
            return format!("{{colors.{n}}}");
        }
    }
    for r in &t.palette {
        for (step, v) in STEPS.iter().zip(r.steps.iter()) {
            if v.to_hex() == hex {
                return format!("{{colors.{}-{step}}}", r.role);
            }
        }
    }
    hex
}

fn radius_ref(t: &Tokens, s: &str) -> Option<String> {
    let token = |v: &str| {
        t.radius
            .iter()
            .find(|(_, x)| x == v)
            .map(|(n, _)| format!("{{rounded.{n}}}"))
    };
    match parse_radius(s)? {
        Radius::Full => Some(token("9999px").unwrap_or_else(|| "9999px".to_string())),
        Radius::Px(v) => {
            let v = format!("{}px", num(v, 2));
            Some(token(&v).unwrap_or(v))
        }
    }
}

fn typography_ref(t: &Tokens, c: &ComponentSample) -> Option<String> {
    let entries: Vec<String> = stack(&c.style.font_family)
        .into_iter()
        .map(|(e, _)| e.to_ascii_lowercase())
        .collect();
    let fits = |tok: &TypeToken| {
        entries.contains(&tok.family.to_ascii_lowercase())
            && size_key(tok.size) == size_key(c.style.font_size)
            && tok.weight == c.style.font_weight
    };
    let label = t.type_roles.iter().filter(|(n, _)| n == "label");
    label
        .chain(t.type_roles.iter())
        .find(|(_, tok)| fits(tok))
        .map(|(n, _)| format!("{{typography.{n}}}"))
}

fn derive_components(scan: &Scan, t: &Tokens) -> Vec<ComponentToken> {
    let mut out = Vec::new();
    for (kind, name) in [
        ("button", "button-primary"),
        ("button-secondary", "button-secondary"),
        ("input", "input"),
        ("card", "card"),
        ("link", "nav-link"),
    ] {
        let Some(c) = scan.components.iter().find(|c| c.kind == kind) else {
            continue;
        };
        let st = &c.style;
        let mut props: Vec<(String, String)> = Vec::new();
        let mut bg_name = None;
        if let Some(bg) = color::parse(&st.background).filter(|c| c.a > 0.0) {
            let r = color_ref(t, bg, None);
            bg_name = r
                .strip_prefix("{colors.")
                .and_then(|r| r.strip_suffix('}'))
                .map(str::to_string);
            props.push(("backgroundColor".into(), r));
        }
        if let Some(fg) = color::parse(&st.color).filter(|c| c.a > 0.0) {
            let on = bg_name.as_deref().unwrap_or("");
            props.push(("textColor".into(), color_ref(t, fg, Some(on))));
        }
        if let Some(r) = typography_ref(t, c) {
            props.push(("typography".into(), r));
        }
        if let Some(r) = radius_ref(t, &st.radius) {
            props.push(("rounded".into(), r));
        }
        let padding = st.padding.trim();
        if !padding.is_empty()
            && split_top(padding, char::is_whitespace)
                .iter()
                .any(|p| px(p) != Some(0.0))
        {
            props.push(("padding".into(), padding.to_string()));
        }
        if st.height > 0.0 {
            props.push(("height".into(), format!("{}px", num(st.height, 2))));
        }
        if !props.is_empty() {
            out.push(ComponentToken {
                name: name.to_string(),
                props,
            });
        }
    }
    out
}

// ---------- helpers ----------

/// Splits at `sep` outside parentheses, trimming and dropping empty pieces.
fn split_top(s: &str, sep: impl Fn(char) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if depth == 0 && sep(c) => {
                out.push(&s[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

/// `12px`, `0` → px.
fn px(tok: &str) -> Option<f64> {
    let v = if tok == "0" {
        0.0
    } else {
        tok.strip_suffix("px")?.parse::<f64>().ok()?
    };
    v.is_finite().then_some(v)
}

/// A number with at most `dp` decimals and no trailing zeros.
fn num(v: f64, dp: usize) -> String {
    let s = format!("{v:.dp$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        &s
    };
    if s == "-0" { "0".into() } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Tokens {
        let scan: Scan = serde_json::from_str(include_str!("fixtures/scan.json")).unwrap();
        derive(&scan, "2026-10-30")
    }

    fn mono() -> Tokens {
        let scan: Scan = serde_json::from_str(include_str!("fixtures/mono.json")).unwrap();
        derive(&scan, "2026-10-30")
    }

    fn semantic<'a>(t: &'a Tokens, name: &str) -> Option<&'a Rgba> {
        t.semantic.iter().find(|(n, _)| n == name).map(|(_, c)| c)
    }

    fn role<'a>(t: &'a Tokens, name: &str) -> &'a TypeToken {
        &t.type_roles
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1
    }

    fn named<T: Clone>(v: &[(String, T)]) -> Vec<(&str, T)> {
        v.iter().map(|(n, x)| (n.as_str(), x.clone())).collect()
    }

    #[test]
    fn roles_from_the_fixture() {
        let t = fixture();
        let roles: Vec<(&str, String)> = t
            .palette
            .iter()
            .map(|r| (r.role.as_str(), r.base.to_hex()))
            .collect();
        assert_eq!(
            roles,
            vec![
                ("primary", "#0071e3".to_string()),
                ("secondary", "#bf4800".to_string()),
                ("accent", "#29a354".to_string()),
                ("neutral", "#1d1d1f".to_string()),
            ]
        );
        let neutral = &t.palette[3];
        assert_eq!(neutral.steps[10].to_hex(), "#1d1d1f");
        assert_eq!(neutral.uses, 300);
        assert_eq!(t.palette[0].uses, 14);
        assert_eq!(t.palette[0].steps[6].to_hex(), "#0071e3");
        assert_eq!(t.facts.primary_buttons, 12);
        assert_eq!(t.facts.primary_links, 0);
        assert_eq!(t.facts.colors_seen, 9);
        assert!(!t.facts.dark);
    }

    #[test]
    fn semantic_from_the_fixture() {
        let t = fixture();
        let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
        assert_eq!(hex("background").as_deref(), Some("#ffffff"));
        assert_eq!(hex("foreground").as_deref(), Some("#1d1d1f"));
        assert_eq!(hex("muted").as_deref(), Some("#f5f5f7"));
        assert_eq!(hex("muted-foreground").as_deref(), Some("#6e6e73"));
        assert_eq!(hex("border").as_deref(), Some("#d2d2d7"));
        assert_eq!(hex("primary").as_deref(), Some("#0071e3"));
        assert_eq!(hex("primary-foreground").as_deref(), Some("#ffffff"));
        assert_eq!(hex("ring").as_deref(), Some("#0071e3"));
    }

    #[test]
    fn type_from_the_fixture() {
        let t = fixture();
        let body = role(&t, "body-md");
        assert_eq!((body.size, body.weight), (17.0, 400));
        assert_eq!(body.line_height, Some(1.47));
        assert_eq!(body.family, "SF Pro Text");
        assert_eq!(body.letter_spacing.as_deref(), Some("-0.022em"));
        assert_eq!(t.facts.body.as_ref(), Some(body));
        assert_eq!(t.facts.smallest_text, 12.0);

        let lg = role(&t, "body-lg");
        assert_eq!((lg.size, lg.weight), (21.0, 400));
        let sm = role(&t, "body-sm");
        assert_eq!(sm.size, 14.0);
        let display = role(&t, "display");
        assert_eq!((display.size, display.weight), (48.0, 600));
        assert_eq!(display.family, "SF Pro Display");
        assert_eq!(role(&t, "label").line_height, Some(1.18));
        assert_eq!(role(&t, "caption").size, 12.0);
        assert_eq!(role(&t, "title").size, 21.0);

        assert_eq!(
            named(&t.type_scale),
            vec![
                ("xs", 12.),
                ("sm", 14.),
                ("base", 17.),
                ("lg", 21.),
                ("xl", 28.),
                ("2xl", 40.),
                ("3xl", 48.)
            ]
        );

        let fam: Vec<&str> = t.families.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(fam, vec!["sans", "display"]);
        assert!(t.families[0].1.starts_with("\"SF Pro Text\""));
    }

    #[test]
    fn layout_from_the_fixture() {
        let t = fixture();
        assert_eq!(t.spacing_base, Some(4.));
        assert_eq!(
            named(&t.spacing),
            vec![
                ("xs", 4.),
                ("sm", 8.),
                ("md", 12.),
                ("lg", 16.),
                ("xl", 20.),
                ("2xl", 24.),
                ("3xl", 32.)
            ]
        );
        assert_eq!(
            named(&t.radius),
            vec![
                ("sm", "4px".to_string()),
                ("md", "8px".to_string()),
                ("lg", "12px".to_string()),
                ("xl", "18px".to_string()),
                ("full", "9999px".to_string()),
            ]
        );
        let sh: Vec<&str> = t.shadows.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(sh, vec!["sm", "md", "lg"]);
        assert!(t.shadows[0].1.contains("1px 2px"));
        assert!(t.shadows[1].1.contains("2px 8px"));
        assert!(t.shadows[2].1.contains("8px 30px"));
        assert_eq!(t.breakpoints, vec![734, 1068]);
        assert_eq!(t.variables[0].0, "--sk-body-background-color");
    }

    #[test]
    fn components_use_refs() {
        let t = fixture();
        let b = t
            .components
            .iter()
            .find(|c| c.name == "button-primary")
            .unwrap();
        let get = |k: &str| {
            b.props
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("backgroundColor"), Some("{colors.primary}"));
        assert_eq!(get("textColor"), Some("{colors.primary-foreground}"));
        assert_eq!(get("rounded"), Some("{rounded.full}"));
        assert_eq!(get("padding"), Some("8px 16px"));
        assert_eq!(get("typography"), Some("{typography.label}"));
        assert_eq!(get("height"), Some("36px"));
        assert_eq!(t.components.len(), 1);
    }

    #[test]
    fn monochrome_has_no_roles() {
        let t = mono();
        let roles: Vec<&str> = t.palette.iter().map(|r| r.role.as_str()).collect();
        assert_eq!(roles, vec!["neutral"]);
        assert!(semantic(&t, "primary").is_none());
        assert!(semantic(&t, "primary-foreground").is_none());
        assert!(semantic(&t, "ring").is_none());
        assert!(t.spacing_base.is_none());
        assert!(t.radius.is_empty() && t.shadows.is_empty() && t.spacing.is_empty());
        assert_eq!(t.site, "Mono");
    }

    #[test]
    fn primary_falls_back_to_links_then_contrast() {
        use crate::site_design::scan::ColorUse;
        let use_ = |value: &str, role, count, on_link| ColorUse {
            value: value.into(),
            role,
            count,
            on_button: 0,
            on_link,
        };
        let scan = Scan {
            url: "https://x.dev".into(),
            colors: vec![
                use_("rgb(17, 17, 17)", ColorRole::Text, 100, 0),
                use_("rgb(250, 204, 21)", ColorRole::Background, 20, 0),
                use_("rgb(0, 102, 204)", ColorRole::Text, 10, 10),
                use_("transparent", ColorRole::Background, 500, 0),
            ],
            ..Default::default()
        };
        let t = derive(&scan, "2026-10-30");
        assert_eq!(t.palette[0].role, "primary");
        assert_eq!(t.palette[0].base.to_hex(), "#0066cc");
        assert_eq!(t.palette[1].base.to_hex(), "#facc15");
        assert_eq!(t.facts.primary_links, 10);
        assert_eq!(t.facts.colors_seen, 3);
        let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
        assert_eq!(hex("primary-foreground").as_deref(), Some("#ffffff"));
        assert_eq!(hex("ring").as_deref(), Some("#0066cc"));
        assert!(hex("background").is_none() && hex("border").is_none());
        assert!(t.components.is_empty() && t.type_roles.is_empty() && t.type_scale.is_empty());
    }

    fn use_(value: &str, role: ColorRole, count: u32) -> crate::site_design::scan::ColorUse {
        crate::site_design::scan::ColorUse {
            value: value.into(),
            role,
            count,
            on_button: 0,
            on_link: 0,
        }
    }

    #[test]
    fn translucent_colors_are_not_roles() {
        let scan = Scan {
            colors: vec![
                use_("rgba(0, 0, 0, 0.1)", ColorRole::Shadow, 900),
                use_("rgba(0, 113, 227, 0.1)", ColorRole::Background, 400),
                use_("rgb(51, 51, 51)", ColorRole::Text, 100),
                use_("rgb(0, 0, 0)", ColorRole::Text, 50),
                use_("rgba(0, 0, 0, 0.12)", ColorRole::Border, 30),
            ],
            ..Default::default()
        };
        let t = derive(&scan, "2026-10-30");
        let roles: Vec<(&str, String)> = t
            .palette
            .iter()
            .map(|r| (r.role.as_str(), r.base.to_hex()))
            .collect();
        assert_eq!(roles, vec![("neutral", "#333333".to_string())]);
        assert_eq!(t.palette[0].uses, 100);
        // An observed translucent border stays translucent.
        let border = semantic(&t, "border").unwrap();
        assert_eq!(border.to_hex(), "#0000001f");
        // Equal hue and lightness but different alpha don't cluster.
        let scan = Scan {
            colors: vec![
                use_("rgb(0, 0, 0)", ColorRole::Text, 10),
                use_("rgba(0, 0, 0, 0.5)", ColorRole::Text, 50),
            ],
            ..Default::default()
        };
        let t = derive(&scan, "2026-10-30");
        assert_eq!(t.palette[0].uses, 10);
    }

    #[test]
    fn muted_skips_the_body_colors() {
        let scan = Scan {
            body_background: Some("rgb(243, 244, 246)".into()),
            colors: vec![
                use_("rgb(255, 255, 255)", ColorRole::Background, 300),
                use_("rgb(243, 244, 246)", ColorRole::Background, 100),
                use_("rgb(30, 41, 59)", ColorRole::Text, 200),
                use_("rgb(107, 114, 128)", ColorRole::Text, 50),
                use_("rgb(156, 163, 175)", ColorRole::Text, 20),
            ],
            ..Default::default()
        };
        let t = derive(&scan, "2026-10-30");
        let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
        assert_eq!(hex("background").as_deref(), Some("#f3f4f6"));
        assert_eq!(hex("muted").as_deref(), Some("#ffffff"));
        assert_eq!(hex("foreground").as_deref(), Some("#1e293b"));
        assert_eq!(hex("muted-foreground").as_deref(), Some("#6b7280"));
    }

    #[test]
    fn foreground_is_weighted_by_text_length() {
        let text = |color: &str, tag: &str, chars: u32| TextSample {
            tag: tag.into(),
            in_main: tag != "li",
            family: "Inter".into(),
            used_family: Some("Inter".into()),
            size: 16.0,
            weight: 400,
            line_height: None,
            letter_spacing: None,
            color: color.into(),
            chars,
        };
        let scan = Scan {
            body_background: Some("rgb(255, 255, 255)".into()),
            // Elements: 9 links, 3 paragraphs, 1 muted note.
            colors: vec![
                use_("rgb(0, 102, 204)", ColorRole::Text, 9),
                use_("rgb(29, 29, 31)", ColorRole::Text, 3),
                use_("rgb(110, 110, 115)", ColorRole::Text, 1),
            ],
            // Text: the paragraphs carry most of it.
            texts: vec![
                text("rgb(0, 102, 204)", "a", 60),
                text("rgb(29, 29, 31)", "p", 900),
                text("rgb(110, 110, 115)", "p", 40),
                // Footer fine print outside main: longest, but not the foreground.
                text("rgba(0, 0, 0, 0.56)", "li", 3000),
            ],
            ..Default::default()
        };
        let t = derive(&scan, "2026-10-31");
        let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
        assert_eq!(hex("foreground").as_deref(), Some("#1d1d1f"));
        assert_eq!(hex("muted-foreground").as_deref(), Some("#6e6e73"));

        // With nothing in main, every sample counts.
        let mut outside = scan.clone();
        outside.texts.iter_mut().for_each(|t| t.in_main = false);
        let t = derive(&outside, "2026-10-31");
        let fg = semantic(&t, "foreground").map(|c| c.to_hex());
        assert_eq!(fg.as_deref(), Some("#0000008f"));
    }

    #[test]
    fn foreground_is_readable_on_the_background() {
        let text = |color: &str, chars: u32| TextSample {
            tag: "p".into(),
            in_main: true,
            family: "Inter".into(),
            used_family: Some("Inter".into()),
            size: 17.0,
            weight: 400,
            line_height: None,
            letter_spacing: None,
            color: color.into(),
            chars,
        };
        let mut scan = Scan {
            body_background: Some("rgb(255, 255, 255)".into()),
            // Light text on dark hero tiles has the most characters.
            texts: vec![
                text("rgb(245, 245, 247)", 860),
                text("rgb(29, 29, 31)", 250),
            ],
            ..Default::default()
        };
        let fg = |s: &Scan| {
            let t = derive(s, "2026-10-31");
            semantic(&t, "foreground").map(|c| c.to_hex())
        };
        assert_eq!(fg(&scan).as_deref(), Some("#1d1d1f"));
        // Nothing readable on the background: the unfiltered pick.
        scan.texts.truncate(1);
        assert_eq!(fg(&scan).as_deref(), Some("#f5f5f7"));
        // Unknown background: no filter.
        scan.body_background = None;
        scan.texts.push(text("rgb(29, 29, 31)", 250));
        assert_eq!(fg(&scan).as_deref(), Some("#f5f5f7"));
    }

    #[test]
    fn muted_text_is_not_the_background() {
        let scan = Scan {
            body_background: Some("rgb(255, 255, 255)".into()),
            colors: vec![
                use_("rgb(29, 29, 31)", ColorRole::Text, 9),
                use_("rgb(255, 255, 255)", ColorRole::Text, 3),
                use_("rgb(245, 245, 247)", ColorRole::Text, 2),
                use_("rgb(110, 110, 115)", ColorRole::Text, 1),
                use_("rgb(245, 245, 247)", ColorRole::Background, 5),
            ],
            ..Default::default()
        };
        let t = derive(&scan, "2026-10-31");
        let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
        assert_eq!(hex("muted-foreground").as_deref(), Some("#6e6e73"));
    }

    #[test]
    fn site_names() {
        assert_eq!(site_name("https://www.apple.com/iphone/"), "Apple");
        assert_eq!(site_name("localhost:5173"), "Localhost");
        assert_eq!(site_name("http://localhost:5173/x"), "Localhost");
        assert_eq!(site_name("https://docs.github.com"), "Github");
        assert_eq!(site_name("https://www.bbc.co.uk/news"), "Bbc");
    }

    #[test]
    fn box_shadows() {
        let s = parse_box_shadow("rgba(0, 0, 0, 0.08) 0px 2px 8px 0px").unwrap();
        assert_eq!(
            (s.x, s.y, s.blur, s.spread, s.inset),
            (0., 2., 8., 0., false)
        );
        assert!((s.color.a - 0.08).abs() < 0.01);
        let s = parse_box_shadow("inset 0 0 0 1px #ccc, 0 1px 2px red").unwrap();
        assert!(s.inset);
        assert_eq!(s.spread, 1.);
        assert_eq!(s.color.to_hex(), "#cccccc");
        assert!(parse_box_shadow("none").is_none());
    }
}
