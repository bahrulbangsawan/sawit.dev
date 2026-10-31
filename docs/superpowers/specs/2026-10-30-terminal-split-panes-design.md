# Terminal split panes and background sessions: design

Sawit's terminal dock gets herdr's two features we use, built in: **split
panes** with herdr's pane menu, and **sessions that keep running** when a
window closes or Sawit quits, so any Sawit window can pick them up again.
herdr is then removed from Sawit completely: no launcher button, no install
prompt, no Tools catalog entry.

Reference: https://herdr.dev and the three screenshots the user shared on
2026-10-30 (herdr's pane menu, and herdr's two-pane layout with a header on
each pane).

## Why

Today Sawit's dock runs herdr as a program inside one terminal tab to get
splits. That is a terminal inside a terminal: two sets of tabs, herdr's own
key prefix, its own mouse handling (hence the "Shift takes the mouse back"
workaround), and an external install. The user wants to stop using herdr and
keep the two things it gave them.

## What the user asked for

- Split panes, and the pane menu: **Rename pane, Split right, Split down,
  Zoom, Send right-clicks to pane, Close pane** (exactly herdr's menu).
- Background sessions: "when I close herdr and open on another window
  terminal, the same sessions are still running."
- Remove herdr from Sawit.

## Goals

1. A terminal tab holds one or more panes, split right or down, nested in
   any combination, each pane its own shell (or SSH session, or script).
2. The pane menu above, on right-click.
3. Closing a window or quitting Sawit does not end terminal processes. The
   next Sawit window shows them again with their scrollback, screen and
   split layout as they were.
4. No herdr anywhere in the product.

## Non-goals

- herdr's other features: the Spaces/Agents sidebar, agent status dots,
  "Needs you" badges, remote machines, the Files/Changes buttons.
- Surviving a reboot or logout. Background sessions live as long as the
  background process does (see Architecture).
- Panes outside the bottom dock (splitting center tabs, mixing editors and
  terminals in one split).
- Dragging panes between tabs, or tabs between windows.
- Saving named layouts or restoring a layout into new shells.
- `sawit-mcp` terminal tools working while the app is closed (possible later,
  since the service is separate; not in this version).

## Anti-goals (failure even if it works)

- Typing or output that feels slower than today's terminal.
- Losing a running process: a window close, app quit, app crash or app
  update must never kill a terminal the user did not close.
- A second, half-working terminal: every terminal in the dock (shells, SSH,
  Run panel scripts) goes through the same path.

## User-facing behaviour

### Panes

- **Splitting.** "Split right" puts a new pane to the right of the focused
  pane, "Split down" below it, each taking half of the focused pane's space.
  The new pane starts a shell in the focused pane's current folder (ask
  the shell's cwd; fall back to the tab's project folder, then `$HOME`) and
  takes focus. Splits nest freely (a binary tree).
- **Pane header** (screenshot 3): shown on every pane when the tab has more
  than one pane, hidden for a single pane so the dock keeps its height
  (*my call*). Left: an icon (terminal, server for SSH, play for a script),
  the pane title, the pane's folder shortened with `~`. Right: three ghost
  buttons, Split right, Zoom, Close.
- **Focus.** Clicking a pane focuses it. The focused pane's border uses the
  theme's focus/accent color; other panes get the plain border. The tab
  label shows the focused pane's title.
- **Resizing.** Dragging the divider between two panes resizes them. Panes
  never shrink below 20 columns or 4 rows. Double-clicking a divider evens
  the two sides (*my call*).
- **Zoom.** The pane fills the whole tab area; other panes keep running,
  hidden. The header shows that the pane is zoomed and its Zoom button reads
  as Unzoom. Splitting or focusing another pane unzooms first. Zoom is
  separate from the dock's existing Maximize Terminal (the dock filling the
  window); both can be on.
- **Rename pane.** Edits the title inline in the pane header (for a single
  pane, in the tab label). A renamed pane keeps its name when the program
  sets a new title. Renaming to empty brings back the program's title.
- **Send right-clicks to pane.** A checkmarked toggle, per pane, off by
  default. Off: right-click opens the pane menu. On: right-clicks go to the
  program (as today, when it reports the mouse) and the menu opens with
  Shift+right-click instead. Shift+right-click opens the menu in every pane,
  either way (*my call*: herdr's escape hatch is not documented; this keeps
  the menu reachable).
- **Close pane.** Ends the pane's process (as closing a tab does today). Its
  space goes to its sibling. Closing the last pane closes the tab.
- **When a process exits.** In a split, a shell pane whose process exits
  closes itself, like tmux (*my call*). A single-pane tab keeps today's
  "This process has exited" banner. SSH panes always keep the
  Disconnected/Reconnect banner, inside the pane, so Reconnect still works.
- **Keyboard** (in the Terminal key context; *my calls*, following
  iTerm/Ghostty):
  - `cmd-d` Split right, `cmd-shift-d` Split down.
  - `cmd-alt-left/right/up/down` focus the neighbouring pane.
  - `cmd-shift-enter` toggles Zoom when the tab has more than one pane;
    with one pane it keeps today's meaning (maximize the dock), by letting
    the action propagate to the workspace.
  - No new close shortcut; `exit` or the menu closes a pane.
