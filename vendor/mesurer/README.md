# mesurer, as Sawit's browser runs it

[mesurer](https://github.com/ibelick/mesurer) 0.2.7, commit
`5c2fdaefde54591329d94b179af6ef5f32e15676`, Copyright (c) 2026 Julien Thibeaut, MIT License
([`LICENSE`](LICENSE)). It is the browser tab's Annotate tools on the page: inspect and measure,
distances, guides, rulers, layout guides, typography, X-ray, responsive previews, colour sampling,
arrows, pen, text, comments, screenshots and recordings.

Sawit uses mesurer **unmodified**. Nothing in its source is patched; what WebKit lacks, and what
Sawit supplies (saving, the clipboard, captures, colour sampling, comments for agents), lives in
Sawit's glue, `crates/core/src/browser/page/mesurer/`, as overrides that only mesurer's world sees
and as the props mesurer takes (`persistence`, `captureVisibleTab`, `theme`, `initialState`,
`features`).

## What is here

- `bundle/mesurer.js`: `build/entry.tsx` bundled with mesurer, React and React DOM (one minified
  IIFE, its stylesheet inside). It defines `globalThis.__sawitMesurerBundle` and runs in the
  `sawit-mesurer` content world. mesurer's banner heads it; React's licence comments end it.
- `bundle/gate.js`: `build/gate.ts`, mesurer's keyboard gate and history patch, which the Chrome
  extension runs in the page's own world; Sawit evaluates it there too.
- `bundle/SHA256SUMS`, `bundle/BUILD.json`: the files' sums, and the commit and tool versions
  they were built with.
- `LICENSE`: mesurer's, verbatim. `licenses/`: React, React DOM, scheduler and Tailwind CSS
  (whose generated styles mesurer carries), verbatim from the lockfile's packages.

The app embeds the bundle with `include_str!` (`crates/core/src/browser/mesurer/mod.rs`), so
`cargo build` needs neither Node nor pnpm.

## Rebuilding

```sh
vendor/mesurer/rebuild.sh            # clones the pinned commit into $TMPDIR and rebuilds bundle/
vendor/mesurer/rebuild.sh --check    # rebuilds elsewhere and compares with bundle/SHA256SUMS
MESURER_SRC=~/src/mesurer vendor/mesurer/rebuild.sh   # builds from a clone you have
```

It needs Node and pnpm. It verifies the clone is at the pinned commit with no changes, runs
`pnpm install --frozen-lockfile --ignore-scripts` (no lifecycle script is needed), mesurer's
`generate:styles`, then `build/build.mjs` (esbuild, the version mesurer's own build uses).

## Upgrading

1. Change the commit in `rebuild.sh` and `build/build.mjs`, and the version in `build/build.mjs`.
2. Run `rebuild.sh`; copy `LICENSE` and `licenses/` again if they changed.
3. Update `MESURER_VERSION` and `MESURER_COMMIT` in `crates/core/src/browser/mesurer/mod.rs`
   (its banner test fails until they match), `NOTICE` and the README's Credits.
4. Run the browser e2e spec.
