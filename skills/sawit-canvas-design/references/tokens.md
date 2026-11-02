# Starting values

Use these only where the project has no design system of its own (see "How to use it" in `SKILL.md`). The rule ids in brackets point to `SKILL.md`; `sources-map.md` traces each to its source lines. Engine paths are in the FigCraft checkout Sawit pins (`https://github.com/storytold/figcraft`, rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`).

## Frame presets [cnv.4]

Every preset `frame.create {preset}` accepts, with its exact size (`crates/engine/src/cmd/create.rs:15-51`; names match without regard to case, `create.rs:368-374`). A preset with no `x`/`y` lands 100 to the right of the page's right-most top-level layer, top-aligned (`create.rs:380-411`).

| Category | Preset | Size | Use it for |
| --- | --- | --- | --- |
| Desktop | `Desktop` | 1440×1024 | the default desktop screen |
| Desktop | `MacBook Air` | 1280×832 | the narrow desktop check |
| Desktop | `MacBook Pro 14"` | 1512×982 | a wide laptop check |
| Desktop | `MacBook Pro 16"` | 1728×1117 | |
| Desktop | `Wireframe` | 1440×1024 | |
| Desktop | `TV` | 1280×720 | |
| Tablet | `iPad mini 8.3` | 744×1133 | the default tablet (portrait; `frame.swapOrientation` for landscape) |
| Tablet | `iPad Pro 11"` | 834×1194 | |
| Tablet | `iPad Pro 12.9"` | 1024×1366 | |
| Tablet | `Surface Pro 8` | 1440×960 | |
| Tablet | `Android Expanded` | 1280×800 | |
| Phone | `iPhone 16` | 393×852 | the default mobile screen |
| Phone | `iPhone 16 Pro` | 402×874 | |
| Phone | `iPhone 16 Pro Max` | 440×956 | |
| Phone | `iPhone 16 Plus` | 430×932 | |
| Phone | `iPhone SE` | 320×568 | the narrowest check (WCAG reflow width is 320) |
| Phone | `Android Compact` | 412×917 | the Android check |
| Phone | `Android Medium` | 700×840 | |
| Presentation | `Slide 16:9` | 1920×1080 | slides |
| Presentation | `Slide 4:3` | 1024×768 | |
| Watch | `Apple Watch Series 10 42mm` | 187×223 | |
| Watch | `Apple Watch 41mm` | 176×215 | |
| Watch | `Apple Watch 45mm` | 198×242 | |
| Paper | `A4` | 595×842 | |
| Paper | `A5` | 420×595 | |
| Paper | `A6` | 297×420 | |
| Paper | `Letter` | 612×792 | |
| Paper | `Tabloid` | 792×1224 | |
| Social media | `Instagram post` | 1080×1350 | |
| Social media | `Instagram story` | 1080×1920 | |
| Social media | `Facebook post` | 1200×630 | |
| Social media | `X post` | 1200×675 | |
| Social media | `LinkedIn post` | 1200×1200 | |
| Social media | `YouTube thumbnail` | 1280×720 | |
| Social media | `Dribbble shot` | 1600×1200 | |

A macOS app has no preset: draw its minimum window and one wide window with `frame.create {width, height, name}`. For Sawit itself that is 720×480 (with the sidebar) and 1470 wide, from the user's own rule for Sawit pages.

## Type scale [typ.4, typ.5, typ.7, typ.8]

| Role | Size / line height | Weight | Notes | Source |
| --- | --- | --- | --- | --- |
| Meta, counts | 12/16 | 400 | mono welcome | U tokens:86-95 |
| Caption | 13/1.4 | 400 | U gives 13–14/20 | IS T-21; U |
| Body, dense app UI | 14/20 | 400–500 | U gives 14–15/20–22 | U |
| Body | 16/1.5 | 400 | long-form starts here | IS T-21, T-61 |
| Hero subhead | 18–20/28–30 | 400 | secondary color, at most 576 wide (36rem) | U |
| Logo wordmark | 16–18/1 | 500–600 | | U |
| Heading | 18/1.3 | 600 | | IS T-21 |
| Title | 24/1.2 | 600 | letter spacing −1% to −2% from here | IS T-21, T-27 |
| App page title | 28–36/1.15 | 400–500 | | U |
| Stat figure | 28–32/1 | 500 | tabular figures | U |
| Display | 36/1.1 | 600 | | IS T-21 |
| Hero headline | 56–72 desktop, 36–44 mobile / 1.02–1.08 | 400–600 | one or two lines; letter spacing down to −3% from 56px | U; IS |

On the canvas: `lineHeight` takes px or a percent (`"150%"`), `letterSpacing` px or a percent of the size (`"-2%"`), and `features: {"tnum": true}` turns on tabular figures (the `node.set` keys in `crates/engine/src/cmd/props.rs:19-37`).

## Spacing [spc.1–spc.10]

| What | Value |
| --- | --- |
| Scale | 4, 8, 12, 16, 20, 24, 32, 40, 48, 64, 80, 96, 128 (2 only inside small controls) |
| Inside a control | 4–12 |
| Related items (list rows) | 2–8 |
| Between groups | 12–24, and at least 2× the gap inside |
| Between sections | 32–64; landing 80–128; mobile half or less |
| Heading to its content | 8 below, 24 above |
| Padding | compact panel 16, card 20–24, frame edge 24–32, mobile edge 16 (never under 12) |
| Text inside a bordered or filled box | at least 8, ideally 12–16 |
| Rows (desktop) | 32 compact, 36 default; nav rows 32–36 with 2–4 between |
| Control clearance | 12 bordered, 24 borderless (glyph to glyph), 24+ between groups |
| Scroll peek | 16–32 past the edge |
| Landing hero offset | 96–128 below the nav on desktop, 48–64 on mobile |
| Mobile full-width button | 16 in from each side and above the bottom safe area |

## Radius [pol.1, pol.2]

| Element | Radius |
| --- | --- |
| Keycap, tag, small button | 4–6 |
| Row, input, button | 8–10 |
| Card | 12–16 |
| Panel, window | 14–20 |
| Nested | outer = inner + padding (+ border width); inner at least 4; past 24 of padding, independent radii |

## Shadows and rings [pol.3, pol.4, sta.6]

| Use | Value |
| --- | --- |
| Light, small | `0 1px 2px rgba(0,0,0,0.05)` |
| Light, floating | `0 8px 24px rgba(0,0,0,0.06)` |
| Light ring replacing a depth border | `0 0 0 1px` black 6%, `0 1px 2px -1px` black 6%, `0 2px 4px 0` black 4%; on hover 8%, 8%, 6% |
| Dark ring | `0 0 0 1px` white 8%; on hover 13% |
| Dark active row or button highlight | `inset 0 1px 0 rgba(255,255,255,0.12)` |
| Hairline | primary text color at 6–10% |

On the canvas an effect is a JSON object in `node.set {ids, props: {effects: [...]}}` or `effect.set`: `{"type": "dropShadow", "color": "rgba(0,0,0,0.05)", "offset": [0, 1], "radius": 2, "spread": 0}`. The field names come from the engine's `Shadow` (`crates/doc/src/props.rs:502-517`), colors accept CSS text (`crates/color/src/lib.rs:52-60`), and unknown effect keys are refused (`crates/engine/src/cmd/props.rs:70-77`). This shape was read from the source, not run. The engine's default drop shadow is black 25%, offset (0, 4), blur 4 (`crates/doc/src/props.rs:514-517`): never keep it.

## Neutral ramps with measured contrast [col.1, col.8, col.10, col.19]

unslop's graphite ramps (`U tokens:30-51`) are good app starting points. Contrast below was computed with the WCAG 2 formula (`scripts/contrast.py` in this skill), not taken from the source: muted text fails 4.5:1 on the raised surface, so it never sits there.

Dark graphite:

| Role | Hex | Secondary text on it | Muted text on it |
| --- | --- | --- | --- |
| Deepest (sidebar) | `#0f0f10` | 7.45 | 5.22 |
| Page | `#141415` | 7.16 | 5.02 |
| Card | `#19191b` | 6.83 | 4.79 |
| Raised (active row, input) | `#232325` | 6.10 | 4.28, fails |
| Text | primary `#ededed`, secondary `#a1a1a6`, muted `#85858b` | | |

Light graphite:

| Role | Hex | Secondary text on it | Muted text on it |
| --- | --- | --- | --- |
| Page | `#fafafa` | 7.49 | 4.54 |
| Card | `#ffffff` | 7.81 | 4.74 |
| Raised | `#f4f4f5` | 7.11 | 4.31, fails |
| Text | primary `#171717`, secondary `#525252`, muted `#737373` | | |

unslop's warm and ink ramps (`U tokens:30-51`) fail more: muted text also fails on the light warm page (`#7b7266` on `#f7f4ee`, 4.31) and the light ink page (`#6b7487` on `#f5f7fa`, 4.37).

Status colors from the same table, measured:

| Color | On | Ratio | Use |
| --- | --- | --- | --- |
| Positive dark `#4cc38a` | page `#141415` | 8.31 | text and marks |
| Negative dark `#e5484d` | page `#141415` / card `#19191b` / raised `#232325` | 4.70 / 4.49 / 4.01 | text on the page only; marks and large text elsewhere |
| Positive light `#16a34a` | white | 3.30 | icons, marks and large text only; small text needs a darker green (`#15803d` measures 5.02 on white, 4.80 on `#fafafa`, 4.56 on `#f4f4f5`) |
| Negative light `#dc2626` | white / `#fafafa` / `#f4f4f5` | 4.83 / 4.63 / 4.39 | text on page and card, not on raised |
| White text | on `#2563eb` / on `#3b82f6` | 5.17 / 3.68 | why solid fills sit on the 600 step [col.15] |

The formula, for a script: each sRGB channel `c` (0–1) becomes `c/12.92` when `c <= 0.03928`, else `((c+0.055)/1.055)^2.4`; luminance `L = 0.2126 R + 0.7152 G + 0.0722 B`; ratio `(L1 + 0.05) / (L2 + 0.05)` with `L1` the lighter (https://www.w3.org/TR/WCAG22/#dfn-relative-luminance, https://www.w3.org/TR/WCAG22/#dfn-contrast-ratio). Compare the unrounded ratio.

## Motion [mot.4–mot.9, mot.11]

| What | Value |
| --- | --- |
| Curve | the project's ease-out, else `cubic-bezier(0.22, 1, 0.36, 1)`; springs at bounce 0 |
| Hover, toggle | 120–180ms on color or fill; high-frequency actions instant or at most 150ms |
| Press | scale exactly 0.96 over 150ms; disabled never scales |
| Menu, popover, tooltip | 160–220ms |
| Enter | 300ms (sheets and layout changes up to 500): opacity 0 to 1, rise 6–12, optional blur 4px to 0 |
| Exit | 150ms: opacity to 0 with a 12px rise; remove instantly where motion adds nothing |
| Stagger | semantic chunks 100ms apart; up to six list items 40–60ms apart; display headline words 80ms apart |
| Icon swap | scale 0.25 to 1, opacity 0 to 1, blur 4px to 0, 300ms |
| Reduced motion | movement, scale and blur become an opacity cross-fade; spinners and progress stay |
| Limits | pause control for anything moving over 5s; no more than 3 flashes a second |

In a prototype, `proto.interaction.add` takes `durationMs` and an `easing` that can be `{customBezier: {x1, y1, x2, y2}}` (`crates/engine/src/cmd/proto.rs:16`); prototypes run only in design files (`crates/engine/src/boards/board.rs:209-217`).

## Icons [cmp.2–cmp.4]

| Adjacent text | Icon size | Stroke (rendered px) |
| --- | --- | --- |
| 14–16px, weight 400 | 16 | 1.5 |
| 14–16px, weight 500–600 | 16 | 2 |
| 18–20px | 20 | by weight as above |
| bold (700) or an emphasized standalone icon | 1–1.25× the text | 2.5 |

A 24-grid icon drawn at 16 scales its stroke by 16/24 (a stroke of 2 paints about 1.33): draw at the render size or keep the stroke absolute. The icon box has one width per list with an 8–10 gap to its label.