- **Tab context menu** stays as it is (Move to Project, Remove from Group,
  Close). Close on a tab ends every pane in it.

### Background sessions

- **Closing a window** detaches its terminal tabs: their processes keep
  running, with their layout, titles, renames, zoom and scrollback.
- **Quitting Sawit** detaches every window's tabs the same way. There is no
  "end all terminals on quit" prompt (*my call*, matches herdr: "Close the
  app, they keep running").
- **Opening a window** attaches the detached tabs whose project is one of
  the window's projects, in their old order. Detached tabs with no project
  (started outside any project, SSH tabs) attach to the next window that
  opens. Only if nothing attaches does the dock start a fresh shell, as it
  does today.
- **A tab is shown in one window at a time.** The dock header gets a
  "Background" button (with a count) whenever there are tabs this window is
  not showing: detached ones and ones open in other Sawit windows. Its menu
  lists each tab (project, title, pane count, how long it has run) with
  "Show Here" (moves it to this window) and "End" (kills it), plus "End All
  Background Terminals".
- **Run panel and SSH panel** keep finding their terminals after a restart:
  a script still running in a background tab shows as running and Stop works;
  an SSH server shows its live session count. (*My call*: Run scripts persist
  like any terminal; a dev server started from Sawit keeps its port after
  Sawit quits, and the Ports page can stop it.)

### herdr removal

Everything below goes:

- The dock's herdr button, Install herdr button, `open_herdr`,
  `herdr_available`, `herdr_logo`, and the herdr wording in the empty state
  ("Start a shell in the project folder." only).
- `assets/logo/herdr.svg`, `HERDR_LOGO`, its line in
  `assets/tool_logos/SOURCES.md`, the Tools page logo mapping, and the herdr
  entry in the dev tools catalog (`crates/core/src/dev_tools.rs`).
- `scripts/install.sh`'s herdr prompt and `SAWIT_HERDR`.
- The "Terminal / herdr" tooltip (becomes "Terminal"), README mentions
  (feature table, install section, terminal section, design notes), the
  `main.rs` PATH comment, and the view's "(herdr, vim)" doc comment.
- `multiplexer_markers` stays, for tmux and Zellij, and keeps `HERDR_` too:
  Sawit may still be launched from inside a herdr someone has installed,
  and the shell must not think it is nested. Only its comment changes.
- The CHANGELOG says herdr support was replaced by built-in panes and
  background sessions. It is generated from commit messages on push
  (`scripts/release.sh`), so the commit messages carry this, not a hand
  edit.

## Architecture

### The terminal service

A background process, **the terminal service**, owns every PTY and its
`alacritty_terminal` emulator. App windows are its clients. This is the
tmux/herdr model, and it is the only way processes outlive the app: a PTY's
master side must stay open in some process.

- **Same binary.** The service is `sawit` started with a marker (an env var
  or argv flag, like the SSH askpass mode in `main.rs`), checked before GPUI
  starts. No second binary in the bundle.
- **Started on demand** by the first window that needs a terminal: spawned
  detached (`setsid`, stdio to a log file), with the app's login-shell PATH
  already adopted by `main.rs`, so shells find `brew`, `docker`, and so on.
- **Socket.** A Unix socket in a `0700` directory under Sawit's
  application-support folder; the service checks the peer's uid. One
  service per user.
- **Lifetime.** The service exits when it has no sessions and no clients for
  a short grace period (*my call*: 10 s). It never exits while a session is
  alive.
- **Versions.** The socket name carries a protocol version. A newer Sawit
  with a new protocol starts its own service. An older service still running
  shows up in the Background menu as one row, "Terminals from an older Sawit
  are still running", with "End Them" only: a newer app can't speak the old
  protocol to list or attach its tabs (*my call*). A compatible service is
  reused across app updates.
- **Crashes.** If the app crashes, the service is unaffected. If the
  service dies, its processes end with it (their PTYs close); the app shows
  each affected pane as exited, and starts a new service for new terminals.

### What lives where

- **Core (`sawit-core::terminal`):**
  - `Session` as today (PTY + emulator + snapshot), used inside the service.
  - A **layout model**, pure and UI-free: a tab is a tree of splits
    (`axis`, `ratio`, two children) with panes at the leaves. Operations:
    split a pane, close a pane (sibling takes its space), set a divider
    ratio with minimum sizes, find the neighbour in a direction, zoom. Unit
    tested on its own.
  - The **service**: listens on the socket; owns sessions, tabs (layout,
    per-pane name/rename/right-click toggle, focused pane, zoom, project
    group, Run job tag, SSH identity), and which client each tab is
    attached to.
  - The **client**: a `RemoteSession` handle with the surface
    `TerminalView` uses today (`write`, `resize`, `scroll`, selection,
    `cd`, `at_prompt`, `foreground_job`, `text_tail`, modes, events), plus
    tab operations (list, attach, detach, create, split, close, rename, …).
- **App (`crates/app/src/terminal`):**
  - `TerminalPanel` keeps the tab strip, groups and banners, but each tab
    is now a `TerminalTab` that renders its layout tree: pane headers,
    dividers, zoom, and one `TerminalView` per pane.
  - `TerminalView` draws the latest frame its `RemoteSession` holds instead
    of calling `Session::snapshot` while painting. Input handling stays,
    plus the pane menu and the right-click toggle.
  - Window open/close attaches and detaches tabs; the Background menu.

### Wire protocol (the builder's choice, within these limits)

- Length-prefixed binary messages over the socket. The builder picks the
  encoding (a compact serde format such as postcard or bincode, or a small
  hand-written one).
- **Output:** the service pushes frames, not the client polling. A frame
  carries only changed rows (plus cursor, modes, display offset, title,
  at-prompt / foreground-job state), coalesced to at most one per display
  refresh per pane. A full frame is sent on attach and resize.
- **Input:** keystrokes, paste, mouse, scroll, resize and selection are
  fire-and-forget messages; things that need an answer (`text_tail`,
  selection text for copy, tab listing) are request/response.
- Colors are resolved in the service with `Palette::default()`, as today.
  If themes ever change the palette, the client sends it on attach.
- **Performance bar:** keystroke-to-screen latency and CPU for a full-screen
  TUI redraw (e.g. Claude Code streaming, `htop`) no worse than today's
  in-process terminal, measured before the cutover.

### Existing callers

- `sawit-mcp` tools (`terminal_run`, `terminal_read`, `terminal_input`,
  `terminal_close`, `app_state`) keep their `tab` index meaning "that tab's
  focused pane". `app_state`'s terminal list gains each tab's panes (index,
  title, running job, focused) and an optional `pane` argument on
  read/input/close addresses one pane. `terminal_close` without `pane`
  closes the tab, as today.
