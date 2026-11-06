//! The status line's colours: colour names and #RRGGBB, the themes (Claude Pool's), and the
//! ANSI codes for 16, 256 and true colour.

use super::Config;

const ANSI_NAMES: &[&str] = &[
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightMagenta",
    "brightCyan",
    "brightWhite",
];
const ANSI_RGB: &[(u8, u8, u8)] = &[
    (0, 0, 0),
    (205, 49, 49),
    (13, 188, 121),
    (229, 229, 16),
    (36, 114, 200),
    (188, 63, 188),
    (17, 168, 205),
    (229, 229, 229),
    (118, 118, 118),
    (241, 76, 76),
    (35, 209, 139),
    (245, 245, 67),
    (59, 142, 234),
    (214, 112, 214),
    (41, 184, 219),
    (255, 255, 255),
];

/// A colour name's or `#RRGGBB`'s red, green and blue.
pub fn rgb(color: &str) -> Option<(u8, u8, u8)> {
    if let Some(i) = ANSI_NAMES.iter().position(|n| *n == color) {
        return Some(ANSI_RGB[i]);
    }
    let hex = color
        .strip_prefix('#')
        .or_else(|| color.strip_prefix("hex:"))?;
    let v = u32::from_str_radix(hex, 16)
        .ok()
        .filter(|_| hex.len() == 6)?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// A theme: a colour per role (and a background for the block themes), and the separators'.
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub(super) fg: [&'static str; 8],
    pub(super) bg: Option<[&'static str; 8]>,
    pub separator: &'static str,
}

/// The themes, Claude Pool's: model, identity, location, git, context, session, usage, text.
pub const THEMES: &[Theme] = &[
    Theme {
        id: "classic",
        name: "Classic",
        fg: [
            "cyan",
            "magenta",
            "blue",
            "green",
            "yellow",
            "white",
            "brightYellow",
            "brightBlack",
        ],
        bg: None,
        separator: "brightBlack",
    },
    Theme {
        id: "minimal",
        name: "Minimal",
        fg: [
            "white",
            "brightBlack",
            "brightBlack",
            "brightBlack",
            "brightBlack",
            "brightBlack",
            "white",
            "brightBlack",
        ],
        bg: None,
        separator: "brightBlack",
    },
    Theme {
        id: "ocean",
        name: "Ocean",
        fg: [
            "#E6F1FF", "#E6F1FF", "#0B1E2D", "#0B1E2D", "#0B1E2D", "#E6F1FF", "#0B1E2D", "#E6F1FF",
        ],
        bg: Some([
            "#1F4E79", "#2E6FA7", "#6CB4EE", "#7FD1B9", "#A9DEF9", "#3D5A80", "#98C1D9", "#293241",
        ]),
        separator: "#6CB4EE",
    },
    Theme {
        id: "sunset",
        name: "Sunset",
        fg: [
            "#FFF4E6", "#FFF4E6", "#2B1A12", "#2B1A12", "#2B1A12", "#FFF4E6", "#2B1A12", "#FFF4E6",
        ],
        bg: Some([
            "#9C2C3C", "#C8553D", "#F28F3B", "#FFD5C2", "#F6BD60", "#6D2E46", "#F7EDE2", "#3D2B1F",
        ]),
        separator: "#F28F3B",
    },
    Theme {
        id: "nord",
        name: "Nord",
        fg: [
            "#88C0D0", "#B48EAD", "#81A1C1", "#A3BE8C", "#EBCB8B", "#D8DEE9", "#D08770", "#4C566A",
        ],
        bg: None,
        separator: "#4C566A",
    },
    Theme {
        id: "mono",
        name: "Monochrome",
        fg: ["white"; 8],
        bg: Some([
            "#3A3A3A", "#4A4A4A", "#5A5A5A", "#4A4A4A", "#3A3A3A", "#4A4A4A", "#5A5A5A", "#2A2A2A",
        ]),
        separator: "brightBlack",
    },
];

pub fn theme(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.id == id)
}

pub(super) fn color_code(c: &Config, color: &str, bg: bool) -> Option<String> {
    let basic = |i: usize| {
        if i < 8 {
            (if bg { 40 } else { 30 }) + i
        } else {
            (if bg { 100 } else { 90 }) + i - 8
        }
    };
    if let Some(i) = ANSI_NAMES.iter().position(|n| *n == color) {
        return Some(basic(i).to_string());
    }
    let (r, g, b) = rgb(color)?;
    Some(match c.color_level.as_str() {
        "256" => format!("{};5;{}", if bg { 48 } else { 38 }, xterm256(r, g, b)),
        "16" => basic(nearest_basic(r, g, b)).to_string(),
        _ => format!("{};2;{r};{g};{b}", if bg { 48 } else { 38 }),
    })
}

fn xterm256(r: u8, g: u8, b: u8) -> u32 {
    let level = |v: u8| -> u32 {
        match v {
            ..48 => 0,
            48..115 => 1,
            _ => (u32::from(v) - 35) / 40,
        }
    };
    if r == g && g == b {
        return match r {
            ..8 => 16,
            249.. => 231,
            _ => 232 + (u32::from(r) - 8) * 24 / 247,
        };
    }
    16 + 36 * level(r) + 6 * level(g) + level(b)
}

fn nearest_basic(r: u8, g: u8, b: u8) -> usize {
    let d = |(cr, cg, cb): (u8, u8, u8)| {
        let (dr, dg, db) = (
            i32::from(r) - i32::from(cr),
            i32::from(g) - i32::from(cg),
            i32::from(b) - i32::from(cb),
        );
        dr * dr + dg * dg + db * db
    };
    (0..ANSI_RGB.len())
        .min_by_key(|&i| d(ANSI_RGB[i]))
        .unwrap_or(0)
}
