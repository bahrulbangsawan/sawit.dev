# Browser Preview 3: Design Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Design panel (Inspect · Colors · Fonts · Assets · System) and a one-click export of
any site's design system to `.nyawit/design/<host>/`. The export is DESIGN.md, DTCG tokens, a
Tailwind v4 theme, shadcn variables, components and assets. It comes with
`browser_inspect` / `browser_design` / `browser_export_design` for agents.

**Architecture:**
- **Page:** a second spliced module, `browser_design.js`, observes and reports (`scan`, `rules`,
  `style`).
- **Core:** all derivation and every file format is pure Rust in `sawit_core::site_design`
  (`color`, `scan`, `tokens`, `inspect`, `tailwind`, `export`, `assets`), tested headless. Sawit
  fetches what the page can't read (cross-origin CSS, the manifest, assets) with
  `/usr/bin/curl`.
- **App:** orchestrates the flow and renders the panel. Exports run in the background with a
  progress status, because the control socket times out at 30 s.

**Tech Stack:** Rust, GPUI (gpui-kit 0.8.1), serde_json, futures (`buffer_unordered`), `/usr/bin/curl` via `sawit_core::cmd::run`, plain JS.

**Spec:** `docs/superpowers/specs/2026-10-29-browser-annotate-design-design.md` (sections "Design panel", "Export folder", "Derivation rules", D8–D11)

## Global Constraints

- Plans 1 and 2 are merged. They provide `page_op`, `shoot`, `flash`, `PickMode::Inspect`,
  `Hovered`, `panel::frame`, `date::local_date` and `annotations::Rect`.
- **Everything exported traces to something observed.** A role, token or shadcn variable that
  wasn't derived from the page is omitted, never filled with a default.
- **Constants:**

  | Constant | Value |
  |---|---|
  | `RAMP_L` | `[0.97, 0.93, 0.88, 0.80, 0.71, 0.62, 0.54, 0.46, 0.38, 0.30, 0.22]` |
  | `RAMP_C` | `[0.10, 0.22, 0.40, 0.62, 0.85, 1.00, 0.95, 0.85, 0.72, 0.58, 0.45]` |
  | `STEPS` | `[50,100,200,300,400,500,600,700,800,900,950]` |
  | Chromatic threshold | chroma ≥ 0.03 |
  | Clustering | ΔE_OK < 0.02 |
  | Hue gap for secondary/accent | ≥ 30° |

  Chroma is reduced in 0.005 steps until the color is in sRGB.
- **Limits:**
  - Scan: ≤ 6 000 elements, answer ≤ 2 MB.
  - Assets: curl `--max-time 30 --max-filesize 52428800`, 6 at a time, 300 MB total.
  - CSS and manifest fetches: 10 s, ≤ 5 MB.
  - Tool output ≤ 60 KB.
- **Export folder:** `.nyawit/design/<host>/` (replaced on re-export) or
  `~/Downloads/sawit-design/<host>/` without a project. Its layout follows the spec's "Export
  folder" block exactly.
- **DESIGN.md** follows Google Labs' alpha format:
  - Front-matter keys in order: `version`, `name`, `description`, `colors`, `typography`,
    `rounded`, `spacing`, `components`.
  - Sections in order: Overview, Colors, Typography, Layout, Elevation & Depth, Shapes,
    Components, Do's and Don'ts. Empty sections are left out.
- **License line** (README.md and the Fonts footer): "Assets and fonts belong to their owners;
  check licenses before shipping." The Fonts footer reads "Fonts belong to their foundries. Check
  the license before shipping."
- **`blob:` URLs** aren't saved: "Can't save blob: media (made by the page's scripts)."

## Review Focus

1. **A site whose CSS is all on a CDN** (apple.com). Fonts, variables, breakpoints and
   `:hover` states still appear, because the sheets are fetched and parsed in the page. Covered
   by the Task 7 test `merge_rules_adds_fetched_sheets` and the Task 11 apple.com step.
2. **Export while offline, or an asset host returning 403.** DESIGN.md, the tokens and the
   themes are still written; failures land in `assets.json` with reasons. Covered by the Task 5
   test `failed_downloads_are_recorded` and the Task 3 test `write_folder_without_assets`.
3. **A page with no chromatic colors** (a monochrome blog). No primary, secondary or accent; no
   invented blue; shadcn lists them as not seen. Covered by the Task 2 test `monochrome_has_no_roles`.
4. **Re-exporting the same host.** The old folder is replaced and no stale assets remain.
   Covered by the Task 3 test `write_folder_replaces_old_export`.
5. **Hover-inspecting quickly across a dense page.** The panel shows the latest element, with no
   backlog of stale answers. Covered by Task 7's `inspect_drops_stale` (drop when a newer request
   is pending) and a Task 11 step.

---

### Task 1: Colors (`site_design::color`)

**Files:**
- Create: `crates/core/src/site_design/mod.rs` (`pub mod color; …` as later tasks add them), `crates/core/src/site_design/color.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod site_design;`)

**Interfaces — Produces:**
- `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Rgba { pub r: u8, pub g: u8, pub b: u8, pub a: f32 }`,
  with `Serialize` as its hex string. Methods:
  - `to_hex(&self) -> String`: `#rrggbb`, or `#rrggbbaa` when `a < 1`.
  - `to_oklch(&self) -> (f64, f64, f64)`
  - `from_oklch(l, c, h) -> Option<Rgba>`: `None` out of gamut.
  - `oklch_css(&self) -> String`: `oklch(L C H)`. L and C have up to 4 decimals and H up to 2,
    with trailing zeros trimmed (`1.0000` → `1`). H is printed as `0` when C < 0.0005.
- `pub fn parse(css: &str) -> Option<Rgba>`. Accepts:
  - `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`;
  - `rgb[a]()` in comma or space syntax with `/ alpha`, numbers or percentages;
  - `hsl[a]()`;
  - `oklch()`, `oklab()`;
  - `color(srgb …)`, `color(display-p3 …)` (converted, then clamped);
  - `transparent`.
- `pub fn delta_e(a: Rgba, b: Rgba) -> f64` (OKLab Euclidean).
- `pub fn contrast_ratio(fg: Rgba, bg: Rgba) -> f64`: WCAG 2.x, with `fg` composited over `bg`
  when its alpha < 1.
- `pub fn ramp(base: Rgba) -> [Rgba; 11]`. Hue is kept. L comes from `RAMP_L` and C =
  base C × `RAMP_C`, reduced until in gamut. The step whose `RAMP_L` is nearest the base L is the
  base exactly.
- `pub const RAMP_L`, `RAMP_C`, `STEPS` (values in Global Constraints).

