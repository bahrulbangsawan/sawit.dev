# Size and RAM, before and after B6 (2026-11-07)

Backlog item B6: a smaller build and app, less RAM, no feature lost. Measured on an 8-core M-series
Mac with 16 GB, macOS 27.2, while other agents were building (so CPU times are rough; sizes are
exact; memory runs are listed one by one).

## How it was measured

- **Builds:** `cargo build --release -p sawit` with `CARGO_BUILD_JOBS=3` and
  `CARGO_PROFILE_RELEASE_STRIP=false`, so `cargo bloat --release --crates` can read the symbols;
  the shipped size is that binary after `strip -x` (what `strip = true` does on macOS) and an
  ad-hoc signature.
- **Bundle:** what `scripts/bundle-macos.sh` makes (the binary, the `sawit-mcp` link, the icon,
  NOTICE and the Inter licence), summed file by file, and its `ditto` zip (the updater's download).
- **Memory:** `e2e/specs/footprint.test.js`, run on its own
  (`SAWIT_E2E_FOOTPRINT=1 SAWIT_E2E_BINARY=… e2e/run.sh --no-build --filter footprint`): Sawit in a
  throwaway home, its window filling the screen, steps driven over the control socket. Each step
  waits 8 s, then keeps the least of three `footprint` samples a second apart (the memory macOS
  charges Sawit: dirty plus compressed, not clean file pages). WebKit's processes are counted
  apart. `SAWIT_E2E_FOOTPRINT_STACKS=1` adds `malloc_history` stacks per step.
- **Before** is `backlog` at `816b6923` (the tip this branch merged, with every other item in); the
  B6 commits sit on top of it. The first "before" build (at `e5dcaea6`, where the track started) is
  in the size table too.
- **Pre-backlog (`e04558a`)**: not built. A third clean release build costs about 25 minutes and
  10 GB of target dir with three other agents building; the installed v0.0.9 release
  (`/Applications/Sawit.app`, older than `e04558a`) is given for context only.

## Size

"After B6" is `816b6923` with the B6 commits (like for like with "before"); "final" is the branch
after merging `backlog` again at `b76c83c1` (more canvas work landed, about +0.3 MB of its own).

| | v0.0.9 (context) | `e5dcaea6` (track start) | `816b6923` (before) | after B6 | change | final (`b76c83c1` + B6) |
|---|---:|---:|---:|---:|---:|---:|
| `sawit` binary, stripped | 83.41 MB | 93.01 MB | 93.13 MB | **92.14 MB** | **-0.98 MB** | 92.41 MB |
| `Sawit.app` | 83.90 MB | 93.50 MB | 93.62 MB | **92.64 MB** | -0.98 MB | 92.90 MB |
| zip (updater download) | | 39.97 MB | 40.01 MB | **39.74 MB** | -0.27 MB | 39.86 MB |
| `__text` (code) | | 60.70 MB | 60.77 MB | 60.72 MB | -0.04 MB | 60.93 MB |
| `__const` (embedded data) | | 20.20 MB | 20.23 MB | 19.20 MB | -1.03 MB | 19.21 MB |
| clean release build | | 23.5 min, 6.0 GB peak rustc RSS | 22.4 min | | | |

The code budget, change by change (cargo bloat and `size -m`):

| Change | `__text` | `__const` |
|---|---:|---:|
| opt-level "s" for cold paths (rmcp -836 KiB, std -717 KiB, tokio -410 KiB, serde_json -242 KiB, sawit_mcp +1.2 MiB as code inlined from those moves into it) | -1.3 MB | |
| Pele and mesurer deflated in the binary | | -1.0 MB |
| Sawit's allocator (`memory.rs`, below): every allocation goes through a function LLVM can't see into, about 2% more code across the crates | +1.4 MB | |

The allocator gives back 1.4 MB of the size saving for 130-170 MB less memory with the canvas
open; kept for that. Inlined, it cost 3.2 MB; out of line, 1.4 MB.

## RAM

Footprint in MB (Sawit's own process; the least of three samples), each step on top of the last.
Before: `816b6923`, two runs (the second stopped at the terminals step: an empty control reply
while tabs were opening, since tolerated by the spec). After: `816b6923` + B6 once, and the final
branch twice.

| Step | before | after B6 | final, run 1 | final, run 2 | change |
|---|---:|---:|---:|---:|---:|
| idle (one terminal) | 108 / 107 | 109 | 128 | 131 | see below |
| + one browser tab | 141 / 140 | 142 | 140 | 171 | 0 |
| + canvas open (empty) | 410 / 407 | **244** | 243 | 249 | **-165** |
| + a Mermaid diagram on it | 504 / 582 | **333** | 331 | 333 | **-170 to -250** |
| + 10 terminals | 479 | **345** | 343 | 344 | **-135** |
| two idle minutes later | 423 | **250** | 249 | 273 | **-150 to -175** |
| WebKit's processes, one tab (3) | 37-39 | 43 | 36-38 | 38 | 0 |

- **Canvas:** the 165 MB was freed frames kept dirty in the system allocator's large-block cache
  (`Malloc Large (empty)` in vmmap: 203 MB with an empty canvas). Sawit's allocator now maps blocks
  of 4 MB and more on their own and unmaps them when freed.
- **Two idle minutes later:** the hidden browser tab gives its web view up (before and after:
  -56 MB before), and now Pele's QuickJS engine is dropped too: -94 MB after, so the engine held
  about 40 MB with this diagram.
- **Correction:** the commit `perf(canvas): system font files are mapped, not read` puts the
  empty canvas's +290 MB on the fallback fonts. The vmmap and malloc_history runs above showed it
  was the allocator's cache instead (fixed by the allocator commit); the font mapping still keeps
  the fallback faces (150 MB here) out of Sawit's memory once text is laid out.
- **Fonts:** the Mermaid step reads one face of each fallback family; `malloc_history` shows 9
  font files, 150 MB, now mapped from `/System/Library/Fonts` (clean, shared) instead of read into
  the heap. How much of that the "before" column carried depends on when the canvas lays text out,
  hence its spread (504 / 582).
- **Idle:** Sawit's heap is the same (Malloc Small 28-32 MB in every run); the difference is one
  more 20 MB window drawable (IOSurface 60 MB instead of 40 MB), seen in most B6 runs and in no
  "before" run, but also absent from the `816b6923` + B6 run (109 MB). It is the window server's
  buffer pool for the window, not memory Sawit allocates; not explained further.
- **Ten terminals** (nine new shells) cost about 11 MB together, before and after: alacritty
  grows each grid only as lines arrive.
- **Syntax set:** loaded once instead of twice once a Markdown file with a code block is opened;
  not part of these steps (a few MB).


## What changed

| Change | Commit | Effect |
|---|---|---|
| Pele and mesurer bundles deflated in the binary, inflated when run | `perf(core): the Pele and mesurer bundles ship deflated` | -1.0 MB binary; no memory while unused (each was a fresh String already) |
| Pele's QuickJS engine dropped after two idle minutes | `perf(mermaid): Pele's engine gives its memory back` | the engine's heap goes back to the system; the next diagram reloads Pele (~60 ms) |
| One syntect syntax set for the editors and Markdown code blocks | `perf(editor): one syntax set` | a second copy of bat's syntax set no longer loads when a Markdown file has a code block |
| Large blocks mapped on their own, unmapped when freed | `perf(app): large blocks go back to the system when freed` | -165 MB with the canvas open; +1.4 MB code; a new 27 MB block costs ~0.75 ms of page faults |
| System font files mapped instead of read (vendored figcraft-text) | `perf(canvas): system font files are mapped, not read` | each fallback face (4-55 MB files, Apple Color Emoji 180 MB) is clean shared memory, not Sawit's |
| opt-level "s" for MCP, wasm validation, font download, TOML parser | `build: opt-level "s" for more cold paths` | -1.3 MB code |

## Kept as is, and why

- **`panic = "unwind"`.** `abort` would save a little, but the canvas render worker
  (`crates/app/src/board/worker`) and gpui-base's text code catch panics so one bad frame or edit
  doesn't take the app down. That is behaviour.
- **`lto = "fat"`, `codegen-units = 1`, `strip = true`, `opt-level = 3` globally.** Already the
  smallest settings that keep hot paths fast. opt-level "s" was tried on krilla, write-fonts,
  subsetter, figcraft-pdf and html5ever: under fat LTO they came out *bigger*, so they stay at 3.
- **Image decoders (exr, tiff, dds, hdr, pnm, qoi, ff).** gpui-pre turns on every `image` format
  so the app can show any image file; dropping them would lose that. Not ours to turn off.
- **Duplicate crate versions** (`cargo tree -d`): all come from upstream crates we pin, not from
  Sawit's own manifests: gpui-pre (usvg/resvg/tiny-skia 0.46/0.11, fontdb 0.23, core-graphics 0.24,
  objc2 0.5 through accesskit), FigCraft and craft-libs (usvg 0.48, fontdb 0.24, read-fonts
  0.39-0.44, skrifa 0.42-0.47, harfrust 0.12/0.13), rust-i18n (toml 0.8, toml_edit 0.22, base64
  0.23 at build time only). toml_edit 0.25 and syn 3 are proc-macro only, not in the binary.
  Lining them up means forking those crates.
- **FigCraft's wasmi (plugins, ~700 KB with wasmparser), ureq (font download) and krilla (PDF
  export)**: non-optional dependencies of figcraft-engine, and each is a feature.
- **Inter (1.8 MB of TTF in `crates/board/fonts`).** Registered as `FontBytes::Static`, so its
  pages are clean, shared and paged in on use. Deflating it would save ~0.9 MB of disk but put
  1.8 MB of dirty heap in every process that opens a canvas: worse for RAM.
- **Skills (184 KB), CHANGELOG (43 KB), icons (~190 KB).** Small; read rarely; not worth a
  decompression step.
- **Hidden browser tabs** already give their web view up after a while (`browser/suspend.rs`).
- **Terminal scrollback (10,000 lines).** alacritty grows the grid as lines arrive, so an idle
  terminal costs only its screen; a full one costs ~25 MB at 100 columns. Lowering the limit
  would lose history, a behaviour change.
- **Syntax set prewarm at launch** (`highlight::prewarm`): costs memory at idle but keeps the first
  file open fast; kept, now loaded once instead of twice.
- **The canvas's live frame buffers.** With the window filling a Retina screen, a frame is ~27 MB
  of BGRA, held by the renderer (retained pixmap), the converted image and GPUI's atlas (~80 MB):
  by design, for incremental redraws. Recycling the image's buffer would need GPUI to hand it
  back (`RenderImage` keeps it private); the allocator change removes the freed ones instead.
- **No C or JS was swapped for Rust here:** QuickJS (Pele, code mode) is already measured against
  Boa in `docs/notes/2026-11-07-mcp-audit.md`; SQLite, tree-sitter and the system frameworks
  have no smaller pure-Rust equal.

## Tests

- Unit: sawit-core's mermaid, mesurer and embedded tests (62, with the bundles inflating byte for
  byte and a diagram drawn again after the engine went idle), figcraft-text's 157 (one for the
  mapping), sawit-board's 830, the app's 916 (running on the new allocator, plus two of its own).
- e2e, on the final release build: `canvas`, `mesurer`, `browser`, `editor`, `terminal`, `tabs`,
  `screens`, `pages`, `code mode`, `live cursors`, `launch`, `frame code`, `capture`, `loading`,
  `copy output`, `device` and `find`: 74 passed, 5 skipped (headed only), 1 failed once:
  `capture: a full-page recording scrolls the page top to bottom` got an empty control reply; it
  passed twice on its own on the same build, and once on the `816b6923` build.
- Not run: the full e2e suite (the coordinator's), and a long session (hours) on the allocator.
