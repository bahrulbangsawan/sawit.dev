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
- **`app_open` of a canvas file** takes the keyboard when its tab only shows again (Q13). Canvas ops without a
  `project` now go to the agent's own project, as terminal and browser ops do, with Sawit open or closed: Q12 is
  settled.
- **Media.** README and the site still show the Excalidraw canvas until they are re-recorded. Seven unused site images
  (`site/public/media/{canvas,instances,mcp,network,port-conflict,prompts,tools}.jpg`) were left out of the repository.

## Releasing

- Push `main` from the main checkout, on `main`: the pre-push hook now refuses a push of main, and a release cut, from a
  checkout on another branch. `git push --dry-run` publishes nothing (it may cut the release locally, and the next
  real push of main sends it). Wait for the tag before removing a worktree you pushed from.
- Read `scripts/release.sh --dry-run` first. The notes list `feat`/`fix` subjects only; a few internal "reconcile"
  and garden-path subjects may need a hand edit (`/consolidate`, Phase 6 step 2).
- Tell users to restart their agent sessions after updating: an older `sawit-mcp` can't name split panes.

## Minor, still open

Small, rare or untested-wiring items. Most of the untested wiring has one root cause: no test can build a `Workspace`
(ruling P8-C8). A test constructor over scratch app paths would turn several hand checks into tests.

- **Keymap.** `keymap.json` as a link to a file that doesn't exist yet can't be saved since keymap.json is written like
  every other file (936832a). Keystroke search can't look up a plain ⎋ or ↩. README promises a page's clipboard keys
  after a click inside an iframe, which posts no press.
- **Files.** A key queued behind a failed ⌘Z or ⇧⌘Z acts on another change without a word (the Do half is fixed).
  Open Project Folder drops the picked folder if the asking project closed while the picker was open.
- **Browser.** Send to terminal can paste into a Run panel script's terminal at its prompt, and counts a program with
  bracketed paste on (nvim, tmux, a REPL) as an agent. `ssh host` typed in a shell tab isn't treated as an SSH session.
  A link followed to another page doesn't reset the "links to other apps" question; only a reload or a typed address
  does. `app_state.browser` describes the front project's browser.
- **Terminal.** A restart left waiting a long time takes the keyboard when it finally starts. `terminal_run` with
  `new_tab` from an agent in another project starts in the front project's folder. Stable tab and pane ids in the
  control protocol are a follow-up (pane ids are u64s that JSON doubles round). The Dock's Quit, logout and shutdown
  end terminals without asking (gpui doesn't implement `applicationShouldTerminate:`).
- **Canvas.** The refusal toast doesn't mention the unsaved copy; Save As then close makes a second copy; backup
  rotation isn't all-or-nothing; a Mermaid diagram of thousands of edges stalls the worker for seconds.
- **Release and install.** The installer's "run it from another terminal" guard relies on Sawit's PTY setting
  `TERM_PROGRAM=sawit`, which no test pins; its pgrep half is untested. A stale remote-tracking ref can still let a dry
  run publish (near impossible). `release_it`'s installer test leaves one temporary folder per run.
- **Tests.** Old test folders (`sawit-board-view-*`, `sawit-design-panel-*` and others) remain in the temporary
  folder from runs before the fix, and `mcp_clients::lock_file` adds empty lock files on every run.
- **MCP.** `git_stage` is still announced as additive, though `git add` replaces the index entry as `git_unstage` does.
