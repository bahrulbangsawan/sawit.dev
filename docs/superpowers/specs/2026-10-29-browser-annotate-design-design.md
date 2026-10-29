# Browser preview: annotate and design extraction

The Browser tab (`docs/superpowers/specs/2026-10-29-browser-pane-design.md`, merged) grows into the place
where the user checks the web app Claude Code is building and hands visual feedback back, and where
they lift another site's design system into their project. Two features are added inside the tab:

- **Annotate:** a rebuild of Visual Annotator (MIT, `~/Documents/visual-annotator-main`). Point at
  elements, comment, screenshot, and send structured markdown to Claude.
- **Design:** a rebuild of the Woblo Chrome extension. Inspect any element, list the page's colors,
  fonts and assets, and export a design system (DESIGN.md, tokens, Tailwind/shadcn themes, assets)
  that an agent can build from.

Both live inside Sawit. Neither is a Chrome extension.

## Why

The user's loop: Claude Code edits a web app in Sawit's terminal → the user checks it in the Browser
tab → points at what's wrong → Claude fixes it. Today the last step means describing the problem
in words or switching to Chrome with extensions. Separately, when the user wants a look like
apple.com, they open the site, extract its design system with Woblo and hand the result to Claude.
Both should take one click inside Sawit.

**Success:** visual feedback reaches Claude without leaving Sawit, with selectors, styles, a
screenshot and the component's source file where the dev build exposes it. Opening any site and
clicking Export leaves a folder Claude can build a matching UI from.

## Goals, non-goals, anti-goals

**Goals**
- Annotate the page in the Browser tab and get the result to Claude in three ways: Copy, Send to
  terminal, and an MCP tool the agent pulls from.
- Inspect, Colors, Fonts, Assets and System (tokens + export) in a Design panel.
- One-click export to `.nyawit/design/<host>/`.
- Device sizes in the preview. The Ports panel opens in the Browser tab.
- Every feature is reachable by agents through `sawit-mcp`.
- Engine-specific code sits behind one seam so Servo can replace the system web view later.

**Non-goals (this version)**
- Chrome extension support; the Servo engine; Windows/Linux (Sawit is macOS-only).
- Browsing features: bookmarks, history UI, more than one browser tab per window.
- From Woblo: the screen-wide eyedropper, the saved library and folders, full page export,
  draggable ruler guides, SCSS/JSX output, the 30-device catalog, mobile user agents and touch
  emulation, asset thumbnails.
- From Visual Annotator: severity, the floating in-page toolbar, Shopify section detection.
- Iframes (scans and picking cover the main frame only). Annotations surviving an app restart.
- `sawit` CLI words for these features (the browser tab has none today; MCP is the control surface).

**Anti-goals: these would make it a failure even if it works**
- The overlay leaking into what it measures. Picker boxes, markers or labels show up in agent
  snapshots, design scans, exported screenshots or the page's own layout.
- Breaking the user's app. A hydration error, a layout shift or a stolen keypress caused by the
  injected scripts on a React/Next/Vite dev server.
- Exported tokens that don't match the site. A palette or type scale invented from defaults rather
  than read from the page. Everything in the export must trace back to something observed.
- Copying assets into the user's source tree silently. Exports go to `.nyawit/` (git-ignored);
  moving files into the app is the user's or the agent's explicit act.

## Decisions

