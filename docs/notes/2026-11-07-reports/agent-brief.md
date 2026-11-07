# Agent brief: 2026-11-07 Sawit backlog, parallel run

The backlog is `docs/notes/2026-11-07-backlog.md` (in your worktree). Read the "Tasks at a glance"
table, the **Ground rules**, and every section for the items your track owns before you start.
The ground rules are binding (one commit per item, Rust first, no outside brand names, only the
user moves the keyboard, no feature lost, keep app and MCP in step, README MCP table line).

## Your workspace

- You own one git worktree under `/Volumes/bahrul/project/sawit-wt/<track>` on branch `wt/<track>`,
  branched from `backlog`. Work and commit ONLY there. Never touch `/Volumes/bahrul/project/sawit.dev`
  (the user's main checkout), the `backlog` worktree, or another track's worktree.
- Do not push, release, merge, rebase onto main, or delete branches. The coordinator merges your
  commits into `backlog` item by item.
- If your item depends on another track's item (noted in your task), run
  `git merge backlog` in your worktree when told it has landed (or check `git log backlog`) before
  starting it. Only merge `backlog`, nothing else.

## Builds (all build output stays on the bahrul disk)

Every cargo command:

```
source /Volumes/bahrul/.toolchains/env.sh
export CARGO_TARGET_DIR=/Volumes/bahrul/project/sawit-wt/target-<track>
export CARGO_BUILD_JOBS=3
```

Six agents build at once on an 8-core / 16 GB Mac: keep `CARGO_BUILD_JOBS=3`, build only the crates
you need (`cargo build -p sawit`, `cargo test -p sawit-core <filter>`), and don't run several cargo
commands at the same time. The first build of your target dir is slow; that's expected.

Run long commands with logs, e.g.
`cd <worktree> && (source ...; export ...; cargo build -p sawit) > /Volumes/bahrul/.toolchains/logs/<track>-build.log 2>&1`.
You may use plain Bash for builds and tests (the user's "Sawit terminals" preference is satisfied by
logs here; never `terminal_input` into a tab you didn't create — indices shift).

Before each commit: `cargo fmt`, `cargo clippy --workspace` (or at least the crates you touched,
no new warnings), and the tests of the crates you touched.

## End-to-end tests and the real app

- e2e: `cd <worktree> && SAWIT_E2E_BINARY=/Volumes/bahrul/project/sawit-wt/target-<track>/debug/sawit
  /Volumes/bahrul/.toolchains/e2e-locked.sh e2e/run.sh --no-build --filter '<regex>'`.
  The lock is machine-wide: e2e takes over the screen. Hold it only as long as needed; run focused
  filters, not the whole suite. First run in a worktree does `npm ci` in `e2e/` (fine).
- Known pre-existing e2e failures on main: covers ×2, skills "eleven packs". Don't chase them.
- To see your build: launch your own binary with a throwaway home (see `e2e/lib/app.js` for how the
  harness does it: env vars, `HOME`), inside the e2e lock, and screenshot it with `screencapture -l`
  or `screencapture -R`, or write a focused e2e spec that saves screenshots to its artifacts. Read
  the PNGs to judge the result. Quit what you launched.
- The `sawit` MCP tools (`app_*`, `canvas_*`, `browser_*`) talk to the USER's running Sawit, not your
  build. Use them for canvas design work (below) and for reading Sawit's own behaviour, never to
  type into the user's terminals.

## Design work: mandatory for every redesign or new UI

Any item that designs or redesigns UI (new panels, menus, bars, pages, buttons, dialogs) must:

1. Load the `better-interface` skill and the six domain skills it routes to (`better-accessibility`,
   `better-layout`, `better-writing`, `better-typography`, `better-colors`, `better-ui`) with the
   Skill tool. For the canvas, also read `canvas_design_guide` / the `sawit-canvas-design` skill.
2. Design on the Sawit canvas first when the item says so (B14, B15 panels, C1, B16's shared look):
   use the `canvas_*` tools with `file` set to a `.sawit` file inside your worktree, e.g.
   `<worktree>/docs/design/<item>.sawit`, so you don't overwrite the user's project canvas. Export a
   PNG (`canvas_export`) and look at it. Commit the `.sawit` file with the item.
3. Implement it in GPUI with the project's own tokens and components (`vendor/gpui-base`,
   `vendor/gpui-component`, theme colors). No new design system.
4. Run a `better-interface` review of the RENDERED result (screenshots of your build, light and dark
   if the app supports both, narrow window, empty / loading / error states), and fix every HIGH and
   MEDIUM finding before committing. Put the final review table (findings + verdict) in your report.
5. It must look good: aligned, consistent spacing, icons where the backlog asks, no empty space, no
   clipped text. Check it with your own eyes in screenshots, not just in code.

## No bugs

Test each item: unit tests for logic, an e2e spec (new or extended) for UI behaviour where practical,
and a manual look at the real build. Re-run the e2e specs near what you changed. If something can't
be verified, say so in the report rather than claiming it works.

## Report

Keep `/Volumes/bahrul/project/sawit-wt/reports/<track>.md` up to date as you go: per item, the
commit hash, what changed, tests run and results, design review verdict, what's not verified,
follow-ups. When you finish (or are blocked), your final message lists the commits in order.

Item A6 (native keyboard events) stays **blocked** (ground rule 5): nobody does it.
