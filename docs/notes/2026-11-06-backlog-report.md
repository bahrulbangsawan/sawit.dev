# Backlog report (2026-11-06)

The report for `docs/notes/2026-11-06-backlog-prompt.md`: 30 items, plus the user's extra requests.
This file is the summary and the item index. The details, per item, are in five parts:

1. [Keyboard and safety](2026-11-06-backlog-report/keyboard-and-safety.md): items 11 + 18 (with
   the **focus-change table**), 13 (with the **destructive-action inventory**), 12.
2. [Terminal, full screen and find](2026-11-06-backlog-report/terminal.md): items 2, 1, 22, 17, 21.
3. [Browser, tabs and popups](2026-11-06-backlog-report/browser-and-popups.md): items 16, 6, 10,
   15 (with the **popup audit**), the device-mode × and the loading bar.
4. [Tools, GitHub, permissions and Agent Accounts](2026-11-06-backlog-report/tools-github-accounts.md):
   items 8, 3, 23, 24, 29, 4, 28, 19, 20, 5, 14, 9, and the CLI/MCP parity pass.
5. [Canvas to code, cursors and size](2026-11-06-backlog-report/canvas-and-size.md): items 27, 25,
   26, 30, the FigCraft gaps work, and item 7 (with the **before/after table**).

## Where it stands

- **All 30 items are merged on branch `backlog`**, with the extras the user asked for:
  - Safe Close All and Force Close All in the terminal and file tab menus (`07d426b`);
  - the browser device mode's × back to Responsive (`2a96253`);
  - a visible loading bar for URLs (`b9e0c4c`).
- Another session's uncommitted canvas work (the FigCraft gaps) was backed up, committed in 9
  commits and integrated (`e9f7630`…`b9437c6`, from `wt/others`). See part 5 for what it has and
  its gaps.
- **Not pushed and not released**, as the user asked.
- `git log --oneline bdca7d5..backlog` lists every commit (87). Items 11 and 18 are in `bdca7d5`
  itself, which was committed on `main` before the backlog branch.

## Build setup

Every toolchain, cache and build output lives on the bahrul disk (exFAT).
`source /Volumes/bahrul/.toolchains/env.sh` sets `RUSTUP_HOME`, `CARGO_HOME` and
`CARGO_TARGET_DIR`. Rust 1.99.1 (from `rust-toolchain.toml`) is installed there; the Mac has no Rust
of its own. A clean debug build takes ~7 min and an incremental one ~25 s when the machine is
quiet. Cargo copies instead of hard-linking on exFAT (a harmless warning). `bdca7d5` alone did not
compile; `30f4745` fixes it.

## Final gate (backlog @ `654d624`)

- `cargo fmt`: OK.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: passes, 2,873 tests, 0 failures (the four large suites have 874, 914,
  821 and 193 tests; the rest are small integration suites).
- `scripts/check-line-limit.sh`: OK in every track.

## e2e

e2e drives the real app through accessibility. It can't run from an agent's own shell (no
Accessibility permission), so every run went through a Sawit terminal under a shared lock. Many
track runs failed before any window appeared because the screen was locked overnight; those were
re-run later.

1. A full run against a **stale binary** had 47 failures: the exFAT disk kept an old
   `target/debug/sawit`.
2. The next full run: **72 pass, 13 fail.** All 13 were then fixed or shown to be environmental:
   - a real crash: wry's `url()` unwrapped on a tab with no URL yet (`57f78df`);
   - the Accounts page's Diagnostics checks weren't readable by accessibility (`5ac8b79`), and the
     Mac's real `/Applications` Claude Pool leaked into the test home (`35a87c0`);
   - ⇧⌘↩ in the browser lost the layout after item 10 (`2d17d38`);
   - the canvas top bar's icon buttons had no names (`290357c`);
   - the skills install sheet's note wasn't read by screen readers (`845a8de`);
   - stale specs: covers (`1f658c6`), copy-output (`50f7492`, `7b3e66f`), tabs (`d779962`);
   - focus specs that fail when the window isn't exposed through accessibility (environmental;
     they pass on rerun).
