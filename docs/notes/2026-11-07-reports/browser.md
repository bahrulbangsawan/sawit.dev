# Browser track report (B7, B8, B8.1, D1–D6)

Worktree `/Volumes/bahrul/project/sawit-wt/browser`, branch `wt/browser`. Merged `backlog` twice (once before D1 for B16's menu look, once before D6).

## Commits (in order)

| Item | Commit | Status |
|---|---|---|
| B7 favicons on browser tabs | b6a7ad7 | done |
| B8 SEO panel + `browser_seo` | ed4c405 | done |
| B8.1 agent-readiness checks, scoring, external score | e1a4daa1 | done |
| (merge backlog) | 79ee47b8 | |
| D1 device classes/resolutions, capture at chosen size | 4b67780f | done |
| D2 full-page screenshot (tall view, tiles, PDF) | bc86995f | done |
| D3 native video (AVAssetWriter), replaces JS muxer for Sawit's own recordings | fb72839a | done (mesurer's own button not moved, see below) |
| D4 full-page recording | 273cef21 | done |
| D5 MCP: `browser_screenshot` options, `browser_record_*` | 0ff25821 | done |
| (merge backlog) | 1ce9ac35 | tool count conflict resolved to 176 |
| D6 capture menu, recording timer, capture bar | ba789b17 | done |

## Tests run