| # | Decision | Why |
|---|---|---|
| D1 | The engine stays the system web view (wry 0.57, WKWebView). All `wry`/`objc2` use in the browser moves to `crates/app/src/browser/engine.rs` (`Engine`). Nothing else in the browser module touches `wry` or `objc2`. | Agreed with the user: real-browser rendering now, Servo later as a second `Engine` with the same methods. No trait until that second engine exists. |
| D2 | Page-side code is plain JS injected at document start (main frame only), as two modules spliced into the existing agent IIFE: `browser_overlay.js` and `browser_design.js`. They add ops to the agent's `OPS` table and post through its token-signed `post`. | Engine-agnostic; one injection, one token, one dispatcher. |
| D3 | All UI that isn't on the page is GPUI: a side panel (340 px, right) inside the Browser tab with two modes, Annotate and Design. Only one is open at a time. The page area shrinks to make room. | GPUI can't draw over the native web view. A panel beside it is native, themed, and can't be broken by page CSS. |
| D4 | Highlights, markers, the hover label and measurements are drawn in-page, in one closed shadow root on a `<sawit-overlay data-sawit-overlay>` host. The host is created on first use and appended to `<html>`, never `<body>`. It uses `pointer-events: none` except while picking. | Hover feedback has to sit on the page. Staying outside `<body>` avoids React hydration mismatches. A closed root keeps page CSS out. |
| D5 | The agent snapshot, the design scan and the picker skip `[data-sawit-overlay]`. Every screenshot (agent, annotation, export) hides the overlay first and restores it after. | Anti-goal 1. |
| D6 | Annotations are kept in `BrowserView` memory for the session, numbered per page URL. Screenshots go to `.nyawit/annotations/<yyyy-mm-dd>/<host>[-<port>]-<n>.png`, or Sawit's cache dir without a project. | Claude Code reads image files by path. `.nyawit/` is already git-ignored. |
| D7 | Send to terminal pastes the markdown into the terminal panel's active tab (bracketed paste, via `sawit_core::terminal::paste_bytes`). It doesn't press Enter, so the user can add a line before submitting. It also shows and focuses that terminal. | Claude Code runs there. Not pressing Enter keeps the user in control. |
| D8 | Exports go to `.nyawit/design/<host>/`. A re-export of the same host replaces that folder. Without a project they go to `~/Downloads/sawit-design/<host>/`. | Stable path for agents; regenerable from the site. |
| D9 | Stylesheets the page can't read (cross-origin, `cssRules` throws) are fetched by Sawit with `/usr/bin/curl` (`Referer` = page URL). Their text goes back into the page, which parses it with a constructable `CSSStyleSheet`. | @font-face sources, `:root` custom properties, breakpoints and `:hover`/`:focus` rules often live in CDN sheets. curl matches `update.rs`; no new dependency. |
| D10 | Assets are downloaded by Sawit with curl: `Referer` = page URL, Safari user agent, `--max-time 30`, `--max-filesize 50M`, 6 at a time, 300 MB total per export. `data:` URLs are decoded in Rust. `blob:` URLs aren't saved; they're listed as "Can't save blob: media (made by the page's scripts)". Inline SVG is saved from its markup. Icons listed in the web app manifest are found by fetching the manifest. Failures are listed with a reason, not fatal. | Hotlink-protected CDNs need the Referer. Caps keep a video-heavy site from filling the disk. |
| D11 | All token derivation and every export format is pure Rust in `sawit_core::site_design`, tested headless. The page only observes and reports. | Anti-goal 3: deterministic, testable rules about what counts as a token. |
| D12 | Device sizes set the web view's frame to the device size scaled to fit, and the page zoom to the same scale (`WebView::zoom`). So the CSS viewport equals the device size even in a small window. | A clipped frame would lay the page out at the wrong width. |

## User-facing behaviour

### Toolbar

`‹ › ⟳ [address] [device ▾] [Annotate] [Design] [↗] [</>]`

- **Device menu** (Lucide `Smartphone`): Responsive (fills the tab), iPhone 15 393×852, Pixel 8
  412×915, iPad mini 744×1133, iPad Pro 11″ 834×1194, Laptop 1280×800, Desktop 1440×900, then
  Rotate. Device mode centers the page on a neutral background, with a caption like `393 × 852 ·
  75%` when scaled. It is kept per tab, not per site.
- **Annotate** (Lucide `Pencil`) and **Design** (Lucide `Palette`): toggles that open the side
  panel in that mode. They show as active while open. Opening Annotate turns on pick mode.
