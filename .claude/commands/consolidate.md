---
description: Review stash, worktrees, branches and uncommitted work; fix errors, keep MCP tools and the sawit CLI in full control of the app, commit, refresh README media, release and push main
argument-hint: "[optional note about what changed]"
---

# /consolidate

Bring everything into `main` in a releasable state and push it. Run each phase in order. Don't skip a phase
because it "looks fine", and don't push until every gate below passes. Extra context from the user: $ARGUMENTS

## Phase 1: Review (report, change nothing yet)

Run these and summarize what you find in a short table (item → state → planned action):

```bash
git status --short
git stash list --format='%gd %cr %s'
git worktree list --porcelain
git branch -vv --no-color
git branch -r --no-color
git fetch --prune origin && git log --oneline main..origin/main && git log --oneline origin/main..main
git config core.hooksPath     # must print .githooks (release hook)
```

For each **stash**, run `git stash show -p stash@{N} | head -200`. Decide whether it is already in `main`
(`git diff stash@{N}^ stash@{N}` vs the code now), still wanted, or obsolete.

For each **worktree** other than the main checkout, check `git -C <path> status --short` and
`git log main..<its HEAD>`. A worktree is safe to remove only when it has no uncommitted changes and
no commits missing from `main`. Worktrees under `/private/tmp/...` or the scratchpad are usually left
behind by earlier sessions.

For each **local branch** other than `main`, run `git log --oneline main..<branch>`: empty means it is
already merged.

**Ask the user before** applying or dropping a stash, removing a worktree, deleting a branch, or merging
a branch that has unmerged commits. Bundle these into one question with your recommendation per item.
Never `git stash drop`, `git worktree remove --force`, `git branch -D` or `git reset --hard` without that answer.
Merge what the user wants kept into `main` (resolve conflicts yourself, then rebuild).

If `origin/main` is ahead of `main`, `git pull --rebase origin main` before going on.

## Phase 2: Verify and fix

Run the same gates CI and the release need, in this order, and fix every failure before moving on:

```bash
cargo fmt --all -- --check          # on failure: cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

- For a failure, find the root cause (superpowers:systematic-debugging). Fix the code, not the check: no
  `#[allow]`, `#[ignore]` or deleted tests to make it green unless the user agrees.
- If `web/canvas` sources changed (`git status` shows `web/canvas/**`), rebuild the canvas bundle with
  `npm run build` in `web/canvas`, so `crates/app/canvas/*.br` matches its sources.
- Look at untracked files: anything that belongs to the product gets added. Build output, logs, `.env`,
  secrets, `*.local.*` and scratch files must not be committed. Add a `.gitignore` entry if one keeps
  showing up.
- Rerun all four gates after the last fix. Quote their final result (pass counts, `0 warnings`).

## Phase 3: MCP tools and the CLI must match the app (required)

An AI agent must be able to do through `sawit-mcp` everything a person can do in the app. **This phase is
not optional:** when the changes add, change or remove anything a user can see or do, update the MCP tools
in the same consolidate run, before committing. Only when nothing user-facing changed may you skip it, and
then you must say so in the report.

How the pieces fit:

- `crates/mcp/src/<area>.rs` holds the tools, one module per area, each with its own router merged in
  `Sawit::router` (`crates/mcp/src/lib.rs`). Tools that work without the app (instances, db, diagrams, ports,
  network, ssh, scripts, git, prompts, files) call `sawit-core` directly. After changing a store the app
  also shows, they call `app_control::notify_reload` with the right `Store`.
- `crates/mcp/src/app.rs` drives the **running app** over the control socket. It sends
  `sawit_core::app_control::Command` (`crates/core/src/app_control.rs`), which the app handles in
  `crates/app/src/workspace/control.rs`, and starts the app first when it isn't running.

Check every user-facing change from Phases 1–2 (`git diff $(git describe --tags --abbrev=0) --stat`, plus
the uncommitted diff) against this list, and fill each gap:

1. **A new or changed panel, page, tab, view or action in the app** →
   - add the variant to `Panel`/`Page` or a new `Command` in `app_control.rs` (serde `snake_case`);
   - handle it in `workspace/control.rs`, returning a useful reply (what was opened, ids, or a clear error);
   - add the MCP tool in `app.rs` that sends it.

   Opening a screen is not enough: the agent must be able to perform the action itself (start, stop,
   run, save, export, close, …) and read the result back.
2. **New data or operations in `sawit-core`** (a new store, admin or export function, tool, setting) →
   add a matching MCP tool in the right area module that works without the app. Writes that the app shows
   call `notify_reload`; add a `Store` variant if needed.
3. **Changed or removed behavior** → update or remove the matching tool, its parameter schema and its
   description. Never leave a tool calling a command the app no longer handles.
