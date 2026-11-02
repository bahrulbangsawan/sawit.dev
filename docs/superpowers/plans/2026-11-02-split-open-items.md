# Open items after the 500-line split (2026-11-02)

Every hand-written source file is now 500 lines or fewer (`scripts/check-line-limit.sh`, run by CI and by
`cargo test`), split by responsibility into module folders with the public paths kept. Which files each feature
owned and what they became is in `2026-11-02-feature-map.md`. Each lane also looked for speed: the `perf(...)`
commits since 6935d18 carry their before and after numbers in their subjects and bodies (81 code changes, and
22fd986 for debug builds). Every code change was reviewed by a skeptic looking for a change anyone could see: six
were dropped, and eight were fixed after their review (four at the merge, four by their lanes).

What is left is below: calls that are yours, the speedups that were dropped, and what the lanes measured or noticed
but didn't change.

## Calls that are yours

- **Faster, but a visible change**:
  - MCP replies are pretty-printed: `app_state` is 5.3 KB against 2.8 KB compact, `canvas_get` of 60 layers
    13.2 KB against 8.3 KB (an agent's tokens).
  - Excel export uses default compression; fast compression is much quicker but makes bigger files.
  - The site's demo video (822 KB) downloads in full on every visit; playing it on scroll saves about 0.8 MB of the
    first load. Screenshots at about 800 px in a `srcset` would cut desktop image bytes 60-70%.
  - The site's plain `<a>` links skip the router (`<Link>` would use the client code the entry already ships).
  - `listening_jobs` could read each process's folder with `proc_pidinfo` instead of `lsof -a -d cwd` (about 17 ms
    a refresh). That also fixes a bug: launched from Finder (no `LANG`), `lsof` escapes non-ASCII folder names, so
    the Ports panel shows them wrong.
  - The canvas reads and parses a file when it opens, serializes it on autosave, and renders PNG and PDF exports in
    the click handler, all on the UI thread. Moving them off it needs a loading state, a snapshot sawit-board can
    send across threads, and an export that finishes later.
- **sawit-mcp's canvas ops when the app has frozen** (accepted with 8c9ae0b, which asks the app its version once
  per app). Only with the app's UI thread stuck for more than 40 s: an op the agent was told timed out still runs
  when the app recovers, an old app that froze is refused at once instead of after 40 s, and a paused app (debugger)
  given an op larger than the socket buffer holds the call instead of timing out. The code before had the same
  exposure for every op that names no file.

## Found while checking, as they were before the split

- **`canvas_load` of the backup `canvas_clear` just kept, with the app running**, reads the backup from before:
  the app writes the new `.bak` after it replies. Eight rounds of add, clear and load at once gave the previous
  backup (or, the first time, no file) in every round, in the baseline build and the final one alike. With the app
  closed the backup is there at once.
- **`stop_processes` and `sawit bakar ports` leave alone any job whose command line says `claude`**, as they leave a
  coding agent: a dev server started from a folder whose path contains the word (`/tmp/claude-501/…`) counts as
  one and is reported as protected. The same server started elsewhere is stopped.

## Dropped after review

The commits are not on `main`; what each would need to come back:

| Change | Gain given up | Why it was dropped |
|---|---|---|
| Read the local Postgres servers' Keychain passwords at once (core-cli, 05f410f) | `sawit list` 0.79 -> 0.39 s debug | Concurrent reads of a legacy Keychain item can ask the user twice and race its migration. |
| Work out who installed each Dev Tools copy at detection (ws-pages, 56c9e72) | 54 -> 1.7 µs a row | The trash button and the upgrade command would stay as they were at detection. |
| Look for the Config Files off the main thread (ws-keymap, 93349e2) | 1.23 -> 0.061 ms a look | The list could be stale on first show and after a trash. |
| Read the system trust store once per process (db-core, 7e0c4d7) | second TLS setting 590 ms -> <0.1 ms | Trust changes in Keychain Access would wait for a restart, and a failed read would stick. |
| One QuickJS per thread for `browser_evaluate` (browser-agent, 7336665) | 245 -> 30 µs release, 3.8 -> 0.32 ms debug | A kept runtime's compile stack budget is about 2 KB smaller than a fresh one's, so scripts nested about 580 deep switch form. Needs `JS_SetMaxStackSize` calibrated per build. |
| Read only the `mcpServers` key of each client config (mcp-page, d191de3) | 46 -> 16 ms CPU a detection | Under serde_json's `raw_value` feature a key-only read differs from a whole read for one crafted key. |

## Speedups not taken

### Debug builds

`target/debug/sawit` is the build in daily use here, and every test runs in debug. 22fd986 builds QuickJS, PNG and
deflate, SQLite and the terminal's parser at opt-level 3 there (rquickjs-core was measured too: no gain). Not
measured, because each rebuilds gpui or the whole FigCraft stack:

- taffy: about 55% of a debug frame on the Skills page is its `compute_layout`.
- lyon: one diagram edge takes 1.35 ms in debug against 83 µs in release (299 edges, 405 ms a frame).
- imbl, FigCraft's node map: the largest cost of a skeleton apply in debug.
- sawit-board itself: its board model runs 5-8x slower in debug than in release.

### Canvas

- FigCraft's `settle_routes` runs after every command (`engine/src/boards/board/derived.rs:48`): drawing a Mermaid
  diagram takes 0.25 s at 500 edges, 1.45 s at 1000 and 11.4 s at 2000 (release). The fix belongs in FigCraft, as
  does `check_finite_geometry`, which scans every node after every command.