- **↗** (Lucide `ExternalLink`): opens the current URL in the default browser.
- **</>** (Lucide `Code`): Web Inspector, unchanged.
- **⌘⇧.** toggles Annotate, whether the page or Sawit has the keyboard.

### Picking (shared by Annotate and Design → Inspect)

- Hovering outlines the element: a 2 px accent outline, margin and padding tinted the way
  DevTools does it. A label shows `tag#id.class  W×H`.
- **↑ / ↓** (or Alt+scroll) walks to the parent or back to the child. **Esc** leaves pick mode.
- A click selects the element. It doesn't reach the page: `click`, `mousedown`, `pointerdown` and
  `mouseup` are cancelled in the capture phase.
- In Design → Inspect, with an element selected, holding **Alt** while hovering another shows the
  distances between the two boxes.
- Leaving pick mode removes every listener the picker added.

### Annotate panel

- All annotations of the session, grouped by page URL with the current page's group first. Each
  row has its number (matching a marker on the page; numbers count per URL), the element label, a
  comment field, an intent (Fix · Change · Question · Copy; Fix by default), a camera toggle
  (include the screenshot, which is always taken at pick time; on by default), and **Show**
  (scrolls the element into view and flashes it) and **Delete** buttons.
- A new pick appends a row and focuses its comment field. The keyboard moves from the page to the
  panel.
- Footer: **Include console errors** (on by default; adds nothing when there are none), **Copy**
  (Lucide `Copy`), **Send to terminal** (Lucide `SquareTerminal`) and **Clear**. Copy and Send
  show a note in the footer for 3 seconds: "Copied" or "Sent to Terminal 2".
- Send to terminal only pastes into a tab whose program accepts bracketed paste: Claude Code,
  zsh, bash ≥ 5.1. It uses the active tab if it accepts one, else the highest-numbered tab that
  does. Otherwise it copies instead and says "No terminal is ready for a paste, so it was copied."
  This way pasted lines never run as shell commands.
- Markers are numbered dots on the page for the current URL's annotations. They follow their
  elements on scroll and resize, and are re-attached when the user comes back to a URL and the
  selector still matches.

### Annotation markdown

```markdown
# Visual feedback — <document title>
**URL:** <url> · **Viewport:** 393×852 (iPhone 15)

## 1. `button.btn-primary` — fix
- **Comment:** The button should use the brand blue, not green.
- **Selector:** `main > div > button.btn-primary`
- **Element:** `<button class="btn-primary">` "Submit order"
- **Source:** `src/components/Checkout.tsx:42` (React, <Checkout>)
- **Box:** 120×40 at (350, 680)
- **Styles:** display: flex; background-color: rgb(34, 197, 94); font-size: 14px
- **Screenshot:** /Users/me/app/.nyawit/annotations/2026-10-29/localhost-5173-1.png

## Console errors on <current url> (2)
- `+12.3s` Uncaught TypeError: Cannot read properties of undefined (reading 'id') — app.js:88
```

- With annotations on several URLs, each URL gets its own `# Visual feedback` header block, the
  current page first. Console errors are those of the current page since its last navigation,
  newest 20. Their time is seconds since that page started.

- **Rules:** a line is left out when its value is empty. **Text** is cut at 100 characters. The
  Copy intent adds an `html` fenced block of the outerHTML, cut at 20 000 characters with
  `<!-- cut -->`. **Styles** lists `display` plus the non-default values of position, overflow,
  z-index, opacity, background-color, color, font-size, font-weight, border-radius, gap, padding
  and margin, in that order.
- **Source (best effort, dev builds only):**
  - React: `_debugSource` on the element's fiber or its owners, else the nearest component name.
  - Vue: `__vueParentComponent.type.__file`.
  - Svelte: `__svelte_meta.loc`.
  - Otherwise: a `data-source`/`data-inspector-relative-path` attribute.
  - Absent → no line.

### Design panel

