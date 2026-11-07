# Track `mcp` report (B1, B3, B9, B10, B11)

Worktree `/Volumes/bahrul/project/sawit-wt/mcp`, branch `wt/mcp`.

## B1: MCP audit, gaps, code mode — `9bbe59d`

- **Audit:** `docs/notes/2026-11-07-mcp-audit.md`: every tool (168) with its app feature and
  hints, the gaps, and the inconsistent names/arguments (kept; listed for a later rename with
  aliases).
- **Gaps closed:** `git_fetch`, `git_pull` (ff-only), `git_push` (new module `git_remote.rs`);
  `app_open_page`'s `page` argument now names `permissions` and `accounts`. GitHub PRs/issues
  are left to GitHub's MCP server (installed by the MCP page on Sawit's sign-in) — decision.
- **Code mode:** `search_api` + `run` (`crates/mcp/src/code_mode/`). Signatures generated from the
  router's tool definitions at run time. `run` = QuickJS via `rquickjs` (already linked),
  nested calls through `ToolRouter::call` with the request's context. Setting:
  `SAWIT_MCP_TOOLS=code` lists only the two tools (others still callable); default lists all.
- **Numbers:** tools/list 197,073 B (~49k tokens) → 2,307 B (~600 tokens). Boa vs QuickJS: Boa
  +6.7 MB binary and ~7× slower on the test script; QuickJS adds 0 MB to Sawit. Decision: QuickJS.
- **Tests:** `cargo test -p sawit-mcp` 204 + 1 + 12 pass (new: 11 code-mode tests incl. a real
  MCP client over a duplex pipe). clippy `-p sawit-mcp --tests` clean, fmt, line limit OK.
  e2e `code mode` (new spec `e2e/specs/code-mode.test.js`): pass (drives terminal_run →
  terminal_read → app_state in one `run` against the real app).
- **Decisions:** no renames; TypeScript annotations not stripped (no oxc/swc added; scripts are
  JS, the description says so); the MCP server's default `INSTRUCTIONS` are at 3,998/4,000 chars
  so code mode isn't mentioned there (the code-mode instructions replace them in code mode).
- **Not verified:** a real agent session (Claude Code) configured with `SAWIT_MCP_TOOLS=code`;
  only the in-process client and the e2e client. `clippy --workspace` not run for B1 (only the
  mcp crate changed; the app crate was built and its e2e passed).

## B3: agents reuse terminals — `4644706`

- `terminal_run` goes to the agent's own most recent idle terminal (per agent session id; ops
  without an identity count as one anonymous agent), with `cd` when `cwd` differs; never a
  typing/locked/pinned/busy terminal. Fallback `run()` now also skips locked/pinned tabs for agents.
- Runs are numbered (`(run N)` in the answer, `run` in `terminal_read`); `close_when_done`
  closes a tab opened for the command once done (seen busy then 0.6 s idle, or 3 s quiet for a
  too-quick command), keeping the output (last 64 runs, 5,000 lines each).
- New: `crates/app/src/terminal/panel/agent_runs.rs`. Skill text in `sawit-terminal`, README row
  and feature bullet.
- **Tests:** panel tests (reuse 13 commands → 1 tab, locked tab not reused, one-off closes and
  output stays) pass; `cargo test -p sawit --bin sawit terminal::` 101 pass; mcp 205 pass; core
  app_control pass; `clippy --workspace --tests` clean. e2e: `terminal reuse` (50 commands via
  code mode → before+1 tabs; one-off tab closes) pass; `terminal`, `terminal lock`,
  `terminal menus`, `code mode` pass.
- **Decisions:** reuse with `cd` rather than a new tab when the agent's own idle terminal is in
  another folder; the optional per-project cap was not built (reuse already keeps it to 1–2).
- **Not verified:** two different real agents (two sawit-mcp processes) at once in the real
  app — covered only by the unit logic (owner id match).

## B9: Upgrade button on New Project — `87d86380`