4. **Parity check**: every `Command` variant, `Panel` and `Page` must be reachable from at least one MCP tool:

   ```bash
   sed -n '/pub enum Command/,/^}/p;/pub enum Panel/,/^}/p;/pub enum Page/,/^}/p' crates/core/src/app_control.rs
   grep -n 'Command::\|Panel::\|Page::' crates/mcp/src/app.rs
   ```

   Also compare the sidebar tabs, pages and title bar buttons in `crates/app/src` with that list. Report
   anything still uncovered and why.
5. **Safety stays as it is**: `query` stays read-only, a start whose port is taken is still refused with an
   explanation, and destructive tools (drop, kill, delete, force) say so in their description and need an
   explicit argument such as `confirm: true`. Passwords and secrets never appear in a tool result.
6. **Tool descriptions** are written for an agent: what it does, when to use it, what it returns, and
   whether it needs the app running.
7. **Tests**: add or extend tests in the module (`#[cfg(test)]`, as the existing ones do) for every new tool
   and for each new `Command`'s JSON round-trip. Then rerun the Phase 2 gates.
8. **Proof against the real app**: build release, launch it with a scratch `SAWIT_HOME`, and start
   `target/release/sawit-mcp` with the same `SAWIT_HOME`. Speak MCP JSON-RPC over stdio to it:
   `initialize`, then `tools/list` (check the new tools are listed), then `tools/call` on each new or changed
   tool. Check its effect in the app with a screenshot (Phase 5 recipe) or `{"cmd":"state"}`. Quote the
   calls and replies in the report.

### The `sawit` CLI must match too (required)

The CLI is the person-at-a-terminal side of the same features, so it gets the same treatment in the same run.
Skip it only when nothing user-facing changed, and say so in the report.

How it fits together:

- `crates/core/src/cli.rs`: `parse_args` → `Invocation`, the `Usage:`/examples help text printed by
  `sawit --help`, opening paths in the running app over `sawit.sock`, and its tests.
- `crates/core/src/kebun.rs`: the subcommands that work without the app. These are `sawit <db> [table]
  ["sql"]`, `sawit list [instances|db|ports|connections]` (`ListWhat`) and `sawit bakar ports|instances`
  (`BakarWhat`). Reserved words are in `SUBCOMMANDS`. Output is in Indonesian with karhutla/plantation jokes,
  tables go to stdout and status and timing go to stderr.

For each user-facing change:

1. **New data or operations** (a new store or resource, export, admin action, tool) → make it reachable
   from the CLI. Use `sawit list <thing>` for listing, a `bakar` target for stopping or killing, or a new
   subcommand for actions such as export. Call the **same `sawit-core` functions as the MCP tool**; never
   copy the logic. Give every new word English and Indonesian aliases, as the existing ones have.
2. **New subcommand words** → add them to `SUBCOMMANDS` so they aren't taken as a database name. Keep the
   escape (`sawit ./<word>` opens a folder with that name) working.
3. **Changed or removed behavior** → update the parsing, help text and errors. An unknown word must fail
   with a helpful message, never be silently ignored.
4. **Safety rules match the app and MCP**: destructive commands ask first, accept `-y`, and refuse to run
   without a terminal unless given `-y`. `bakar` keeps skipping protected processes (`protected`,
   `burnable`). Passwords never print.
5. **Help**: `sawit --help` lists every subcommand, target and flag, with an example for each new one.
6. **Tests**: add parser tests in `cli.rs`/`kebun.rs` (the same style as `parse(&["list"])` and
   `BakarArgs::parse(...)`) for each new word, alias, flag and error case.
7. **Proof**: run the release binary with a scratch `SAWIT_HOME` (`target/release/sawit --help`,
   `sawit list <new>`, the new subcommand against demo data, the `-y`/no-terminal paths) and quote the output.
8. **Parity**: a capability that exists in the app should be reachable from MCP, and also from the CLI
   when it makes sense in a terminal. In the report, list any app or MCP capability with no CLI path and
   whether that is on purpose.

## Phase 4: Commit the work

Group the changes into a few logical commits (one feature or fix each) rather than one huge commit. The
subject of every commit becomes a changelog line through `scripts/release.sh`, so:

- Use Conventional Commits: `feat:`, `fix:`, `perf:`, `docs:`, `refactor:`, `chore:` (optionally with a scope:
  `feat(db): …`).
- Write the subject as a short, lowercase, user-facing sentence, like the existing CHANGELOG.md entries.
- Never write a `chore(release):` commit by hand, and leave the `version` in `Cargo.toml` and the
  `CHANGELOG.md` sections alone. The release step owns them, and `release.sh` refuses to run while those
  files have uncommitted changes.
- End each message with the attribution line from the session's system reminder.

## Phase 5: Update README.md, screenshots and the demo video

List what is new with `git log --oneline $(git describe --tags --abbrev=0)..HEAD` and
`git diff --stat $(git describe --tags --abbrev=0)..HEAD`. For each change a user would notice:

1. **Text**: update the matching README section, or add one. Keep README's style: short plain sentences,
   bullets, `<kbd>` shortcuts. Keep these in sync with the code:
   - the feature table under *Overview*;
   - the sidebar tab list under *Welcome screen* (compare with `Panel` in `crates/core/src/app_control.rs`);
   - *Keyboard shortcuts* (compare with the key bindings in `crates/app/src`);
   - *The `sawit` command* and *Databases, ports and instances from the terminal*: every subcommand,
     `list` target, `bakar` target, alias and flag from `sawit --help` after Phase 3, with an example each;
     the feature-table row for the psql-style CLI; and the CLI line in each feature section, like the one
     under *Listening ports*;
   - the *MCP for AI agents* section. Rebuild its tool table from the `tools/list` reply in Phase 3, grouped
     by area (app control, instances, databases, diagrams, ports, network, SSH, run commands, git, prompts,
     files), so every tool is listed. Say which tools need the app running. Update the `mcp.png` screenshot if
     that page changed;
   - the *Architecture* module list (compare with `crates/core/src/lib.rs`);
   - the `SAWIT_VERSION=` example in *Install* (set it to the version this push will release);
   - *Size & memory*, if Phase 2's release build changed the size (`ls -lh target/release/sawit`).
2. **Screenshots** (`docs/media/*.png`, 2560px wide, One Dark): retake a screenshot when its screen
   changed, and add one for a new screen. Recipe:
   - Build release, launch with demo data and its own home: `SAWIT_HOME=<scratchpad>/home target/release/sawit --foreground`.
     Homebrew PostgreSQL 16 is used for the demo instances.
   - Synthetic clicks don't reach the window. Drive the app over its control socket instead: send
     `sawit-control/1\n` + a JSON `Command` from `crates/core/src/app_control.rs` (for example
     `{"cmd":"show_panel","panel":"ports"}`, `{"cmd":"open_page","page":"tools"}`, `open_table`, `open_sql`,
     `open_diagram`), then close the write half and read the reply. With a long `SAWIT_HOME`, the socket
     falls back to `/tmp/sawit-$USER-<hash>.sock`. Send `{"cmd":"state"}` to find yours.
   - Capture the window: find its id with a Swift `CGWindowListCopyWindowInfo` script filtered by the PID
     (the large ≈1280×801 window), then `screencapture -x -o -l <id> docs/media/<name>.png`.
   - Look at every new screenshot with Read before using it. It must show realistic demo data and no
     personal paths, hostnames, tokens or real customer data.
   - Kill only your own instance, by its full command line (`/bin/kill`, since `kill` is aliased).
3. **Video**: if a new screen is worth showing, re-record `docs/media/demo.mp4` and regenerate
   `docs/media/demo.gif` from it (ffmpeg, palette-based, same width as now), and update the scene list in
   the caption under the GIF and its `alt` text. If no new screen is shown, leave both alone and say so.
4. Every image and link in README.md must resolve: `grep -o 'docs/media/[^")]*' README.md | xargs ls`.
   Delete files in `docs/media/` that nothing references any more.

Commit this as `docs: update README, screenshots and demo for <short summary>`.

## Phase 6: Release and push

1. Final check: `git status --short` is empty, `git stash list` holds only what the user chose to keep,
   `git worktree list` and `git branch` show only what the user chose to keep, and the Phase 2 gates
   passed on this exact HEAD.
2. Note the version you expect: the patch after the one in `Cargo.toml` (for example `0.0.5` → `0.0.6`).
3. Push: `git push origin main`.
   - With `core.hooksPath = .githooks`, the pre-push hook runs `scripts/release.sh`. It bumps the version in
     `Cargo.toml`/`Cargo.lock`, writes the new `CHANGELOG.md` section from the commit subjects, commits
     `chore(release): vX` and tags `vX`. A background job then pushes that commit and tag
     (log: `.git/sawit-release.log`). Don't run `release.sh` yourself too, or the version is bumped twice.
   - If `core.hooksPath` is not set: run `tag=$(scripts/release.sh)`, then
     `git push origin main && git push origin "$tag"`.
4. Wait for the release push to finish, then confirm:
   - `cat .git/sawit-release.log` has no errors;
   - `git log --oneline -3` shows `chore(release): vX` on top, and `git ls-remote --tags origin vX` lists the tag;
   - `git status -sb` shows `main` even with `origin/main`;
   - `head -20 CHANGELOG.md` shows the new section with sensible entries. If a line reads badly, the cause
     is its commit subject. Tell the user; don't rewrite pushed history.
   - `gh run list --workflow release.yml --limit 1` shows the release build started. Report its URL.

## Report

Finish with a short summary:

- what was found (stashes, worktrees, branches, uncommitted files) and what happened to each;
- what was fixed, and the final gate results;
- MCP: the tools added, changed or removed, the parity check result (every `Command`/`Panel`/`Page`
  covered, or what is still missing and why), and the live `tools/call` proof;
- CLI: the subcommands, targets and flags added or changed, any app/MCP capability left without a CLI
  path (and why), and the live output proof;
- the commits pushed, the new version and tag, and the release run URL;
- which README sections, screenshots and video changed, and anything left for the user.