Five segments: **Inspect · Colors · Fonts · Assets · System**. The page is scanned when the panel
opens and again after each navigation. A spinner shows while scanning.

- **Inspect:** the hovered element live, pinned by a click. It shows:
  - Size and box model (margin / border / padding).
  - Typography: family (the face actually used), size, weight, line-height, letter-spacing,
    color, alignment, transform.
  - Background: color, gradient or image.
  - Border and radius, shadow, opacity, transition.
  - Layout: display, position, gap, flex/grid properties.
  - **States:** the declarations of rules matching the element with `:hover`, `:focus`,
    `:focus-visible` and `:active`.
  - **Contrast:** text color against the effective background (walks up through transparent
    backgrounds), with the ratio and AA/AAA badges for its size.

  Copy buttons: **CSS**, **Tailwind**, **HTML** (outerHTML with computed styles inlined, ≤ 200
  elements).
- **Colors:** every color in use, deduplicated and grouped by role (Text, Background, Border,
  SVG fill/stroke, Shadow, Gradient), sorted by use. A swatch with the hex and a usage count;
  clicking copies the hex. A row menu copies `rgb()`, `oklch()`, a CSS variable line, or the
  Tailwind class for its role (`text-[#0f7fff]`, `bg-[#0f7fff]`, `border-[#0f7fff]`).
- **Fonts:** each family in use with weights, sizes, line-heights and a sample line in that font
  (system fonts render in GPUI; web fonts show their name). The source is web font (files from
  @font-face) or system. Per family: copy the CSS `font-family` line, the Tailwind
  `font-[…]`, and **Save files**. Footer: "Fonts belong to their foundries. Check the license
  before shipping."
- **Assets:** Images, SVG, Icons, Video, Lottie, with counts. Each row shows a label, type,
  dimensions and where it is used. Clicking scrolls the page to the first use and flashes it.
  **Save** per row and **Save all**, with **Include videos** off by default; files go to the
  export folder's `assets/`. Labels come from alt, aria-label, title, or the file name. An image
  or SVG inside a link to `/` in the header, or with "logo" in its class, alt or file name, is
  labelled "logo". Icons: `link[rel~=icon]`, `apple-touch-icon`, `mask-icon`, manifest icons, and
  inline SVGs ≤ 48 px.
- **System:** the derived tokens:
  - Palette: primary, secondary, accent, neutral as 50–950 ramps, plus semantic colors.
  - Font families: sans, serif, mono, display.
  - Type scale xs–4xl, spacing scale, radius, shadows, breakpoints.
  - The site's own CSS custom properties (up to 200).
  - **Export** writes the folder and offers **Reveal in Finder** and **Copy path**.

### Export folder `.nyawit/design/<host>/`

```
DESIGN.md        Google Labs DESIGN.md (version: alpha): YAML tokens + prose
tokens.json      DTCG design tokens
theme.css        Tailwind v4 @theme
shadcn.css       shadcn/ui :root variables in oklch(), only the ones derived from the page; a comment
                 lists the ones not seen (e.g. destructive, chart-1…5) to keep from your theme
variables.css    the site's own custom properties, as found
screenshot.png   the page as shown at export (overlay hidden)
components/      button.html, button-secondary.html, input.html, card.html, nav.html, link.html
                 (one detected sample each, outerHTML with computed styles inlined)
assets/          images/ svg/ icons/ fonts/ video/ lottie/
assets.json      [{file, url, kind, bytes, width, height, label, used_on: [selectors], error}]
README.md        source URL, date, what each file is, and "Assets and fonts belong to their
                 owners; check licenses before shipping."
```

**DESIGN.md** follows the alpha spec (`github.com/google-labs-code/design.md`):

