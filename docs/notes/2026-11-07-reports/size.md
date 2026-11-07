# Track: size (B6)

Worktree `/Volumes/bahrul/project/sawit-wt/size`, branch `wt/size` (started at `backlog` e5dcaea6;
`backlog` merged at 816b6923 and again at b76c83c1). The note is
`docs/notes/2026-11-07-size-and-ram.md`. Measurement binaries, bundles and cargo-bloat output:
`/Volumes/bahrul/project/sawit-wt/size-measure/`. Logs: `/Volumes/bahrul/.toolchains/logs/size-*`.

## Commits (in order)

1. c0db18e9 perf(core): the Pele and mesurer bundles ship deflated
2. 2e2848dc perf(mermaid): Pele's engine gives its memory back after two idle minutes
3. 8408b230 perf(editor): one syntax set for the editors and Markdown's code blocks (vendored gpui-component patch)
4. 51e4fd6d perf(canvas): system font files are mapped, not read (vendored figcraft-text)
5. b3bb85d9 build: opt-level "s" for more cold paths in release
6. e8cfeb7e Merge backlog (816b6923)
7. 05d56218 test(e2e): footprint spec measures memory per step, run on demand
8. 3c378675 perf(app): large blocks go back to the system when freed (global allocator)
9. 2d8184a0 Merge backlog (b76c83c1)
10. ce7a7607 docs: B6's size and RAM, before and after

## Results (816b6923 vs 816b6923 + B6)

Binary 93.13 -> 92.14 MB (-0.98), .app 93.62 -> 92.64, zip 40.01 -> 39.74. Footprint: canvas open
410 -> 244 MB, + Mermaid 504/582 -> 333, + 10 terminals 479 -> 345, two idle minutes later
423 -> 250. Idle 108 -> 109 (the final branch shows 128-131: one extra 20 MB window IOSurface, not
Sawit's heap; unexplained). Final branch (b76c83c1 + B6): 92.41 MB binary.

## Decisions / notes

- The fonts commit message attributes the canvas's +290 MB to fallback fonts; later vmmap showed it
  was the allocator's large-block cache (fixed in commit 8). Corrected in the note.
- The global allocator costs +1.4 MB of code (3.2 MB if inlined) for ~165 MB less RAM; kept.
- Pre-backlog e04558a not built (time and disk); v0.0.9 in /Applications quoted for context.
- Pre-existing, not B6: `line_limit` test fails on other tracks' files.

## Tests

Unit: core mermaid/mesurer/embedded 62, figcraft-text 157, board 830, app 916 — all pass. Clippy clean on
touched crates. e2e on final release build (canvas, mesurer, browser, editor, terminal, tabs, screens,
pages, code mode, live cursors, launch, frame code, capture, loading, copy output, device, find): 74 pass,
5 skipped (headed), 1 flaky fail (capture full-page recording: empty control reply), which passed 2/2
on rerun.

## Not verified

Full e2e suite; long sessions on the allocator; third-party crates that might free Rust-allocated
>=4 MB blocks with C free() (none found in Sawit's code; e2e found none).
