# Open items after the bug hunt (2026-11-01)

The hunt over everything new since v0.0.7 found and fixed its Critical and Important findings in eleven areas
(canvas core and UI, browser, keymap, terminal, catalog, files, MCP, release, live smoke, site), then a cross-area
review and a final round of smaller fixes. What is left is below: product calls that are yours, the release steps,
and minor items nobody has fixed yet. The hand checks to run before a release are in
`2026-10-29-native-canvas-hand-checks.md`; the program's rulings in `2026-10-29-native-canvas-rulings.md`.

## Calls that are yours

- **Licence.** The repository has no `LICENSE` file.
- **Updates are unsigned.** Restart to Update installs whatever the release asset holds.
- **CI has never built this code.** No workflow builds or tests on a push: the first build of the native-canvas tree
  is the v0.0.8 tag's release job. An update from 0.0.7 to 0.0.8 has never been run end to end, and macOS 13 and 14
  are untested.
- **The site's deploy.** `site.yml` skips its deploy, with a notice, until the repository has a
  `CLOUDFLARE_API_TOKEN` secret. Its job still runs the site's package scripts before the deploy step.
- **The app-wide browser** can't be reached, by agents or the user, once a folder is open (each project has its own).
- **Config Files' ⌘⌫** moves a file such as `~/.zshrc` to the Trash without asking, and the panel can't undo it.
- **An agent's canvas reads bring the canvas forward** (Q12's first half): every canvas op, `canvas_get` included,
  opens the canvas tab and makes it active, over a maximized terminal too. With no `project` that is now the canvas
  of the agent's own project, so a read also brings that project forward, where it used to leave the project in
  front. Which canvas the ops use is settled for a project opened at its repository's root: the agent's own
  project's, with Sawit open or closed. The cases where the two still differ are under Minor, still open.
- **`app_open` of a canvas file** takes the keyboard when its tab only shows again (Q13).

## Releasing

- Push `main` from the main checkout, on `main`: the pre-push hook now refuses a push of main, and a release cut, from a
  checkout on another branch. `git push --dry-run` publishes nothing (it may cut the release locally, and the next
  real push of main sends it). Wait for the tag before removing a worktree you pushed from.
- Read `scripts/release.sh --dry-run` first. The notes list `feat`/`fix` subjects only; a few internal "reconcile"
  and garden-path subjects may need a hand edit (`/consolidate`, Phase 6 step 2). Three fixes refine features new in
  this release and can fold into their lines: 649d809 and c54df37 into `sawit asks before running a statement that
  writes`, and 82b0de9 into the `sawit nyalakan` line. 649d809 describes a failure on CALL and DO that never shipped.
- Tell users to restart their agent sessions after updating: an older `sawit-mcp` can't name split panes.
- Tell them to quit and reopen Sawit once it is updated, too: a `sawit-mcp` of 0.0.8 refuses a canvas op that names
  a `file` while an older app runs, since that app would run the op on the project's canvas.

## Minor, still open

Small, rare or untested-wiring items. Most of the untested wiring has one root cause: no test can build a `Workspace`
(ruling P8-C8). A test constructor over scratch app paths would turn several hand checks into tests.

- **Keymap.** `keymap.json` as a link to a file that doesn't exist yet can't be saved since keymap.json is written like
  every other file (936832a). Keystroke search can't look up a plain ⎋ or ↩. README promises a page's clipboard keys
  after a click inside an iframe, which posts no press. `keymap_unbind` can't take out a "(keys turned off)" row of
  your own that turns off no default; the editor's remove button can.
- **Dev tools.** One run of `sawit cabut`, the Tools page or `uninstall_dev_tools` orders removals by `needs` only, so
  `sawit cabut node codex` can remove Node.js before an npm-installed Codex, whose `npm uninstall -g` then fails.
  "X needs it" names a tool installed another way too (Homebrew's Python "needs" uv), which can put off a safe removal.
- **Files.** A key queued behind a failed ⌘Z or ⇧⌘Z acts on another change without a word (the Do half is fixed).
  Open Project Folder drops the picked folder if the asking project closed while the picker was open.
- **Browser.** Send to terminal can paste into a Run panel script's terminal at its prompt, and counts a program with
  bracketed paste on (nvim, tmux, a REPL) as an agent. `ssh host` typed in a shell tab isn't treated as an SSH session.
  A link followed to another page doesn't reset the "links to other apps" question; only a reload or a typed address
  does. `app_state.browser` describes the front project's browser. The network log writes a FormData body's values as
  they are, so a secret field's value holding `&` is redacted only up to it.
- **Terminal.** A restart left waiting a long time takes the keyboard when it finally starts. Stable tab and pane ids
  in the control protocol are a follow-up (pane ids are u64s that JSON doubles round). The Dock's Quit, logout and
  shutdown end terminals without asking (gpui doesn't implement `applicationShouldTerminate:`). `terminal_close` asks
  for confirm only for a running program or the pane with the keyboard, so an idle shell you look at while you type in
  the editor closes without it (its scrollback goes). A `terminal_split` refused for its tab, pane or size still shows
  the dock, and a tab behind another is measured for a split with the size it last had.
- **Canvas.** The refusal toast doesn't mention the unsaved copy; Save As then close makes a second copy; backup
  rotation isn't all-or-nothing. (A diagram of thousands of edges no longer stalls the worker: Pele refuses one past
  2,000 edges at once, and the canvas renders a diagram off the UI thread.)
- **An agent's canvas, open or closed** (Q12's second half). With no `project`, Sawit closed can still pick another
  canvas than Sawit open in these cases: a project outside a git repository with the agent in a subfolder (no
  walk-up there), a Sawit project folder above nested repositories (several repositories in one folder, or a
  superproject's submodules), and a subfolder canvas an older `sawit-mcp` left, which still wins as the nearest. With
  Sawit open, an agent whose folder no window has open gets the front window's project's canvas. Passing `project`
  avoids all of them.
- **Release and install.** The installer's "run it from another terminal" guard relies on Sawit's PTY setting
  `TERM_PROGRAM=sawit`, which no test pins; its pgrep half is untested. A stale remote-tracking ref can still let a dry
  run publish (near impossible). `release_it`'s installer test leaves one temporary folder per run.
- **Tests.** Old test folders (`sawit-board-view-*`, `sawit-design-panel-*` and others) remain in the temporary
  folder from runs before the fix, and `mcp_clients::lock_file` adds empty lock files on every run. No test drives
  `terminal_close`'s refusal or an agent's new tab folder through `Workspace::control`, the CLI's routing of
  `sawit list keys`, `browser_export_design`'s refusal in `export_op`, or `sawit <db>`'s question before a write.
- **MCP.** `git_stage` is still announced as additive, though `git add` replaces the index entry as `git_unstage` does.