- **Front matter:**
  - `version: alpha`, `name: <Site> design system`, `description: Extracted from <url> on <date> by Sawit.`
  - `colors` (semantic names + ramp steps such as `primary-500`).
  - `typography` (`display`, `headline-lg`, `headline-md`, `title`, `body-lg`, `body-md`,
    `body-sm`, `label`, `caption`; each with `fontFamily`, `fontSize`, `fontWeight`,
    `lineHeight`, `letterSpacing` when not normal).
  - `rounded` (`sm`, `md`, `lg`, `xl`, `full`), `spacing` (`xs` … `3xl`).
  - `components` (`button-primary`, `button-secondary`, `input`, `card`, `nav-link`) using
    `{colors.*}`, `{rounded.*}` and `{typography.*}` references wherever a value equals a token.
- **Prose sections, in spec order:** Overview, Colors, Typography, Layout, Elevation & Depth,
  Shapes, Components, Do's and Don'ts. Each is written from templates over the observed data,
  with no model call. Examples:
  - "Primary `#0071e3` is used on 14 buttons and 31 links; reserve it for actions."
  - "Body text is 17px/1.47 SF Pro Text; nothing on the page is set below 12px."

  Sections with nothing observed are left out, as the spec allows.

### Derivation rules (`sawit_core::site_design`)

- **Colors:**
  - Parse `#rgb[a]`, `#rrggbb[aa]`, `rgb[a]()` (comma and space syntax), `hsl[a]()`, `oklch()`,
    `oklab()`, `color(srgb|display-p3 …)` and `transparent`. Normalise to sRGB hex (+ alpha),
    clamping out-of-gamut values.
  - Fully transparent values are dropped. Exact duplicates are merged with their counts summed.
  - Palette clustering merges colors within ΔE_OK < 0.02.
- **Roles:**
  - **neutral:** the most-used color with OKLCH chroma < 0.03.
  - **primary:** the most-used chromatic color (chroma ≥ 0.03) among button backgrounds.
    Fallback 1: the most-used chromatic link text color. Fallback 2: the most-used chromatic
    color overall.
  - **secondary:** the next chromatic color, by use, whose hue differs from primary by ≥ 30°.
  - **accent:** the next one after that, under the same rule.
  - Roles that find no color are omitted, never invented.
- **Semantic:**
  - `background`: the body's effective background. `foreground`: the most-used text color.
  - `muted` / `muted-foreground`: the second most-used neutral background / text color.
  - `border`: the most-used border color.
  - `primary-foreground`: the most-used text color on primary backgrounds, else white or black,
    whichever has more contrast.
  - `ring`: the focus outline color if seen, else primary.
- **Ramps 50–950:** keep the base hue. Target lightness by step:
  `[0.97, 0.93, 0.88, 0.80, 0.71, 0.62, 0.54, 0.46, 0.38, 0.30, 0.22]`. Chroma is base chroma ×
  `[0.10, 0.22, 0.40, 0.62, 0.85, 1.00, 0.95, 0.85, 0.72, 0.58, 0.45]`, reduced in 0.005 steps
  until in sRGB. The step whose target L is nearest the base color's L takes the base color
  exactly.
- **Type:**
  - Samples are (family used, size, weight, line-height, letter-spacing) per text-bearing
    element, weighted by text length.
  - `body-md` is the most-weighted sample among `p`, `li`, `dd`, `td` and `span` inside `main`
    (whole page if no `main`).
  - Headings `h1`–`h3` map to `display` / `headline-lg` / `headline-md`. `h4` maps to `title`.
    `button` and `label` map to `label`. `small` and `figcaption` map to `caption`.
  - The smallest distinct non-heading size above body becomes `body-lg`. The largest distinct
    size below body becomes `body-sm`. Both are the sizes nearest to body.
  - The xs–4xl scale is the distinct observed sizes, ranked around body (`base`): up to 2 below
    (`sm`, `xs`) and 5 above (`lg`, `xl`, `2xl`, `3xl`, `4xl`).
- **Spacing:** non-zero margin, padding and gap px values. The base unit is 4 if ≥ 60 % of the
  values are multiples of 4, else 8 by the same test, else the most common value ≤ 8. The scale is
  the 7 most-used values sorted, named `xs` … `3xl`.