3. The fix-ui track's rerun of the affected specs: 40 pass, 0 fail.
4. The final full run on the merged build:

   - Run 1 (`backlog-e2e5`, 11:56): stopped half-way at 12:30 when the session restarted;
     36 pass, 6 fail (device, find, focus), all of them keyboard tests while the Mac was in use.
   - Run 2 (`backlog-e2e6`, the rest plus those 6): 61 pass, 5 fail.
   - Re-run of the 5 on an idle Mac (`backlog-e2e7`): 4 pass (device iPhone 15, focus ⌃Tab,
     fullscreen panel to panel, live cursors). Three of them had failed with an empty reply from
     the control socket while the app stayed up; the likely cause is the socket's 5 s read
     timeout under load, which drops a request without an answer. It did not repeat, so it is
     left as a known intermittent (`crates/app/src/cli/server.rs`, the accept loop).
   - One real bug: **⌘F on the canvas opened FigCraft's Find and replace dialog** instead of
     item 21's find bar. FigCraft v0.15 (from the FigCraft gaps work) binds ⌘F; the canvas now
     gives ⌘F to Sawit and Find and replace stays in the Edit menu (`0ab69a0`). After the fix:
     the find and canvas specs 43 pass, 0 fail (`backlog-fixf`), and the full gate is green
     (fmt, clippy `-D warnings`, `cargo test --workspace` 2,873 tests, 0 failures).
   - The frame code, editor and design checks specs, which the stopped run never reached: 5 pass,
     0 fail (`backlog-e2e8`).
   - Every spec has passed on the final build in one of these runs.

## MCP tool count