- [ ] **Step 1: Write failing tests.**
  - `parse`:
    - `"#0071e3"`, `"rgb(0, 113, 227)"` and `"rgb(0 113 227)"` → `#0071e3`.
    - `"rgb(0 113 227 / 50%)"` → a = 0.5. `"rgba(0,0,0,0)"` → a = 0.
    - `"hsl(210 100% 50%)"` → `#0080ff`.
    - `"oklch(0.628 0.2577 29.23)"` and `"color(srgb 1 0 0)"` → `#ff0000`, each channel ±1.
    - `"color(display-p3 1 0 0)"` → `#ff0000`.
    - `"#abc"` → `#aabbcc`. `"#11223380"` → a ≈ 0.502.
    - `"transparent"` → a = 0. `"bogus"` → `None`.
  - `to_oklch`: `#ff0000` ≈ (0.628, 0.2577, 29.23) and `#0071e3` ≈ (0.5629, 0.1933, 256.16)
    (±0.001, hue ±0.05°).
  - `contrast_ratio`: black/white `21.0`, `#777777`/white `4.48`, `#0071e3`/white `4.70`
    (±0.01).
  - `ramp`:
    - `ramp(#0071e3)[6] == #0071e3` and `ramp(#0f7fff)[5] == #0f7fff`.
    - L strictly decreases across the 11 steps.
    - Every step round-trips through `parse(to_hex())`.
  - `oklch_css(#ffffff) == "oklch(1 0 0)"`.
  - `delta_e(#ffffff, #fefefe) < 0.02`.
- [ ] **Step 2:** Run `cargo test -p sawit-core site_design::color`. Expected: FAIL.
- [ ] **Step 3: Implement.** Use Björn Ottosson's sRGB↔OKLab matrices. Display-P3 → linear
  sRGB uses the standard P3-D65 → XYZ → sRGB matrices.
- [ ] **Step 4:** Run `cargo test -p sawit-core site_design::color`. Expected: PASS.
- [ ] **Step 5:** Commit `feat(design): color parsing, OKLCH, contrast and ramps`.

### Task 2: Scan types and token derivation (`scan`, `tokens`)

**Files:**
- Create: `crates/core/src/site_design/scan.rs`, `crates/core/src/site_design/tokens.rs`, `crates/core/src/site_design/fixtures/scan.json`, `crates/core/src/site_design/fixtures/mono.json`

