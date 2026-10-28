//! Terminal colors: 16 theme colors plus the standard xterm 256-color cube.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn hex(v: u32) -> Rgb {
        Rgb {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub ansi: [Rgb; 16],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
}

impl Default for Palette {
    /// Zed "One Dark" terminal colors.
    fn default() -> Palette {
        Palette {
            ansi: [
                Rgb::hex(0x282c34),
                Rgb::hex(0xe06c75),
                Rgb::hex(0x98c379),
                Rgb::hex(0xe5c07b),
                Rgb::hex(0x61afef),
                Rgb::hex(0xc678dd),
                Rgb::hex(0x56b6c2),
                Rgb::hex(0xabb2bf),
                Rgb::hex(0x636d83),
                Rgb::hex(0xea858b),
                Rgb::hex(0xaad581),
                Rgb::hex(0xffd885),
                Rgb::hex(0x85c1ff),
                Rgb::hex(0xd398eb),
                Rgb::hex(0x6ed5de),
                Rgb::hex(0xfafafa),
            ],
            foreground: Rgb::hex(0xabb2bf),
            background: Rgb::hex(0x282c33),
            cursor: Rgb::hex(0x74ade8),
        }
    }
}

impl Palette {
    /// Color for an 8-bit index: theme colors, 6x6x6 cube, then grayscale ramp.
    pub fn indexed(&self, ix: u8) -> Rgb {
        match ix {
            0..=15 => self.ansi[ix as usize],
            16..=231 => {
                let i = ix - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                Rgb {
                    r: level(i / 36),
                    g: level((i / 6) % 6),
                    b: level(i % 6),
                }
            }
            _ => {
                let v = 8 + (ix - 232) * 10;
                Rgb { r: v, g: v, b: v }
            }
        }
    }
}