- The Run panel's `start_job`/`stop_job`/`job_status`/`show_job` and the
  SSH panel's session count and focus work across all tabs of the window,
  including panes inside splits and tabs reattached from the background.
- New-project behaviour in `set_projects` (idle shells follow the first
  project) applies per pane.

## Constraints

- macOS is the platform that matters (the app is macOS-only in practice);
  Unix sockets and `setsid` are fine. Windows can stay in-process.
- Rust edition 2024, GPUI via `gpui-kit` 0.8.1 (see
  `docs/notes/gpui-kit-0.8.1-api.md`); `alacritty_terminal` stays pinned at
  0.26.0.
- Key bindings must keep the existing guarantees tested in
  `workspace.rs` and `terminal/view.rs` (no two workspace bindings share a
  keystroke; shell keys stay with the shell).

## Testing

- Layout model: unit tests for split, close, resize limits, neighbour
  search, zoom.
- Service: integration tests in core that start a service on a temp socket,
  open a tab running `sh`, type `echo`, read the output, detach the client,
  reconnect a new client, and find the same process and scrollback; ending
  a tab kills its process group; the service exits after the grace period
  with nothing left.
- App: GPUI tests that split with `cmd-d`/`cmd-shift-d`, move focus with
  `cmd-alt-arrows`, zoom with `cmd-shift-enter` (and that it still maximizes
  the dock with one pane), and open the pane menu with right-click and
  Shift+right-click.
- Manual check before cutover: quit Sawit with Claude Code running in a
  split, reopen, and find it mid-run in the same pane.

## Left to the builder

- The wire encoding and frame diffing details, within the limits above.
- How the service's log is rotated, and its exact grace period.
- Divider hit-area size and drag cursor; pane header height and spacing,
  matching the dock's existing 28 px tab bar style.
- How to split the work into plans (suggested: layout model + pane UI
  in-process first, then the service, then cutover and herdr removal).

## Calls I made (for review)

1. Pane headers only when a tab is split.
2. Shift+right-click always opens the pane menu.
3. A shell pane that exits inside a split closes itself.
4. `cmd-d`, `cmd-shift-d`, `cmd-alt-arrows`, and `cmd-shift-enter` for Zoom
   only in split tabs.
5. Quitting Sawit never asks; terminals keep running.
6. Run panel scripts and SSH sessions persist like shells (dev servers keep
   their ports after quit).
7. Detached tabs reattach by project; tabs without a project go to the next
   window that opens. A tab is in one window at a time.
8. Service exits 10 s after its last session and client are gone; an
   incompatible older service's terminals can only be ended, all at once.

## Implementation plans

1. `docs/superpowers/plans/2026-10-30-terminal-panes-1-splits.md`: split
   panes, pane menu, herdr removal (in-process; shippable alone).
2. `docs/superpowers/plans/2026-10-30-terminal-panes-2-service.md`: the
   terminal service and client (not yet used by the app).
3. `docs/superpowers/plans/2026-10-30-terminal-panes-3-background.md`: the
   app moves onto the service; background terminals and the Background menu.