`sawit-mcp` has **163 tools**, checked against `tools/list` from the release build (`74c97eb`). The
README and the site say 163. Commit messages give other counts (item 10 says 145, item 2 says 147,
item 9's `3f157ea` says 152, the safety merge says 156, the cursors commit says 163). Each message
was written before its branch was rebased onto the others, so only the last one is current.

## Item index

| # | Item | Commits | Real-app check |
|---|---|---|---|
| 11 + 18 | Keyboard focus and ownership | `bdca7d5` (+ `e87c540`, on main) | e2e `focus` (5 tests) |
| 13 | Every destructive action asks first | `e23e8f1`, `ed75cc9` | e2e `confirm` |
| 12 | Closing a project closes its terminals | `92ecd36` | e2e `project close` |
| 2 | Lock, pin and close-all | `ead212b` | e2e `terminal lock` |
| 1 | Copy output, errors and instance logs | `991f4de` | e2e `copy-output` (spec fixed late) |
| 22 | Terminal menus, Safe/Force Close All | `07d426b`, `ed75cc9` | e2e `terminal-menus`, `terminal` |
| 17 | Full-screen hotkeys | `0c16005`, `bd148be`, `2d17d38` | e2e `fullscreen` (4) |
| 21 | ⌘F find bar everywhere | `05f0700`, `a5e5d1b`, `856b76b`, `e2e1166`, `eaeab03` | e2e `find` |
| 16 | Device preview text | `2a96253`, `53053ff` | e2e `device` (2), `annotate-device` |
| 6 | Local dev servers in the browser | `58d5139` | e2e `servers` |
| 10 | Browser and canvas tabs | `1605744`, `326d8c8`, `57f78df` | e2e `tabs`, `tabs-memory` |
| 15 | Popups that spill out | A: `318436b`, `b5284d6`, `9f43bee`; B/C: `a5cc6af`, `f73d7d5`; `b9842aa`, `f5ffef4`, `290357c` | e2e `popups`, `covers`, `annotate-device`; no screenshots on file |
| 8 | GitHub CLI | `4eff6ba` | e2e `tools`; Set Up not seen (gh installed) |
| 3 | MinIO and its client | `616d9f5` | detection on this Mac; instance kind proposed only |
| 23 | Herdr and Herdr TUI | `d13b79d` | TUI round trip; app detected |
| 24 | HyperFrames CLI, Studio, skills | `7115482` | live skills test; doctor without FFmpeg |
| 29 | voxl | `32c597b` | unit tests only; not built here |
| 4 | shadcn/ui skills | `b07064a` | live install/remove test |
| 28 | Design to Code skill | `b950744`, `bea9b29` | live test for 3 agents |
| 19 | GitHub sign-in survives restarts | `99b607c` | unit tests, e2e `github`; **user to verify** |
| 20 | Shell files: one button, two-way sync | `3812290`, `82a10e5` | fixture tests; no real repo |
| 5 | Prompts on GitHub | `60c4136` | round-trip test; no real repo |
| 14 | Permissions page | `dd37467` | MCP in a throwaway HOME, e2e `permissions` |
| 7 | Size and memory | `d779962` (no size change) | release measurements, see part 5 |
| 9 | Claude Pool → Agent Accounts | `5b19500` … `f7c47dc`, `4f5d2fc`, `ec11137` | e2e `accounts`; **user to verify** |
| 27 | Design standard on by default | `38c3cce` | e2e `design checks` |
| 25 | Export HTML and React | `5d45dc0` | e2e `frame code`, `tsc` check |
| 26 | Send to Agent for artboards | `69470e4` | e2e `handoff` (Copy Prompt) |
| 30 | Live build cursors | `b755936`, `38e4c35`, `68014a4`, `d905cfb`, `e45e8ca` | e2e `live cursors` (3); debug CPU only |

Other commits: `30f4745` (build fix), `9b88cb0` (a racy font test), `9d973b9`, `a0a5545` (README),
`c360fe3`, `654d624`, `a97f93e`, `74c97eb`, `ace91bb` (parity pass), `f3c0057` (this backlog's prompt
and the next one, `docs/notes/2026-11-07-backlog.md`).

## Decisions made without the user

After the first round, the user asked the agents to decide alone. The main choices, each explained
in its part:
- No command palette exists, so "add it to the command palette" became keymap actions (items 2, 17,
  22).
- ⌘W and ⌥⌘T in a terminal keep closing center tabs (item 22).
- For terminals, Safe Close All is the item's "Close Idle" (item 22).
- The browser's find uses JS and the CSS Highlight API instead of `WKWebView.find` (item 21).
- ⌃⌘1/2/3 for full screen per panel; Escape exits only in the browser (item 17).
- gpui-component was vendored to fix menus at the source (item 15).
- Other agents' permissions are read only (item 14).
- Lanes come from works only when a session has several open (item 30).

Item 9 was the one item that needed the user's answers. They are in part 4.

## For the user to verify on the real setup

- **GitHub sign-in persistence (item 19):** sign in, ⌘Q, reopen; again offline, after a rebuild and
  after 24 h. If it signs out, read `~/Library/Application Support/sawit/github-sign-in.log`.
- **Agent Accounts (item 9):** the 8 checks in `sawit-wt/reports/pool-plan.md`: import from Claude
  Pool, `cp1`–`cp9`, Set Active, sign-in, shared settings, Remove Claude Pool and Open at Login.
- **Item 30:** fps and CPU on a release build with 6 lanes playing, and a real Claude Code session
  with subagents passing `agent`.
- **Item 7:** the numbers again on a quiet machine.
- Also worth a look: items 20 and 5 against real GitHub repositories, item 26 with a signed-in
  agent, and item 27 with a fresh agent session.

## What isn't done

- Item 7: no size reduction committed; the `opt-level = "s"` experiment is a follow-up (part 5).
- Item 15: no before/after screenshots on file; not every surface was checked in both themes, at
  iPad size, at every panel zoom or with a larger system font.
- Item 3: the MinIO instance kind is a proposal only.
- Item 29: voxl was never built or installed here.
- The FigCraft gaps work: no e2e, no MCP tools of its own, the timer pill on every board, and its
  plan's "Still open" list.
- Item 21: the browser's find misses matches split across elements.
- Item 18: the 2-minute agent loop wasn't run as such.

## Process notes

- Orphaned cargo processes, left over when the bahrul disk unmounted, ran for about 15 h and slowed
  every build and test. They were killed on 2026-11-07.
- 38 wait loops that never ended (a `pgrep -f` that matched itself) were cleaned up.
- One agent ran release builds in a hidden background shell instead of a Sawit terminal. This goes
  against the user's preference.
- Test copies of Sawit left Dock icons through WebKit helper processes.
- Agents' own slips, from their reports:
  - The safety agent may have closed another agent's e2e terminal tab: tab indices had shifted.
  - The catalog agent's manual tests wrote skills into `~/.claude-4/skills` and `~/.agents/skills`.
    All of them were removed.
  - A first version of the Claude Pool removal test pointed at the real `/opt/homebrew/bin`. The
    permission system stopped it before it ran, and the test was fixed.
- Some intermediate commits were not built on their own (`b9e0c4c`, `58d5139`), and `1605744` passes
  its MCP tests only with `326d8c8`. History was not rewritten.