- Core: `stack_builder::version` (`CliVersions` in `new-project-cli.json`: chosen version, cached
  latest + check time, options diff), `stack_builder::cli_help` (parse `create --help`, offered
  flags/values, diff, hiding), `command_at` / `run_command_at`, `new_project::shell_line_at`,
  `create(..., version, ...)`. New `crates/core/src/https.rs`: Rust HTTPS GET (rustls + native
  roots, HTTP/1.1, chunked) for npm's registry (`/create-better-t-stack/latest`), checked at most
  once a day, silent offline.
- **Non-Rust choice:** the options diff runs the new release's `create --help` once, on Upgrade,
  through the user's package runner (npx -y / pnpm dlx / bunx) in the login shell: the CLI
  publishes its options only through itself. The version check is pure Rust HTTP.
- App: `new_project_page/upgrade.rs` — line under the subtitle: "Better-T-Stack v3.49.0",
  "Upgrade to vX.Y.Z" (outline xsmall, CircleArrowUp), "Release Notes" (ghost, ExternalLink);
  after upgrade "Using Better-T-Stack vX.Y.Z", "Go Back to v3.49.0" (Undo2), the New/Gone diff
  line, gone options hidden from the cards, Create Project refuses a stack using a gone option;
  error line in danger colour naming the version kept. Command shown/copied/run uses the version.
- MCP: `stack_options` → `cli` {in_use, pinned, latest, upgrade_available, options_diff}
  (now open-world hint); `stack_create` → `cli_version` (strict x.y.z, else refused; for one
  create); Command::NewProject `cli_version`. `sawit new --create` uses the chosen version.
- Test hooks: `SAWIT_STACK_CLI_LATEST`, `SAWIT_STACK_CLI_HELP`; e2e harness gained an `env`
  option and `click(..., {stoodIn: true})` to press a guarded button whose effect a hook stands in for.
- **Tests:** core stack_builder 17 (version 3, cli_help 4, upstream port 10), https 2, new_project,
  semai, app_control; mcp 205; app new_project 14; `clippy --workspace --tests` clean; e2e
  `new project` pass (offered → Upgrade → diff + hidden Mongoose → Go Back), `pages` pass.
- **Design review (better-interface, rendered screenshots, dark theme, wide and 640 px):**

| Severity | Location | Before | After | Status |
|---|---|---|---|---|
| HIGH | `stack_builder/cli_help.rs` diff | "New:" listed ~60 flags the CLI already had (`--open zed`, `--yolo`…), burying the one real change | New = release vs pinned release's own help; Gone = page's offers the release lacks | Fixed, re-shot |
| MEDIUM | `cli_help.rs` `summary` | Backticks shown literally in the UI | Plain "New: --frontend solid-start. Gone: --orm mongoose." | Fixed, re-shot |
| LOW | `upgrade.rs` Upgrade button | Tooltip on a button disabled while a create runs isn't reachable | Accepted: the button reads fully without it | Open |

  Verdict: Approve. Narrow width: the version row wraps under the subtitle, nothing clipped.
  Light theme: Not verified (e2e runs the default dark theme).
- **Not verified:** a real newer release on npm (latest is 3.49.0 today); the real `npx …
  create --help` path on Upgrade (only the hook); the error line's rendering.

## B10: prompt library save and sync — `f6c8beaf`

- **Deletes:** `FileState::DeletedHere` / `DeletedOnGitHub` in `github::plan_sync` (synced
  before, gone on one side, unchanged on the other; changed on the other → conflict).
  `github::commit_changes` deletes files in the same commit (tree entry with `sha: null`).
  Prompts: backup deletes on GitHub, restore deletes here (`SyncOutcome.deleted`). Shell files:
  upload deletes a file missing here; `apply_download_with` moves a file deleted on GitHub to its
  backup (Undo puts it back); app, CLI and MCP callers pass the deletes.
- **Secrets:** `prompt_sync::secret_lines` (shell_config::find_secrets on prompt bodies being
  sent). A backup with any holds back (`held`) unless `Options.allow_secrets`: app shows a
  danger banner listing the lines with **Back Up Anyway** (auto backup also waits); MCP
  `backup_prompts.allow_secrets`; CLI `-y`.
- **Fresh install:** the empty library offers **Restore from GitHub** when signed in and the
  repository exists (checked once on launch). **Auto sync on launch:** with Back Up
  Automatically on, a restore runs on launch (never over local changes), "Brought in N changes".