- core: `web::` 26 tests (head parsing, favicon choice/decoding ico/svg/png/data:, loopback fetch with redirects, SEO scan of a loopback site, agent-readiness scan of a loopback site with an MCP server (SSE), OpenAPI, Markdown negotiation, scan types, external parsing and local refusal, AVIF size, check-table uniqueness); `browser::tests::capture` 9 (sizing, resolutions, even sizes, HEVC switch, tiles, file names, strip PNG writer); viewport messages.
- app: `browser::` + `menu` + `assets` (198 pass incl. B16's every-menu-item-has-an-icon and the assets test), recorder unit tests (modes, limits, scroll plan).
- mcp: 209 pass (hints lists, tool list, README/site tool count 176, screenshot/record argument tests).
- e2e (real app, my build): `seo` (2 tests: favicon on the tab, panel scan/tabs/open check/agents/scan type/narrow/empty), `capture` (5 tests: four device classes at exact pixel sizes incl. 1179×2556 for iPhone 15 at 3×, JPEG, element; full page at 2× with sticky header + 100vh hero + PDF; MP4 720p/MOV 1080p/GIF recordings that Quick Look opens, cap auto-stop; full-page recording; camera menu → screenshot → record → stop → bar, tab close asks). Final related run: see bottom.
- Live calibration (B8.1): kyzn.life 93/95 shared checks same status, bahrul.me 86/94; scores on shared checks 56 vs 54 and 94 vs 93.

## Decisions

- HTTP: `reqwest` 0.13 (already in the tree via postgresql_archive) with a preconfigured rustls (ring + the Mac's roots) in `crates/core/src/web`. HTML: html5ever 0.27 + markup5ever_rcdom (already in the tree via gpui-base). Image decoders already compiled; resvg moved from dev- to normal dependency of core; `png` 0.18 and `gif` 0.14 (both already in the tree) added as direct deps.
- `browser_seo` runs the scan in the MCP process (the app's control requests time out at 30 s); the app only hands over the tab's address and rendered HTML. With `url` no app is needed.
- DNS-AID via DNS-over-HTTPS (reqwest) instead of adding hickory-resolver; production hosts only. MCP endpoints probed with plain JSON-RPC (handles JSON and SSE answers) rather than adding rmcp's client to core.
- External score: the outside scanner's HTTP API from Rust, behind a per-tab panel switch (not persisted) and `external: true`; local/private addresses never sent; not named in UI, commits or MCP text (the URL is only a code constant).
- Scoring: required+recommended count; bonus checks count (score and max) only when they pass or warn; emerging left out; weights 20/30/40/10; payments count if any protocol passes; Essentials 80/20/+5; grades A+ 95, A 90, B 75, C 45, D 30.
- Captures: snapshot width = target pixels / WebKit's pixels per point (the device-scale override while a device is imitated), then resampled to the exact size (WebKit rounds by a few px). Full page: the view is made as tall as the page (sticky/fixed headers once, no seams) with vh held via WebKit's viewport-units override, which Sawit now keeps set to the page's viewport at every layout (clearing it throws). Tiles above 16,000 px stitched in strips. Page cap 30,000 CSS px.
- Recording: sequential snapshots (afterScreenUpdates false) at the chosen size, timestamped; AVAssetWriter + pixel buffer adaptor on its own thread; MP4 H.264 default, MOV HEVC, GIF in Rust. Recording task outlives the tab so the file is always finished.
- Capture bar is in the page's flow under the toolbar, not floating: the web view draws above GPUI, a floating card would be hidden. It uses B16's button sizing/icons (menu_icons); B16's `board::float_bar` is canvas-scoped and positioned over canvas frames, so not reused.
- No keyboard shortcut bound for captures (⇧⌘5 etc. clash with macOS; left to B5).
- App is dark-theme only (Zed One Dark): design reviews cover dark only.

## Overlaps / notes for the coordinator

- pagejs track (A2): I did not touch `page/mesurer/capture.js` / `files.js`. mesurer's own recording button (Annotate's C V) still uses its JS muxer + `repair_mp4`; moving it to the native recorder is a follow-up (needs a small glue change in mesurer's JS bridge).
- canvas track: `DEVICES` grew from 6 to 20 (the board's Preview size menu lists them all); I changed one board test to find "Desktop" by name.
- popup track: device menu got class headers and scrolls (max 600 px); capture menu built on the shared menu look.
- Tool count: 176 after merging backlog (README ×2, site binary.tsx and trio.tsx).
- Fixes folded into later commits: D1 commit fixes the B8.1 SEO tab test; D3 commit fixes D1 sizes on devices (pixels per point + exact resample), D2's vh-clear crash, and the SEO panel scanning the asked address when a page fails to load.

## Design review (better-interface; rendered build, dark theme)

Scope: SEO panel (Overview, Previews, Checks with an open check, Agents, scan type, loading, empty, error-on-load, narrow 900×640 window) and the capture UI (camera menu, capture bar after screenshot and after video, recording state with tab timer, close-while-recording confirm). Screens: `reports/shots/browser/*.png`, `reports/shots/capture/*.png`.

| Sev | Where | Finding | Fixed |
|---|---|---|---|
| HIGH | toolbar SEO switch (assets.rs) | SearchCheck icon not embedded: icon-only button drew nothing | yes (embedded SearchCheck, CircleMinus, CircleDashed, ImageOff; later Ratio, Video, Square) |
| MEDIUM | seo/flow.rs | a page that failed to load showed "No page to scan" instead of why the scan failed | yes (scans the address asked for) |
| MEDIUM | design (canvas) | WhatsApp preview text ran together in the mock | fixed in the design and built as separate lines |
| MEDIUM | e2e | check rows below the fold can't be pressed by the a11y click (off screen) | test opens a visible check; rows themselves are Buttons with names "Name: status" |
| LOW | capture/record.rs | video files are named with a scale like `2_7x` for responsive tabs; a resolution label would read better | not fixed |
| LOW | seo/agents.rs | the external-score switch label "Ask" is terse (tooltip not supported on Switch) | not fixed; explanatory text sits under it |
| LOW | seo previews | previews use theme colours, not each platform's own (approximations, labelled by platform) | by design |

Checked and fine: every icon button has an accessible name; status shown by icon shape + colour + points (never colour alone); truncated finding/value text has a tooltip or opens in the expanded row; tabular numbers for scores; empty/loading/error states keep the panel frame; narrow window keeps the panel whole; menus follow B16 (icons on every item, section headers). Not verified: light theme (app has none), VoiceOver reading order, 200% zoom.
Verdict: Approve (no HIGH/MEDIUM left).

## Not verified

- Captures on a real 1× external screen (no such screen here); logic sizes by WebKit's pixels per point, the e2e checks exact sizes on the Retina screen only.
- Full-page shots of long real sites (apple.com etc.) and recordings of real sites through MCP: MCP tools talk to the user's running Sawit, not my build; verified via the app's control ops in e2e on fixture pages.
- 60-second 1080p recording's effect on UI smoothness; ScreenCaptureKit 60 fps mode (optional) not built.
- Full-page capture of a scrollable element (D2 "also") not built; D6 Element/Region from the menu not built (agents have ref/selector/clip/region via MCP; Annotate's region capture still exists).
- External score live call (only parsed from a saved answer + local refusal tested).

## Final related e2e run (after D6, filter browser|device|annotate|mesurer|loading|seo|capture|tabs)

51 pass, 2 fail in the batch run: "browser: the panel control op opens Design on a tab of its own" and "mesurer: Annotate mounts it…", both with the control socket closing without a reply. Rerun alone right after, both pass (43 pass, 0 fail with the rest of the suite skipped): flaky under load, not reproduced. All seo (2) and capture (5) tests pass in the batch.
