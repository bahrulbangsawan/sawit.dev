"""WCAG 2 contrast ratio for color pairs, compared unrounded (col.10 in SKILL.md).

Usage:
    python3 -I contrast.py FOREGROUND BACKGROUND [FOREGROUND BACKGROUND ...]

Colors are hex: #rgb, #rrggbb or #rrggbbaa. A foreground with alpha is composited
over its background first; the background must be opaque (measure the color that
is actually painted behind the text). Prints the ratio and whether it passes
4.5:1 (text), 3:1 (large text: 24px+, or 18.67px+ bold) and 3:1 (icons, borders,
focus rings, chart marks).

Formula: https://www.w3.org/TR/WCAG22/#dfn-relative-luminance and
https://www.w3.org/TR/WCAG22/#dfn-contrast-ratio
"""
import sys


def parse(color):
    h = color.strip().lstrip("#")
    if len(h) == 3:
        h = "".join(c * 2 for c in h)
    if len(h) not in (6, 8):
        raise ValueError(f"not a hex color: {color}")
    r, g, b = (int(h[i:i + 2], 16) / 255 for i in (0, 2, 4))
    a = int(h[6:8], 16) / 255 if len(h) == 8 else 1.0
    return r, g, b, a


def channel(c):
    return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4


def luminance(rgb):
    r, g, b = rgb
    return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)


def ratio(fg, bg):
    fr, fg_, fb, fa = parse(fg)
    br, bgc, bb, ba = parse(bg)
    if ba < 1.0:
        raise ValueError(f"background {bg} is translucent: measure the opaque color behind it")
    mixed = tuple(f * fa + b * (1 - fa) for f, b in ((fr, br), (fg_, bgc), (fb, bb)))
    l1, l2 = sorted((luminance(mixed), luminance((br, bgc, bb))), reverse=True)
    return (l1 + 0.05) / (l2 + 0.05)


def main(args):
    if len(args) < 2 or len(args) % 2:
        print(__doc__)
        return 2
    for fg, bg in zip(args[0::2], args[1::2]):
        r = ratio(fg, bg)
        verdict = lambda need: "pass" if r >= need else "FAIL"
        print(f"{fg} on {bg}: {r:.3f}  text {verdict(4.5)}  large {verdict(3.0)}  non-text {verdict(3.0)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