- **MCP:** `dry_run` on backup/restore; `list_prompts` ends with the sync line (repository,
  last sync UTC, auto on/off). Conflicts: kept as pick-a-side (no keep-both), per the backlog.
- **Tests:** github plan delete cases, prompt delete-here/-there/fresh-install round trips,
  core 68 (prompt_sync, github, shell_config, kebun) pass; mcp 205; app prompts/shell_config 7;
  `clippy --workspace --tests` clean. `prompt_sync.rs` split (`prompt_sync/apply.rs`) for the
  500-line limit.
- **Design review:** the new UI is two existing components (Banner with an action, a second
  Placeholder action). Not rendered: both states need a GitHub sign-in, which e2e can't do (only
  the user signs in). Source review: button labels verb-first Title Case like the panel's; banner
  names the lines and the way forward; danger tone matches "Not backed up". Verdict: Approve
  (source only). **Not verified:** the rendered banner, the empty-state Restore button, the
  launch pull, and every GitHub call (no sign-in available to the agent).
- e2e `github` (signed-out prompts and shell footers) and `pages` pass on the B10 build.

## B11: Sawit Skills v2 — `4b12bd49`

- `git merge backlog` first (5e3cdff1; README MCP table conflict resolved: backlog's Agent
  accounts row with `account_pool`, my Prompts row). Skills written against the 172 tools there.
- Pack: `sawit` (router: every skill, ground rules, code mode, CLI fallback), `sawit-databases`,
  `sawit-terminal` (now terminals/Run/ports/network/Local Explorer; SSH moved out),
  `sawit-browser-testing` (+tabs, `browser_seo`, export, mesurer), `sawit-canvas` (+starter
  system, `canvas_lint`, tabs, routes, settings, handoff), new `sawit-ssh`, `sawit-git`,
  `sawit-project`, `sawit-prompts`, `sawit-app`. `VERSION = 2`; the block routes to all ten
  (16 lines, the block test's cap); catalog pack lists ten skills; `install_sawit_skills`
  description names them.
- Map: `docs/notes/2026-11-07-skills-map.md` (every tool → skill, every `sawit` command → skill).
- Tests: new `the_skills_teach_every_tool_and_the_router_reaches_every_skill` (every listed
  tool named by a skill or in NOT_TAUGHT with a reason: render_mermaid, canvas_add_mermaid,
  app_maximize_terminal; router names every skill; descriptions 80–600 chars; front matter
  name); the name checker allows page/panel values git_graph, new_project, shell_config.
  Also code mode's overview now groups singular/plural areas together (`prompt`, `process`).
- **Later tools needing skill text** (the coverage test fails until they get it): canvas MCP
  (B4/B4.1/B4.2/B15 new canvas_* tools), browser captures (D5 `browser_screenshot` options,
  `browser_record_*`), any further Accounts tools (C1 beyond `account_pool`), browser favicons
  if a tool appears (B7).
- **Not verified:** a fresh agent finishing one task per family with only the skills (needs
  real agent sessions); the Skills page showing the v1→v2 update in the real app (covered by the
  core version tests only).
- Tests run: core skills + sawit_skills 58 pass; mcp 209 + 1 + 12 pass; clippy `-p sawit-mcp
  -p sawit-core --tests` clean except a pre-existing `needless_update` in
  `agent_accounts/wallet.rs:311` (accounts track, from backlog). e2e `skills`, `tools`, `code
  mode`, `terminal reuse` pass on the B11 build. The commit also carries `cargo fmt` of
  `new_project_page/render.rs` as merged from backlog (popup track's menu icon line).

## Commits in order

1. `9bbe59d4` B1 feat(mcp): audit, git fetch/pull/push, code mode
2. `4644706b` B3 feat(terminal): terminal reuse, one-off tabs close
3. `87d86380` B9 feat(new-project): Upgrade / Go Back
4. `f6c8beaf` B10 feat(prompts): deletes, secrets, Restore on a new Mac, pull on launch
5. `5e3cdff1` merge of backlog into wt/mcp (for B11)
6. `4b12bd49` B11 feat(skills): Sawit Skills v2