- Every pointer event, hover on an empty canvas included, re-renders the whole view; it needs a "nothing changed"
  signal from the engine.
- The agent animation (every 250 ms for up to 30 s) re-renders the layers panel, the Design panel (0.9-7 ms debug)
  and the menus.
- Layer rows are rebuilt every frame during a drag or a scrub; the context bar rebuilds its model every frame while
  something is selected (about 240 µs debug).
- `skeleton::resolve` scans every node for each id, so update and delete are O(N x M); a backup-keeping save loads
  the old file in full.

### Terminal

- Caching each pane's view would skip about 0.74 ms per idle pane per frame (`TerminalView::write` callers must
  notify first).
- The dock and tab strip still re-render on every output frame (about 0.5 ms CPU debug); a cached strip needs an
  event when a pane's foreground job changes.

### Browser

- The Colors (23 ms), Assets (10.9 ms) and System (6.6 ms) tabs build every row each frame; a virtual list would fix
  them.
- `run_scan` deserializes the scan (up to 2 MB) and merges rules on the UI thread; export clones the whole scan.
- Each page message is parsed on the UI thread (about 100 µs per network message debug); `page_errors(usize::MAX)`
  clones up to 500 entries per console error.
- Saving an annotation's screenshot (base64 decode, write) runs inside the pick's update.

### Database

- Small file I/O runs on the UI thread on open, reload, save and connect (diagram layouts, `profiles.toml`, `.env`,
  D1 resolve, server registry).
- A diagram rebuild runs DBML, ERD, layout and saved layouts on the UI thread, and builds the DBML twice.
- Release still tessellates every edge each frame (about 25 ms for 300 foreign keys at fit); caching paths needs
  GPUI support.
- The DBML view is one element per line per render.

### MCP and agents

- `canvas_commands` loads the whole stored canvas (or has the app send about 120 KB) for a list that doesn't depend
  on the board.
- Requests end at EOF, and about one bench run in 20 stalls until the app drops it after 5 s (also before the
  split); a delimiter or length prefix on both sides would fix it.
- sawit-mcp builds its router and about 120 JSON schemas at every start (about 23 ms debug).
- The database tools resolve their target up to three times and load the whole schema to find one relation for a
  drop, truncate, refresh, import or backup.
- The MCP page re-renders and re-lays-out at display rate while any spinner runs (up to 90 s per sign-in check).

### Workspace and pages

- Syntect highlights files of 4 KB or less in full on the UI thread at every keystroke: 40-60 ms for C++,
  180-220 ms for a 4 KB file under load in release.
- Each window builds its own Ports panel and polls every 3 s, hidden or not (lsof twice and ps, about 50 ms); the
  Git store runs four git processes per project every 4 s whatever is in view, and piles up when status takes
  longer.
- The Files panel re-reads open folders every 3 s while hidden (up to 2,000 after Expand All).
- New Project lays out about 93 cards per keystroke; Tools and Skills re-render every row per installer output line.
- `KeymapStore::rows()` takes about 13 ms, mostly looking up canvas command titles per row (measured before aed6254,
  which builds the canvas keys table once).
- Recent folders are checked with `is_dir` every frame on the welcome screen; a network share could block it.

### Core

- `suggest_port` runs one lsof per taken port from 5432 up; a single listening-ports lsof would do.
- The PDF viewer re-opens and re-parses the file for each page.
- Dev-tool detection is dominated by starting the login shell (0.3-1.5 s).

### Site

- Start's module preloads have `fetchPriority="low"`, so the entry script loads after the poster.
- The root `notFoundComponent` can't be split by the router plugin; a manual lazy one saves about 0.3 KB gz but makes
  a 404 wait for an extra request. Splitting the changelog's loader was measured and reverted (an extra wait on
  navigation).
- Fonts could be self-hosted (React puts the preconnect after the stylesheets, where it does nothing).

## Tests and tooling

- `mermaid::tests::a_diagram_past_a_limit_is_refused_at_once` must finish in 2.5 s, and failed under load while
  QuickJS's C was built at opt-level 0 in debug. With 22fd986 its 2,001-edge render takes 37 ms instead of 164 ms
  (the whole test 111 ms instead of 353 ms on a quiet machine).
- Under heavy load (load average 60 and up), `download::tests::existing_install_is_reused_without_network` and
  `mcp_clients::tests::a_cli_finds_the_node_nvm_keeps_elsewhere` can hit their 10 s stubs.
- The first e2e test of a full run, `browser: loads the fixture site…`, failed in 2 of 3 full runs: right after
  the app starts, agent-desktop's window move and resize time out, then its `set-value` doesn't take, or its Return
  isn't sent because the window's focus can't be confirmed. Launched alone on an idle Mac it passed 5 of 5 times with
  the baseline build and 5 of 5 with the final one. Waiting for the window to take the accessibility calls before
  the first step would make the suite steadier.
- sawit-core's `new_project` tests leave `sawit-new-project-{home,names,remembered,target}-<pid>` folders in the
  temp dir on every run (their `scratch` helper never removes them).
- The app's debug test binary links with "`__eh_frame` section too large (max 16MB)" (as before the split).
- Some intra-doc links in the moved board ops and store docs, and `[super::d1_write]` in `db/sqlite/open.rs`, point
  at items `cargo doc` can't resolve.
- The release binary is 199 KB (0.25%) bigger than the baseline with 666702d: machine code from the speedups'
  caches, tables and threads (`__text` +149 KB and its unwind tables). No direct dependency is unused.