- **Radius:** values ≥ 500px (pills such as Apple's 980px) or ≥ 50 % become `full`. The others are sorted by value and named
  `sm`, `md`, `lg`, `xl` from the 4 most-used. **Shadows:** the 3 most-used distinct
  `box-shadow` values, ordered by largest blur, become `sm`, `md`, `lg`.
- **Breakpoints:** distinct `min-width`/`max-width` px values in `@media` rules from readable and
  fetched sheets.
- **Tailwind mapping (Inspect → Tailwind):**
  - Exact matches to Tailwind v4 defaults use the named class: spacing on the 4px scale (`p-4`),
    font sizes, weights, radius, shadows, and the display, flex and position keywords.
  - Anything else uses an arbitrary value (`p-[13px]`, `text-[#0f7fff]`).

### Agent tools (`sawit-mcp`)

All take `project`, like the existing `browser_*` tools.

| Tool | Does |
|---|---|
| `browser_resize` | `device` (a preset name, case-insensitive, or `responsive`) or `width` + `height`. Returns the CSS viewport size and scale |
| `browser_annotations` | The user's annotations as markdown, plus screenshot paths. `clear` after reading. Says "No annotations yet" when empty |
| `browser_inspect` | Element by `ref`/`selector`: box, typography, colors, contrast, states. `format` `json` (default), `css`, `tailwind`, `html` |
| `browser_design` | `part` `summary` (default: counts + derived tokens), `colors`, `fonts`, `assets`, `tokens`, `variables`. Output capped at 60 KB with a note |
| `browser_export_design` | Writes the export folder. `include_videos` (default false). Returns the folder, the file list and failed downloads |

`app_state` → `browser` gains `viewport` `{width, height, device, scale}` and `panel`
(`annotate`/`design`/`null`). The `sawit-mcp` instructions line for `browser_*` mentions
annotations and design extraction.

### Ports panel

Clicking a port opens `http://localhost:<port>` in the Browser tab. ⌘-click opens the default
browser. Tooltip: "Listening on {addr}:{port}. Click to open in Sawit's browser; ⌘-click for your
default browser."

## Architecture

```
GPUI BrowserView (browser/mod.rs) ── toolbar.rs ── device menu, panel toggles
  │   ├─ annotate.rs  AnnotatePanel  ── sawit_core::annotations (model, markdown, file names)
  │   └─ design/      DesignPanel    ── sawit_core::site_design (colors, tokens, exports, assets)
  │
  └─ engine.rs  Engine (wry WKWebView: build, load, eval+callback, bounds, zoom, snapshot, devtools)
        │  init script = browser_agent.js ⟵ splice ⟵ browser_overlay.js + browser_design.js
        ▼
   page:  window.__sawitAgent.run(op) → OPS{…agent ops…, overlay, pick, markers, flash,
                                           style, scan, rules}
          post(token, kind, json) → console | network | url | picked | hovered | key
```

- `BrowserOp` gains `Resize`, `Annotations`, `Inspect`, `Design`, `ExportDesign` (all native:
  the app orchestrates page ops, fetches and files). The page-side ops are internal, not
  `BrowserOp` variants, so agents use the tool-level API.
- `PageEvent` gains `Picked(Value)`, `Hovered(Value)` (throttled to 10 per second) and
  `Key(String)` (`toggle-annotate`, `escape`). `Picked` carries the element's full annotation
  details (selector, label, text, box, key styles, source hint, outerHTML ≤ 20 000 characters),
  so a pick needs no second round trip. `Hovered` carries only selector, label and box.
- `BrowserView` emits `BrowserEvent::SendToTerminal(String)`. The workspace subscribes, shows the
  terminal panel, pastes into the active terminal tab and focuses it.
- Scan budget: ≤ 6 000 elements visited (visible ones first, in document order), answer ≤ 2 MB.
  Over budget: partial with `"truncated": true`, shown as "Large page: scanned the first 6 000
  elements."

## Errors

- **Export or save without network:** each failed file is listed in `assets.json` and in the
  panel ("12 of 40 assets couldn't be downloaded"). Tokens and DESIGN.md are still written.
- **A page with no readable styles (about:blank, PDF):** "Nothing to extract on this page."
- **Pick mode while the page is loading:** it turns on when the page's agent is ready. The toggle
  shows it's waiting.
- **Send to terminal with no tab accepting a paste:** copies instead and says so (see Annotate
  panel).
- **Export taking longer than the 30 s control timeout:** exports run in the background.
  `browser_export_design` starts one, or joins one already running, and polls its status. The
  panel shows "Downloading assets… 18 / 40".
- **Annotation screenshot of an element larger than the view:** captures the visible part and
  notes "(partly off-screen)".

## Testing

- **Pure Rust, TDD (`sawit-core`):**
  - Color parsing and conversion, ΔE, contrast, ramps.
  - Role and semantic assignment from scan fixtures.
  - Type, spacing, radius and shadow scales.
  - The Tailwind mapping.
  - Each exporter: DESIGN.md front-matter keys and section order, DTCG `$value`/`$type`, the
    Tailwind `@theme` names, shadcn variables.
  - The asset file naming and manifest. The annotation markdown. Device fit and zoom.
- **JS:**
  - `node --check` on each new file.
  - A Rust test asserts the splice: one IIFE, both modules present, the token replaced.
- **Fixtures:** `docs/fixtures/browser/design.html` (known colors, a local @font-face, an inline
  SVG logo, an `<img>`, a CSS background, a primary and secondary button, a card, an input, two
  media queries) and `docs/fixtures/browser/hydrate.html` (a React 19 production build hydrating
  `<body>`, to prove the overlay causes no hydration error).
- **End-to-end:** in the dev app through `sawit-mcp` over stdio, as the browser-pane plan's Task
  5. Window screenshots use the recipe in memory `visual-verify-sawit` (clicks don't work there;
  agents drive through MCP).

## Plans

| Plan | Delivers | File |
|---|---|---|
| 1 Foundation | Engine seam, module splice and overlay basics, side panel shell, device sizes + `browser_resize`, open externally, Ports → Browser, ⌘⇧., overlay hidden in screenshots | `plans/2026-10-29-browser-preview-1-foundation.md` |
| 2 Annotate | Annotation model + markdown, pick → panel flow, markers, screenshots, source hints, console errors, Copy / Send to terminal, `browser_annotations` | `plans/2026-10-29-browser-preview-2-annotate.md` |
| 3 Design | Page scan + CSS fetch, `site_design` derivations and exporters, Design panel (5 segments), asset downloads, export folder, `browser_inspect` / `browser_design` / `browser_export_design` | `plans/2026-10-29-browser-preview-3-design.md` |

## Risks

- **The native-canvas cutover deletes `wry`.** `plans/2026-10-29-native-canvas-8-cutover.md`
  removes `wry` from `crates/app` and requires an `rg "wry"` check to come back empty. The Browser
  tab (merged) and this work depend on `wry`. That plan has to keep `wry` and exclude
  `crates/app/src/browser/` from its check. Coordinate before either lands.
- **Uncommitted work in `main`** (`crates/core/src/nyawit.rs` untracked; `workspace.rs`,
  `control.rs`, `mcp/app.rs` modified) touches the same files. It has to be committed before
  Plan 1 starts.
- **Hydration and page interference.** The overlay is created lazily outside `<body>`. Picking
  cancels events only while active, and the agent's existing hooks are unchanged. The
  `hydrate.html` fixture guards this.
- **Heavy pages.** The scan budget caps work. Hovered events are throttled.
- **Licensing of extracted assets and fonts.** They are saved for reference under `.nyawit/`
  (git-ignored), with the license note in README.md and the Fonts footer.