**Interfaces — Produces** (`scan.rs`, serde, snake_case; this is the JSON contract the page's `scan` op must produce in Task 6):
- `pub enum ColorRole { Text, Background, Border, Fill, Stroke, Shadow, Gradient }` (lowercase).
- `pub struct ColorUse { value: String, role: ColorRole, count: u32, #[serde(default)] on_button: u32, #[serde(default)] on_link: u32 }`
- `pub struct TextSample { tag: String, in_main: bool, family: String, used_family: Option<String>, size: f64, weight: u16, line_height: Option<f64>, letter_spacing: Option<f64>, color: String, chars: u32 }`.
  `tag` is the lowercase tag, or `"button"` for button-like elements.
- `pub struct FontFace { family: String, weight: String, style: String, src: Vec<String> }`
- `pub enum AssetKind { Image, Svg, Icon, Video, Lottie, Font }` (lowercase).
- `pub struct AssetRef { kind: AssetKind, url: Option<String>, svg: Option<String>, label: String, width: Option<u32>, height: Option<u32>, used_on: Vec<String>, logo: bool }`
- `pub struct ComponentStyle { background: String, color: String, radius: String, padding: String, font_family: String, font_size: f64, font_weight: u16, height: f64, border: String }`
- `pub struct ComponentSample { kind: String, selector: String, html: String, style: ComponentStyle }`.
  `kind` is one of `button`, `button-secondary`, `input`, `card`, `nav`, `link`.
- `#[derive(Serialize)] pub struct AssetOutcome { file: Option<String>, url: Option<String>, kind: AssetKind, bytes: u64, width: Option<u32>, height: Option<u32>, label: String, used_on: Vec<String>, error: Option<String> }`.
  One row of `assets.json`. Tasks 3 and 5 use it.
- `pub struct SheetRules { font_faces: Vec<FontFace>, variables: Vec<(String, String)>, breakpoints: Vec<u32>, focus_ring: Option<String> }`
- `pub struct Scan { url, title, viewport: (u32, u32), truncated: bool, colors: Vec<ColorUse>, texts: Vec<TextSample>, spacing: Vec<(f64, u32)>, radii: Vec<(String, u32)>, shadows: Vec<(String, u32)>, body_background: Option<String>, focus_ring: Option<String>, font_faces: Vec<FontFace>, variables: Vec<(String, String)>, breakpoints: Vec<u32>, assets: Vec<AssetRef>, components: Vec<ComponentSample>, blocked_sheets: Vec<String>, manifest: Option<String> }`.
  All fields are `pub`.
- `impl Scan { pub fn merge_rules(&mut self, rules: SheetRules) }`: dedupes font faces by
  (family, weight, style), variables by name (the page's own value wins), and breakpoints
  sorted and unique. Sets `focus_ring` if it was unset.

**Interfaces — Produces** (`tokens.rs`, all `Serialize`):
- `pub struct Ramp { pub role: String, pub base: Rgba, pub steps: [Rgba; 11], pub uses: u32 }`
- `pub struct TypeToken { pub family: String, pub size: f64, pub weight: u16, pub line_height: Option<f64>, pub letter_spacing: Option<String> }`.
  `line_height` is a ratio rounded to 2 decimals; `letter_spacing` is `em`, 3 decimals.
- `pub struct ComponentToken { pub name: String, pub props: Vec<(String, String)> }`.
  `name` is `button-primary`, `button-secondary`, `input`, `card` or `nav-link`. `props` keys are
  `backgroundColor`, `textColor`, `typography`, `rounded`, `padding`, `height`; values are
  `{colors.x}`, `{rounded.x}`, `{typography.x}` refs, or literals.
- `pub struct Facts { pub primary_buttons: u32, pub primary_links: u32, pub smallest_text: f64, pub dark: bool, pub colors_seen: usize, pub body: Option<TypeToken> }`
- `pub struct Tokens { pub site: String, pub url: String, pub date: String, pub palette: Vec<Ramp>, pub semantic: Vec<(String, Rgba)>, pub families: Vec<(String, String)>, pub type_roles: Vec<(String, TypeToken)>, pub type_scale: Vec<(String, f64)>, pub spacing_base: Option<f64>, pub spacing: Vec<(String, f64)>, pub radius: Vec<(String, String)>, pub shadows: Vec<(String, String)>, pub breakpoints: Vec<u32>, pub variables: Vec<(String, String)>, pub components: Vec<ComponentToken>, pub facts: Facts }`
- `pub fn site_name(url: &str) -> String`: `https://www.apple.com/x` → `Apple`; `localhost:5173` → `Localhost`.
- `pub fn derive(scan: &Scan, date: &str) -> Tokens`
- `pub fn parse_box_shadow(s: &str) -> Option<Shadow>`, with
  `pub struct Shadow { pub color: Rgba, pub x: f64, pub y: f64, pub blur: f64, pub spread: f64, pub inset: bool }`.
  Takes the first shadow of a list.

**Rules:** exactly the spec's "Derivation rules", with these precisions:
- **Type roles:**
  - `body-md` = the sample with the most `chars` among `p`, `li`, `dd`, `td` and `span` with
    `in_main` (all of them when none is `in_main`).
  - `h1` → `display`, `h2` → `headline-lg`, `h3` → `headline-md`, `h4` → `title`,
    `button`/`label` → `label`, `small`/`figcaption` → `caption`. Per role, the sample with the
    most `chars`.
- **Families:**
  - `sans` or `serif`: the body sample's stack, by its last generic (`sans-serif` when there
    is none).
  - `display`: the `h1` stack when its used family differs from the body's.
  - `mono`: the most-used stack ending in `monospace`.
- **Variables:** sorted by name, at most 200.
- **Ramp roles:** in order primary, secondary, accent, neutral, each only when found. `uses` =
  the summed counts of colors clustered into its base (ΔE < 0.02).
- **Component refs:** a value becomes a ref when it equals a token.
  - Colors: compared by hex against semantic colors first, then ramp steps (`{colors.primary-600}`).
  - Radius: its normalized px, or `full`.
  - Typography: same family + size + weight.

`fixtures/scan.json` is written by hand to match the Apple-like case:
- Text `rgb(29, 29, 31)` ×300, background `rgb(255, 255, 255)` ×200, background
  `rgb(245, 245, 247)` ×40, text `rgb(110, 110, 115)` ×60, border `rgb(210, 210, 215)` ×50.
- Background `rgb(0, 113, 227)` ×14 (`on_button` 12), text `rgb(0, 102, 204)` ×31 (`on_link` 31),
  text `rgb(191, 72, 0)` ×6, text `rgb(41, 163, 84)` ×3.
- `body_background` `rgb(255, 255, 255)`, `focus_ring` `rgb(0, 113, 227)`.
- Text samples:
  - `p` 17/400, line-height 25, 5 000 chars, in main.
  - `h1` 48/600 (family "SF Pro Display"), `h2` 40/600, `h3` 28/600, `h4` 21/600.
  - `button` 17/400 lh 20, `small` 12/400, `span` 14/400 (800 chars, in main), `div` 21/400
    (200 chars, in main).
- Spacing: `[[4,50],[8,80],[12,40],[16,120],[20,30],[24,60],[32,25],[48,15],[64,10],[13,3]]`.
- Radii: `[["8px",30],["980px",12],["18px",20],["12px",15],["50%",5],["4px",3]]`.
- Shadows, by count: `"rgba(0, 0, 0, 0.04) 0px 1px 2px 0px"` ×20,
  `"rgba(0, 0, 0, 0.08) 0px 2px 8px 0px"` ×10, `"rgba(0, 0, 0, 0.16) 0px 8px 30px 0px"` ×4.
- Breakpoints `[1068, 734]`.
- One `button` component: bg `rgb(0, 113, 227)`, color `rgb(255, 255, 255)`, radius `980px`,
  padding `8px 16px`, SF Pro Text 17/400, height 36.

`mono.json` has only grays.

- [ ] **Step 1: Write failing tests** (`tokens.rs`, loading the fixtures with `include_str!`):
  - `roles_from_the_fixture`:
    - Palette roles are `[primary #0071e3, secondary #bf4800, accent #29a354, neutral #1d1d1f]`.
    - `ramp` of neutral has `steps[10] == #1d1d1f`.
  - `semantic_from_the_fixture`:
    - `background #ffffff`, `foreground #1d1d1f`, `muted #f5f5f7`, `muted-foreground #6e6e73`,
      `border #d2d2d7`.
    - `primary #0071e3`, `primary-foreground #ffffff`, `ring #0071e3`.
  - `type_from_the_fixture`:
    - `body-md` = 17/400 lh 1.47.
    - `body-lg` = 21, from the `div` sample. `h4` is also 21 but is a heading, so it doesn't
      count.
    - `body-sm` = 14, `display` = 48/600 "SF Pro Display".
    - `type_scale` = `[("xs",12),("sm",14),("base",17),("lg",21),("xl",28),("2xl",40),("3xl",48)]`.
  - `layout_from_the_fixture`:
    - `spacing_base == Some(4.)`, spacing `[xs 4, sm 8, md 12, lg 16, xl 20, 2xl 24, 3xl 32]`.
    - Radius `[sm 4px, md 8px, lg 12px, xl 18px, full 9999px]`.
    - Shadows `[sm …1px 2px…, md …2px 8px…, lg …8px 30px…]`.
    - Breakpoints `[734, 1068]`.
  - `components_use_refs`: `button-primary` props are `backgroundColor {colors.primary}`,
    `textColor {colors.primary-foreground}`, `rounded {rounded.full}`, `padding 8px 16px`,
    `typography {typography.label}`, `height 36px`.
  - `monochrome_has_no_roles`: `mono.json` → palette only `neutral`; no `primary` in `semantic`.
  - `site_names`: `site_name("https://www.apple.com/iphone/") == "Apple"`.
  - `box_shadows`: `parse_box_shadow("rgba(0, 0, 0, 0.08) 0px 2px 8px 0px")` → blur 8, y 2.
  - `merge_rules_dedupes`: a variable present in both keeps the page's value; breakpoints come
    out sorted and unique.
- [ ] **Step 2:** Run `cargo test -p sawit-core site_design`. Expected: FAIL.
- [ ] **Step 3:** Implement. Write `fixtures/scan.json` and `mono.json` as described.
- [ ] **Step 4:** Run `cargo test -p sawit-core site_design`. Expected: PASS.
- [ ] **Step 5:** Commit `feat(design): scan contract and token derivation`.

### Task 3: Exports (`site_design::export`)

**Files:**
- Create: `crates/core/src/site_design/export.rs`

**Interfaces — Consumes:** `Tokens`, `Scan`, `AssetOutcome` (Task 2).
**Produces:**
- `pub fn design_md(t: &Tokens) -> String`
- `pub fn dtcg(t: &Tokens) -> serde_json::Value`
- `pub fn tailwind_theme(t: &Tokens) -> String`
- `pub fn shadcn_css(t: &Tokens) -> String`
- `pub fn variables_css(vars: &[(String, String)]) -> String`
- `pub fn readme(t: &Tokens, files: &[String]) -> String`
- `pub fn write_folder(dir: &Path, t: &Tokens, scan: &Scan, screenshot_png: Option<&[u8]>, assets: &[AssetOutcome]) -> std::io::Result<Vec<String>>`:
  1. Creates `dir` if missing, and removes everything in it except `assets/`. The app clears
     and refills `assets/` just before this call (Task 7).
  2. Writes `DESIGN.md`, `tokens.json` (pretty), `theme.css`, `shadcn.css`, `variables.css`,
     `screenshot.png` (when given), `components/<kind>.html`, `assets.json` and `README.md`.
  3. Returns the relative paths, including the files under `assets/`, sorted.

**Formats:**
- **Tailwind:**
  - `@theme {` with `--color-<role>-<step>`, `--color-<semantic>`, `--font-<family role>`,
    `--text-<scale>`, `--spacing: <base>px` and `--radius-<name>` (`full` → `9999px`).
  - `--shadow-<name>` and `--breakpoint-<sm|md|lg|xl|2xl>` from the sorted breakpoints, first 5.
- **shadcn:**
  - `:root {` with `--radius: <md radius>`, and in `oklch_css`:
    - `--background`, `--foreground`, `--card` = background, `--card-foreground` =
      foreground, `--popover` = background, `--popover-foreground` = foreground;
    - `--primary`, `--primary-foreground`;
    - `--secondary` = muted, `--secondary-foreground` = foreground;
    - `--muted`, `--muted-foreground`, `--accent` = muted, `--accent-foreground` = foreground;
    - `--border`, `--input` = border, `--ring`.
  - Each is written only when its source token exists.
  - It ends with `/* Not seen on the page: destructive, chart-1, chart-2, chart-3, chart-4, chart-5, sidebar — keep your theme's values. */`
    plus any of the above that were skipped.
- **DTCG:** groups `color.<role>.<step>` and `color.<semantic>` (`$type` `color`),
  `font.family.*` (`fontFamily`), `font.size.*`, `spacing.*`, `radius.*` (`dimension`, `"17px"`)
  and `shadow.*` (`$type` `shadow`, `$value` `{color, offsetX, offsetY, blur, spread}` from
  `parse_box_shadow`; unparsable ones are skipped).
- **DESIGN.md** prose comes from templates over `Tokens`/`Facts`. Exact copy for the pinned lines:
  - Colors: ``- **Primary** `#0071e3`: used on 12 buttons and 3 links; reserve it for actions.``.
    Leave out the zero parts: "used on 12 buttons;".
  - Typography: ``Body text is 17px/1.47 SF Pro Text; nothing on the page is set below 12px.``
  - Overview: ``A light interface built on SF Pro Text, with N colors in use.``
    (`dark` → "A dark interface").
  - Do's and Don'ts:
    - ``- Do use `{colors.primary}` only for primary actions.`` (when primary exists)
    - ``- Don't set text below 12px.``
    - ``- Do keep spacing on the 4px grid.`` (when `spacing_base` is `Some`)

- [ ] **Step 1: Write failing tests** (with `Tokens` from Task 2's fixture):
  - `design_md_front_matter`: it starts with
    `"---\nversion: alpha\nname: Apple design system\ndescription: Extracted from "`.
    It contains `"colors:\n  background: \"#ffffff\""`, `"  primary-600: \"#0071e3\""`,
    `"rounded:\n  sm: 4px"` and
    `"components:\n  button-primary:\n    backgroundColor: \"{colors.primary}\""`.
  - `design_md_sections_in_order`: the indexes of the 8 `## ` headings increase. Tokens with no
    shadows have no `## Elevation & Depth`.
  - `design_md_prose`: contains ``"- **Primary** `#0071e3`: used on 12 buttons; reserve it for actions."``.
  - `dtcg_shape`:
    - `v["color"]["primary"]["600"]["$value"] == "#0071e3"` with `$type` `"color"`.
    - `v["font"]["size"]["base"]["$value"] == "17px"`.
    - `v["shadow"]["md"]["$value"]["blur"] == "8px"`.
  - `tailwind_theme_lines`: contains `"--color-primary-600: #0071e3;"`, `"--text-base: 17px;"`,
    `"--spacing: 4px;"`, `"--radius-full: 9999px;"`, `"--breakpoint-sm: 734px;"` and
    `"--breakpoint-md: 1068px;"`.
  - `shadcn_lines`: contains `"--primary: oklch(0.5629 0.1933 256.16);"`,
    `"--background: oklch(1 0 0);"` and `"Not seen on the page: destructive"`. Mono tokens list
    `primary` as not seen.
  - `variables_css_lists_them`: `variables_css(&[("--a".into(),"1px".into())]) == ":root {\n  --a: 1px;\n}\n"`.
  - `readme_has_the_license_line`.
  - `write_folder_replaces_old_export`: in a tempdir with a stale `old.txt` and
    `assets/x.png`, `old.txt` is gone, `assets/x.png` stays, and all listed files exist.
  - `write_folder_without_assets`: with `screenshot_png = None` and empty assets, everything else
    is written and `assets.json` is `[]`.
- [ ] **Step 2:** Run `cargo test -p sawit-core site_design::export`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p sawit-core site_design`. Expected: PASS.
- [ ] **Step 5:** Commit `feat(design): DESIGN.md, DTCG, Tailwind and shadcn exports`.

### Task 4: Inspect details and Tailwind (`inspect`, `tailwind`)

**Files:**
- Create: `crates/core/src/site_design/inspect.rs`, `crates/core/src/site_design/tailwind.rs`, `crates/core/src/site_design/fixtures/style_button.json`

**Interfaces — Produces** (`inspect.rs`, serde; the JSON contract of the page's `style` op):
- `pub struct StyleInfo { selector, label, rect: annotations::Rect, #[serde(rename = "box")] box_model: BoxModel, typography: Typography, background: Background, border: Border, effects: Effects, layout: Layout, states: Vec<(String, Vec<(String, String)>)>, effective_background: Option<String>, html: Option<String> }`
- `BoxModel { margin: [f64; 4], border: [f64; 4], padding: [f64; 4] }` (top, right, bottom, left).
- `Typography { family, used_family: Option<String>, size: f64, weight: u16, line_height: Option<f64>, letter_spacing: Option<f64>, color, align, transform }`
- `Background { color, image: Option<String> }`
- `Border { color, style, width: [f64; 4], radius: String }`
- `Effects { shadow: Option<String>, opacity: f64, transition: Option<String> }`
- `Layout { display, position, gap: Option<String>, flex_direction: Option<String>, justify: Option<String>, align: Option<String>, grid_template_columns: Option<String> }`
- `pub struct Contrast { pub ratio: f64, pub large: bool, pub aa: bool, pub aaa: bool }`
- `pub fn contrast(info: &StyleInfo) -> Option<Contrast>`. Large = size ≥ 24, or ≥ 18.66 with
  weight ≥ 700. AA is 4.5 normal / 3 large; AAA is 7 normal / 4.5 large. The background is
  `effective_background`, else white.
- `pub fn css_rule(info: &StyleInfo) -> String`: `selector {\n  prop: value;\n}\n`. Properties
  in this order, skipping defaults:
  1. display (not block/inline), position (not static), padding, margin, gap,
     flex-direction, justify-content, align-items;
  2. font-family (used stack), font-size, font-weight, line-height, letter-spacing (not
     normal), text-align (not start/left), text-transform (not none), color;
  3. background-color (not transparent), background-image;
  4. border (width>0, as `Wpx style color`), border-radius (not 0px), box-shadow, opacity (not
     1), transition (not `all 0s ease 0s`).

**Produces** (`tailwind.rs`):
- `pub fn spacing_class(prefix: &str, px: f64) -> String`:
  - `0` → `-0`, `1` → `-px`.
  - A multiple of 2px → `-{px/4}` (with `.5`).
  - Otherwise `-[Npx]`.
- `pub fn classes(info: &StyleInfo) -> Vec<String>`. Order:
  1. Layout: display (`flex`, `inline-flex`, `grid`, `inline-grid`, `block`, `inline-block`,
     `hidden`, `contents`), `flex-col`, `justify-*`, `items-*`, `gap-*`.
  2. Position: `relative`, `absolute`, `fixed`, `sticky`.
  3. Padding: `p`, else `px`/`py`, else `pt`/`pr`/`pb`/`pl`. Then margin the same way.
  4. Text size: the v4 names `xs 12, sm 14, base 16, lg 18, xl 20, 2xl 24, 3xl 30, 4xl 36, 5xl 48,
     6xl 60, 7xl 72, 8xl 96, 9xl 128`, else `text-[Npx]`.
  5. Weight: `font-thin` … `font-black`.
  6. Leading: named for ratio 1 / 1.25 / 1.375 / 1.5 / 1.625 / 2, else `leading-[Npx]`.
  7. Tracking: named for −0.05 / −0.025 / 0 / 0.025 / 0.05 / 0.1 em, else `tracking-[Nem]`.
  8. `text-center` / `text-right`, `uppercase` / `lowercase` / `capitalize`.
  9. `text-[#hex]`, `bg-[#hex]`.
  10. Border: `border` / `border-N` / `border-[Npx]` + `border-[#hex]`.
  11. Radius: `rounded-xs 2`, `sm 4`, `md 6`, `lg 8`, `xl 12`, `2xl 16`, `3xl 24`, `4xl 32`,
      `full` (≥ 500px or ≥ 50 %), else `rounded-[Npx]`.
  12. `shadow-[…]` with spaces as `_`.
  13. `opacity-N` for multiples of 5, else `opacity-[.N]`.

- [ ] **Step 1: Write failing tests.** `style_button.json` is a button with selector
  `button.buy`:
  - Layout: `inline-flex`, justify and align `center`.
  - Padding 8/16/8/16; margin, border widths and letter-spacing are 0/normal.
  - Text: 17px/400, line-height 20px, `align: "start"`, `transform: "none"`.
  - Colors: white text, bg `rgb(0, 113, 227)`, `effective_background` `rgb(0, 113, 227)`.
  - Radius 980px, no shadow, opacity 1.
  - `spacing_class("p",16.)=="p-4"`, `("p",6.)=="p-1.5"`, `("p",13.)=="p-[13px]"`,
    `("p",1.)=="p-px"`, `("p",0.)=="p-0"`.
  - `classes(button) == ["inline-flex","justify-center","items-center","px-4","py-2","text-[17px]","font-normal","leading-[20px]","text-[#ffffff]","bg-[#0071e3]","rounded-full"]`.
  - `contrast(button)` → ratio 4.70 ±0.01, `large == false`, `aa == true`, `aaa == false`.
  - `css_rule(button)` starts `"button.buy {\n  display: inline-flex;\n"` and contains
    `"  border-radius: 980px;\n"`.
- [ ] **Step 2:** Run `cargo test -p sawit-core site_design`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p sawit-core site_design`. Expected: PASS.
- [ ] **Step 5:** Commit `feat(design): inspect details, contrast, CSS and Tailwind output`.

### Task 5: Fetching and assets (`site_design::assets`)

**Files:**
- Create: `crates/core/src/site_design/assets.rs`

**Interfaces — Consumes:** `AssetRef`, `AssetKind`, `AssetOutcome`, `FontFace` (Task 2).
**Produces:**
- `pub const SAFARI_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";`
- `pub const MAX_TOTAL: u64 = 300 * 1024 * 1024;`
- `pub fn sub_dir(kind: AssetKind) -> &'static str`: `images`, `svg`, `icons`, `video`,
  `lottie`, `fonts`.
- `pub fn file_name(asset: &AssetRef, taken: &mut HashSet<String>) -> String`.
  - Stem: a slug of `label` (lowercase ASCII letters, digits and `-`, ≤ 60) unless the label
    is empty. Else the URL's last path segment stem. Else the kind name.
  - Extension: from the URL (`png jpg jpeg gif webp avif svg ico mp4 webm mov json lottie woff2
    woff ttf otf`), else by kind: svg → `svg`, lottie → `json`, font → `woff2`, image/icon →
    `png`, video → `mp4`.
  - Unique within `taken` as `-2`, `-3`, ….
- `pub fn decode_data_url(url: &str) -> Option<(String, Vec<u8>)>` (base64 and percent-encoded).
- `pub fn curl_args(url: &str, out: &Path, referer: &str) -> Vec<OsString>`:
  `-fsSL --compressed --max-time 30 --max-filesize 52428800 -A <UA> -e <referer> -o <out> <url>`.
- `pub async fn fetch_text(url: &str, referer: &str) -> crate::Result<String>`: curl, 10 s,
  `--max-filesize 5242880`.
- `pub fn font_assets(faces: &[FontFace]) -> Vec<AssetRef>`: one `Font` per `src` URL, with
  label `family-weight-style`.
- `pub async fn download_all(items: Vec<AssetRef>, assets_dir: &Path, referer: &str, include_videos: bool, budget: u64, progress: impl Fn(usize, usize) + Send + Sync) -> Vec<AssetOutcome>`.
  - Local items (inline SVG, data URLs) are handled first, in order, without curl.
  - `blob:` gets the blob error.
  - Videos get "Videos weren't included." when `!include_videos`.
  - Then six curl jobs run at a time (`buffer_unordered(6)`).
  - The budget is checked as each item starts. Once the bytes written so far reach `budget`,
    every later item gets "Skipped: the {budget / 1 MiB} MB export limit was reached."
  - curl failures carry its stderr, trimmed.

- [ ] **Step 1: Write failing tests** (no network):
  - `file_names`:
    - label "Hero photo" + `…/hero_large.jpg?x=1` → `hero-photo.jpg`, and again →
      `hero-photo-2.jpg`.
    - `""` + `…/icons/cart.svg` → `cart.svg`.
    - Inline SVG labelled `logo` → `logo.svg`.
    - `""` + `data:image/png;base64,…` → `image.png`.
  - `data_urls`: `"data:image/svg+xml;base64,PHN2Zy8+"` → `("image/svg+xml", b"<svg/>")`;
    `"data:text/plain,hello%20world"` → `b"hello world"`.
  - `curl_args_carry_the_referer`: `-e` is followed by the referer, `--max-filesize` by
    `52428800`, and the last arg is the URL.
  - `failed_downloads_are_recorded` (in a tempdir):
    - Items: an inline `<svg/>`, a 600-byte data PNG, a second 600-byte data PNG, a video with
      an `http` URL and a `blob:` image, with `budget = 500`, `include_videos = false`.
    - Outcomes: svg written; first PNG written (the total now passes 500); second PNG has an
      error starting with "Skipped: the"; video "Videos weren't included."; blob with its
      message. No curl is run.
  - `fonts_become_assets`: one face with two `src` URLs gives two Font assets labelled
    `SF Pro Text-400-normal`.
- [ ] **Step 2:** Run `cargo test -p sawit-core site_design::assets`. Expected: FAIL.
- [ ] **Step 3:** Implement with `crate::cmd::run("/usr/bin/curl", curl_args(..), Duration::from_secs(40))`,
  the same way `update.rs` does.
- [ ] **Step 4:** Run `cargo test -p sawit-core site_design`. Expected: PASS.
- [ ] **Step 5:** Commit `feat(design): asset naming, data URLs and capped downloads`.

### Task 6: The page's design module (`browser_design.js`)

**Files:**
- Create: `crates/core/src/browser_design.js`
- Modify: `crates/core/src/browser.rs` (`pub const DESIGN_JS`, added to `page_script`'s module
  list), `crates/core/src/browser_overlay.js` (Alt-measure in inspect mode)

**Interfaces — Consumes:** the JSON contracts `Scan`, `SheetRules` (Task 2) and `StyleInfo`
(Task 4). Field names must match exactly.
**Produces** (page ops through `run`):
- `{"op":"scan"}` → `Scan`.
- `{"op":"rules","sheets":[{"href","text"}]}` → `SheetRules`. It also caches the parsed sheets
  for `style`'s states.
- `{"op":"style","ref"|"selector":…, "html":bool}` → `StyleInfo`.

- [ ] **Step 1: Write a failing test.** Extend `page_script_splices_the_modules` with
  `s.contains("blocked_sheets")`, and `s.find("blocked_sheets") < s.find("const OPS")`.
- [ ] **Step 2:** Run `cargo test -p sawit-core browser`. Expected: FAIL.
- [ ] **Step 3: Implement the module.** It is a bare block ending in
  `Object.assign(modules, { scan, rules, style })`. It uses `const` arrows only, the agent's
  `resolve(op)` for `ref`/`selector`, and `helpers.selectorFor` from the overlay.
  - **`scan`:**
    - **Walk:** a `TreeWalker` over `document.body` elements, skipping `[data-sawit-overlay]`
      and invisible ones (zero rect or `visibility:hidden`). It stops at 6 000 →
      `truncated: true`.
    - **Colors:** text color (only for elements with a direct non-blank text node),
      `background-color`, border colors where the width is > 0, SVG `fill`/`stroke`, colors
      inside `box-shadow`, and gradient stops in `background-image`. Count by (computed value,
      role). `on_button` applies to button-like elements: `button`, `[role=button]`,
      `input[type=submit|button]`, and a class matching `/\bbtn\b|button/i`. `on_link`
      applies to `a` that isn't button-like.
    - **Text samples:** keyed by (tag bucket, family, size, weight, lh, ls, color, in_main),
      summing `chars`.
      - `used_family` is the first stack entry where `document.fonts.check("16px " + quoted)`
        is true, or the generic.
      - The tag bucket is the tag, or `button` for button-like elements.
    - **Spacing:** px of margin/padding sides and `gap` > 0, rounded to 0.5. **Radii:** the
      `border-top-left-radius` string, or the largest corner when they differ. **Shadows:**
      `box-shadow` ≠ none.
    - **Readable sheets:** `cssRules`; a throw → `blocked_sheets`. Collect `@font-face`
      (src URLs resolved against the sheet href), custom properties from `:root`/`html` rules
      (values via `getComputedStyle(document.documentElement)`), `@media` widths
      (`(min|max)-width: N(px|em|rem)` → px, em/rem × 16) and `focus_ring` (the outline
      color of `:focus-visible`/`:focus` rules).
    - **Assets:**
      - `img` (`currentSrc`, natural size, alt).
      - `img[src$=".svg" i]` as `svg`.
      - CSS `url()` backgrounds that aren't gradients.
      - Top-level inline `svg`, deduped by markup; markup ≤ 100 KB, else skipped.
      - `link[rel~=icon]`, `apple-touch-icon`, `mask-icon`, and inline SVGs ≤ 48 px as `icon`.
      - `video` (src + poster as an image labelled "… poster").
      - `lottie-player[src]`, `dotlottie-player[src]`, and resource entries ending
        `.lottie`, or `.json` with "lottie" in the URL.
      - Labels and logo per the spec. `used_on` ≤ 3 selectors (`selectorFor` from the overlay).
    - **`manifest`:** `link[rel=manifest]` href, absolute.
    - **Components:** pick one sample each.
      - `button`: the most common (bg, color, radius, padding) signature among button-like
        elements with a chromatic background.
      - `button-secondary`: the next signature, including transparent bg with a border.
      - `input`: the first text-like input or textarea.
      - `card`: the most common signature of 120–800 px wide boxes with radius ≥ 4, a shadow or
        a border, holding a heading or an `img`.
      - `nav`: `header nav`, else `nav`.
      - `link`: the most common `a` in `main` that isn't button-like.
      - Each sample's `html` is its outerHTML with computed styles inlined, for ≤ 200
        descendants, with `script` and `on*` attributes stripped and the whole ≤ 30 KB.
    - **Caps:** colors ≤ 500, text keys ≤ 400, assets ≤ 400. Answer ≤ 2 MB (lists cut, with
      `truncated`).
  - **`rules`:** each `{href, text}` goes through `new CSSStyleSheet().replaceSync(text without @import lines)`.
    The same collectors as for readable sheets apply. The sheets are cached for `style`.
  - **`style`:**
    - It resolves `ref` like the agent and reads computed styles into `StyleInfo`.
    - **States:** for every rule in the readable and cached sheets whose selector contains
      `:hover`, `:focus`, `:focus-visible` or `:active`, strip those pseudo-classes and test
      `el.matches(base)` in a try/catch. Collect `[prop, value]` pairs per state.
    - **`effective_background`:** walk up to the first non-transparent `background-color`,
      else `rgb(255, 255, 255)`.
    - **`html: true`:** inlined outerHTML as in components.
  - **Overlay addition:** in inspect mode, after a pick, holding Alt while hovering draws
    distance lines and px labels between the picked and hovered boxes.
- [ ] **Step 4:** Run `node --check crates/core/src/browser_design.js && node --check crates/core/src/browser_overlay.js && cargo test -p sawit-core browser`. Expected: pass.
- [ ] **Step 5:** Commit `feat(design): page module to scan styles, sheets and assets`.

### Task 7: Design flow in the app (scan, inspect, export)

**Files:**
- Create: `crates/app/src/browser/design/mod.rs`, `crates/app/src/browser/design/flow.rs`
- Modify: `crates/core/src/browser.rs` (ops), `crates/app/src/browser/mod.rs`

**Interfaces — Produces** (core `BrowserOp`, all native):
- `Inspect { #[serde(flatten)] target: Target, format: Option<String> }`
- `Design { part: Option<String> }`
- `ExportDesign { #[serde(default)] include_videos: bool }`
- `ExportStatus`

**Interfaces — Produces** (app, `design/flow.rs`):
- `pub struct Ready { pub scan: Scan, pub tokens: Tokens }`
- `pub enum ScanState { Idle, Scanning, Ready(Rc<Ready>), Failed(SharedString) }`
- `pub struct ExportJob { pub folder: PathBuf, pub done: usize, pub total: usize, pub result: Option<Result<ExportReport, String>> }`
- `pub struct ExportReport { pub folder: PathBuf, pub files: Vec<String>, pub failed: Vec<AssetOutcome> }`
- `impl BrowserView`:
  - `pub(crate) fn scan_design(&mut self, cx) -> Task<Result<Rc<Ready>, String>>`. Steps:
    1. Page `scan`. Check first with `fn extractable(url: &str) -> bool`: only `http(s)` and
       `file` are extractable. A page that isn't, or a scan with no colors and no text, errs
       with "Nothing to extract on this page."
    2. `fetch_text` for each `blocked_sheets` href, in parallel, 20 s overall.
    3. Page `rules`, then `merge_rules`.
    4. Manifest icons: fetch, parse `icons[].src` against the manifest URL, add as `Icon` assets.
    5. `derive(&scan, &date::local_date())`.

    The result is cached per `page_key(url)` and cleared on `Load{started:true}` and `Url`.
  - `pub(crate) fn inspect(&mut self, target: Value, html: bool, cx) -> Task<Result<StyleInfo, String>>`.
    It keeps `inspect_seq: u64`; an answer whose seq isn't the latest is dropped.
  - `pub(crate) fn start_export(&mut self, include_videos: bool, cx) -> Result<PathBuf, String>`.
    Steps:
    1. Clear `<folder>/assets`.
    2. `download_all(scan.assets + font_assets(scan.font_faces), …, MAX_TOTAL, progress)`.
    3. `shoot(None)` → screenshot.
    4. `write_folder`.

    It returns the folder at once. A second call while running returns the same folder.
  - `fn export_folder(&self, host: &str) -> PathBuf`: `nyawit::dir(project)/design/<host>`
    (`nyawit::ensure` first), else `~/Downloads/sawit-design/<host>`.
  - `fn design_part(ready: &Ready, part: &str) -> Result<Value, String>`: `summary`, `colors`,
    `fonts`, `assets`, `tokens`, `variables`. An unknown part errs with "Unknown part “x”. Use
    summary, colors, fonts, assets, tokens or variables." Output is capped at 60 000 bytes by
    cutting lists and adding `"note": "Cut at 60 KB; ask for one part."`.
- **Ops:**
  - `Design` → `scan_design` then `design_part`.
  - `Inspect` → `inspect`, then by `format`: `json` = `StyleInfo` + `contrast`; `css` =
    `css_rule`; `tailwind` = `classes` joined by spaces; `html` = the inlined html.
  - `ExportDesign` → `{"export":"started"|"running","folder"}`.
  - `ExportStatus` → `{"done","progress":[d,t],"folder","files","failed","error"}`, or an error
    "No export has run in this browser tab."

- [ ] **Step 1: Write failing tests.**
  - Core: the 4 ops round-trip; `is_native()` is true for each.
  - App (`flow.rs`):
    - `design_part_caps_output`: a `Ready` with 5 000 colors → ≤ 60 000 bytes and a `note`.
    - `unknown_part_is_explained`.
    - `extractable_pages`: `extractable("https://a.dev")` and `("file:///x.html")` are true;
      `("about:blank")` and `("data:text/html,x")` are false.
    - `inspect_drops_stale`: `fn is_latest(seq, current) -> bool`.
    - `export_folder_without_project` ends with `Downloads/sawit-design/www.apple.com`.
  - Core: `merge_rules_adds_fetched_sheets` (`scan.rs`). A `Scan` with `blocked_sheets` merged
    with `SheetRules{font_faces:[SF], variables:[--x], breakpoints:[734]}` gains all three.
- [ ] **Step 2:** Run `cargo test -p sawit-core browser site_design && cargo test -p sawit browser`. Expected: FAIL.
- [ ] **Step 3:** Implement. Async work goes through `rt::spawn` (tokio) and is awaited in
  `cx.spawn`, as the dev-server pick in `BrowserView::new` does.
- [ ] **Step 4:** Run the same commands, then `cargo build -p sawit`. Expected: pass.
- [ ] **Step 5:** Commit `feat(design): scan, inspect and background export in the browser tab`.

### Task 8: Design panel: Inspect, Colors, Fonts

**Files:**
- Create: `crates/app/src/browser/design/inspect.rs`, `colors.rs`, `fonts.rs`
- Modify: `crates/app/src/browser/design/mod.rs` (`DesignPanel` view and its segments), `crates/app/src/browser/mod.rs` (Design panel body, `Hovered`/`Picked` in inspect mode)

**Interfaces — Produces:**
- `pub enum Segment { Inspect, Colors, Fonts, Assets, System }`
- `pub struct DesignPanel { segment, scan: ScanState, hovered: Option<StyleInfo>, pinned: Option<StyleInfo>, export: Option<ExportJob>, note: Option<(SharedString, Instant)> }`
- `pub enum DesignEvent { Flash(String), SaveAssets(Vec<AssetRef>), SaveAll { include_videos: bool }, Export { include_videos: bool }, Reveal(PathBuf) }`
- Render functions:
  - `inspect::render(info: Option<&StyleInfo>, pinned: bool, cx) -> impl IntoElement`
  - `colors::render(scan: &Scan, cx)`
  - `fonts::render(scan: &Scan, tokens: &Tokens, cx)`
- `fn group_colors(scan: &Scan) -> Vec<(ColorRole, Vec<(Rgba, u32)>)>`: exact duplicates by hex
  merged, each group sorted by count descending. The group order is Text, Background, Border,
  SVG fill/stroke (Fill then Stroke), Shadow, Gradient.

- [ ] **Step 1: Write a failing test.** `group_colors` merges `rgb(0, 113, 227)` and `#0071e3`
  in the same role into one entry with summed counts. Groups follow the role order.
- [ ] **Step 2:** Run `cargo test -p sawit browser`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **Segments:** a row of xsmall `Button`s with `.selected`. Opening the panel or a page load
    starts `scan_design`. A spinner shows while scanning, and failures as text.
  - **Inspect:** a box-model diagram (nested bordered boxes with numbers) and the property
    sections from the spec. States are listed per pseudo-class. The contrast row has the ratio
    and AA/AAA badges. Buttons: **CSS**, **Tailwind**, **HTML** copy to the clipboard with the
    footer note "Copied". `Hovered` updates the view; `Picked` in inspect mode pins (Unpin
    button).
  - **Colors:** a swatch (`div().size_4().bg(...)`), hex and count per row. Click copies the
    hex. The row menu copies `rgb()`, `oklch()`, the CSS variable line and the Tailwind class
    for the role.
  - **Fonts:** grouped by used family. Shows weights, sizes and line-heights, and a sample line
    in that family when `cx.text_system().all_font_names()` has it. The source is "Web font"
    (with its file count) or "System". Buttons: copy `font-family` CSS, copy `font-[…]`, and
    **Save files** (emits `SaveAssets` for that family's font assets). Footer license line.
- [ ] **Step 4:** Run `cargo test -p sawit browser && cargo build -p sawit`. Expected: pass.
- [ ] **Step 5:** Commit `feat(design): inspect, colors and fonts in the design panel`.

### Task 9: Design panel: Assets, System, Export

**Files:**
- Create: `crates/app/src/browser/design/assets.rs`, `system.rs`
- Modify: `crates/app/src/browser/design/mod.rs`, `crates/app/src/browser/mod.rs` (handle `DesignEvent`)

**Interfaces — Consumes:** `start_export`, `download_all`, `flash` (Plan 1), `ExportJob`.

- [ ] **Step 1: Write a failing test.** `fn asset_counts(scan: &Scan) -> Vec<(AssetKind, usize)>`
  returns the five tabs in the order Images, SVG, Icons, Video, Lottie, with zero counts kept.
- [ ] **Step 2:** Run `cargo test -p sawit browser`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **Assets:** kind chips with counts. Rows show the label, kind, `W×H` and the first
    `used_on`. Clicking a row → `Flash(selector)`. **Save** per row, **Save all**, and the
    **Include videos** `Checkbox` (off). Saving downloads into the export folder's `assets/`
    without a full export, then notes "Saved 12 files to .nyawit/design/www.apple.com/assets".
  - **System:**
    - Ramps as 11 swatches per role, and semantic swatches.
    - Font families, the type scale (each line rendered at its size up to 48 px), spacing
      bars, radius samples, shadow names, breakpoints and variables (scrollable, ≤ 200).
    - **Export** button with progress: "Downloading assets… 18 / 40". When done: the file
      count, a failed count ("12 of 40 assets couldn't be downloaded"), **Reveal in Finder**
      (`open -R <folder>/DESIGN.md`) and **Copy path**.
- [ ] **Step 4:** Run `cargo test -p sawit browser && cargo build -p sawit`. Expected: pass.
- [ ] **Step 5:** Commit `feat(design): assets, system tokens and export in the design panel`.

### Task 10: MCP tools

**Files:**
- Create: `crates/mcp/src/browser_design.rs` (`#[tool_router(router = browser_design_router)]`)
- Modify: `crates/mcp/src/lib.rs` (module, `+ Self::browser_design_router()`, tool names, INSTRUCTIONS)

**Interfaces — Consumes:** the Task 7 ops.
**Produces:**
- **`browser_inspect(ref?, selector?, format?, project?)`.** Description: "Design details of one
  element in the browser tab: box model, typography, colors with contrast, :hover/:focus
  states; format json (default), css, tailwind or html".
- **`browser_design(part?, project?)`.** Description: "The design system of the page in the
  browser tab: colors, fonts, assets, CSS variables and derived tokens (palette ramps, type
  scale, spacing, radius, shadows); part summary (default), colors, fonts, assets, tokens or
  variables".
- **`browser_export_design(include_videos?, project?)`.** Description: "Export the page's design
  system to .nyawit/design/<host>/: DESIGN.md, tokens.json (DTCG), theme.css (Tailwind v4),
  shadcn.css, components and assets. Waits for downloads (up to 10 minutes)".
  - It sends `ExportDesign`, then polls `ExportStatus` every 500 ms until `done` or 600 s.
  - It returns the folder, the file list and failed downloads (`url — reason`).
- The INSTRUCTIONS `browser_*` line gains "inspect an element's design, read the page's design
  system, export it as DESIGN.md + tokens + assets".

- [ ] **Step 1: Write failing tests.**
  - The tool-name list includes the three tools.
  - `inspect_needs_a_target`: no ref and no selector → "Give a ref (from browser_snapshot) or
    a CSS selector."
  - `format_is_checked`: `format="scss"` → "Unknown format “scss”. Use json, css, tailwind or html."
- [ ] **Step 2:** Run `cargo test -p sawit-mcp`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p sawit-mcp && cargo build -p sawit-mcp`. Expected: pass.
- [ ] **Step 5:** Commit `feat(mcp): browser_inspect, browser_design and browser_export_design`.

### Task 11: End-to-end check

**Files:**
- Create: `docs/fixtures/browser/design.html`, `docs/fixtures/browser/hero.png` (a 4×4 PNG), `docs/fixtures/browser/favicon.svg`, `docs/fixtures/browser/fonts/.gitignore` (`*`)
- `design.html` contains:
  - **Colors:** body `#ffffff`, text `#1d1d1f`, a muted section `#f5f5f7` with `#6e6e73` text,
    `hr`s and an input bordered `#d2d2d7`.
  - **Buttons:** 3 `.btn-primary` (bg `#0071e3`, white, radius 980px, padding 8px 16px,
    17px/400) and 1 `.btn-secondary` (transparent, 1px `#0071e3` border).
  - **Accent elements:** 5 links `#0066cc`, 2 badges `#bf4800`, 1 success text `#29a354`.
  - **Card:** radius 18px, shadow `rgba(0,0,0,.08) 0 2px 8px`, padding 24px, `h3` + `p`.
  - **Rules:** `@media (min-width: 734px)` and `(min-width: 1068px)`,
    `:root { --brand-blue: #0071e3; }`, and `.btn-primary:hover { background: #0077ed }`.
  - **Font:** `@font-face { font-family: "Fixture Sans"; src: url(fonts/fixture.ttf) }`, used by
    the body.
  - **Assets:** `<header><a href="/"><svg aria-label="Logo">…</svg></a></header>`,
    `<img src="hero.png" alt="Hero photo">`, a CSS background `url(hero.png)`, and
    `<link rel="icon" href="favicon.svg">`.

- [ ] **Step 1:** Run `cp "/System/Library/Fonts/Supplemental/Courier New.ttf" docs/fixtures/browser/fonts/fixture.ttf`
  (git-ignored). Launch the dev app with this repo as the project and serve the fixtures.
- [ ] **Step 2: Through `sawit-mcp`:**
  1. `browser_navigate localhost:8765/design.html`.
  2. `browser_design part=tokens`:
     - `semantic` has `primary #0071e3`, `background #ffffff` and `foreground #1d1d1f`.
     - `breakpoints == [734, 1068]`.
     - `variables` includes `--brand-blue`.
  3. `browser_design part=fonts`: "Fixture Sans" with one `src`.
  4. `browser_design part=assets`: includes the logo svg (`logo: true`), `hero.png` and the
     favicon.
  5. `browser_inspect selector=".btn-primary" format=tailwind` contains
     `bg-[#0071e3] rounded-full`.
  6. `browser_inspect selector=".btn-primary"` (json): states has `hover` with
     `background: #0077ed`, or the equivalent computed form.
  7. `browser_export_design`. The folder `.nyawit/design/localhost/` has every file from the
     spec's layout. `assets/fonts/` has the TTF and `assets.json` lists 0 failures.
  8. `npx @google/design.md lint .nyawit/design/localhost/DESIGN.md` reports no errors (skip
     when `npx` is missing; note it).
- [ ] **Step 3: apple.com smoke** (network):
  - `browser_navigate https://www.apple.com`, then `browser_export_design`.
  - DESIGN.md has a primary color, `assets.json` lists > 10 files and a `logo` entry, and
    `browser_design part=fonts` lists an SF Pro family with web-font sources (fetched
    cross-origin CSS).
  - Hover-inspect quickly: run `browser_hover` across 20 nav links in a loop, then
    `app_state`. The app is responsive and the panel shows the last link.
- [ ] **Step 4:** Take window screenshots of each Design segment on apple.com (memory recipe)
  and check the layout.
- [ ] **Step 5:** Commit the fixtures and fixes: `test(design): fixture, end-to-end and fixes`.
