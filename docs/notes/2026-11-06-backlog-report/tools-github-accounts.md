# Backlog report, part 4: tools, skills, GitHub, permissions and Agent Accounts (items 8, 3, 23, 24, 29, 4, 28, 19, 20, 5, 14, 9)

Back to the [summary](../2026-11-06-backlog-report.md).

---

## Shared catalog machinery

Item 8's commit (`4eff6ba`) also carries what the later catalog items reuse: Tool fields
`formula`, `signature`, `version_arg`, `apple_silicon`, `long`, `keeps`, `warning`;
`Latest::Manifest` and `Latest::Crate`; dated versions; `upgrade_for`; Delete Settings…; and
asking before it pulls in prerequisites. No new crates; the logos add about 10 KB.

Every catalog commit had `cargo fmt`, `cargo clippy --workspace --tests`, `cargo test -p
sawit-core` and the line limit. Live skill tests (`cargo test -p sawit-core live_ -- --ignored`, in a
throwaway HOME) pass. e2e `tools: …` (2/2) passes; `9d973b9` checks the new Dev Tools rows by their
buttons.

## Item 8: GitHub CLI (`4eff6ba`)

- **Entry:** `brew install gh`, upgraded and removed with brew, latest from `cli/cli`, version from
  `gh --version`. A copy outside Homebrew's Cellar (the .pkg, MacPorts, conda) shows "Installed
  outside Homebrew" and is never upgraded or removed.
- **Sign-in:** the row shows `gh auth status` and **Sign In with GitHub CLI**, which runs `gh auth
  login` in a Sawit terminal. Sawit's token is never copied into gh.
- **Setup:** the welcome screen gets a Set Up section only when gh is missing: GitHub CLI is
  ticked, **Install GitHub CLI** installs only on click, **Not Now** hides it. The GitHub page gets a
  banner. Sawit calls `gh` nowhere else today.
- **MCP:** `list_dev_tools` shows `gh auth`.
- **Checked:** gh 2.103.3 is detected and signed in on this Mac. **Not checked** in the real app: the
  Set Up section and the banner, because gh is installed here.

## Item 3: MinIO and the MinIO Client (`616d9f5`)

- `brew install minio/stable/minio` and `minio/stable/mc`. An `mc` counts only if `mc --version`
  names MinIO, so Midnight Commander is excluded. Versions are release dates; there is no latest
  version to fetch (the upstream repos are archived). Homebrew core's `minio` and `minio-mc` are
  deprecated and Homebrew disables them on 2027-02-17 and 2027-07-17.
- **Checked:** the user's core `minio` and `minio-mc` are detected with the right upgrade and
  uninstall lines. The tap's `mc` was not installed (it conflicts with the user's `minio-mc`); fake-Cellar
  tests cover it.
- **Proposal, not built:** a MinIO instance kind (a data dir; `minio server --address
  127.0.0.1:9000 --console-address 127.0.0.1:9001` with generated root credentials; start, stop,
  logs; `write_env` writing `S3_ENDPOINT`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
  `AWS_REGION`; MCP `create_instance` with `kind=minio`). Instances need a kind first.

## Item 23: Herdr and Herdr TUI (`d13b79d`)

- **Found:** the app installs to `~/Applications/Herdr.app` with a link at `~/.local/bin/herdr-app`;
  herdr-core is installed only if missing (`~/.local/share/herdr-core`, links `herdr-core` and
  `hc`). The app is Apple silicon only, its latest version is in `releases.herdr.dev/app/latest.json`,
  and it updates itself. The TUI lives at `~/.local/bin/herdr` and updates with `herdr update`.
  `brew install herdr` is in Homebrew core.
- **Built:** the app is gated ("Needs an Apple Silicon Mac", no Install; MCP and the CLI refuse
  it). Uninstall closes the app, runs `hc server stop`, and removes only what the installer put
  there; the confirmation says agents stop. Settings are never deleted; **Delete Settings…** is a
  separate, confirmed choice.
- **Checked:** the user's running Herdr app 0.1.13 is detected and was not touched. The TUI did a
  full round trip through the CLI and through MCP install, upgrade and uninstall, then was removed.
- **Not checked:** Intel gating on real hardware (unit-tested only), and the app's uninstall for
  real.

## Item 24: HyperFrames CLI, Studio and skills (`7115482`)

- **CLI:** npm (`hyperframes` 0.9.19, Node 22 or newer); no Homebrew formula. `--version` works.
  **Run Doctor** on the row. Missing FFmpeg or Chrome is reported, pointing to
  `HYPERFRAMES_BROWSER_PATH`. Nothing installs silently. FFmpeg is added to the catalog.
- **Finding:** `hyperframes update --yes` also writes its Core Skills into every agent's folder. The
  upgrade sets `HYPERFRAMES_SKIP_SKILLS=1` (checked in a throwaway home).
- **Studio:** a universal DMG (`static.heygen.ai/hyperframes-oss/desktop/Install-HyperFrames.dmg`),
  bundle `dev.hyperframes.desktop` 0.1.0, self-updating, no cask. Install opens HeyGen's installer;
  the user signs in to HeyGen in the app.
- **Skills pack:** the `hyperframes@hyperframes` plugin for Claude Code, or the Core Skills through
  the skills CLI.
- **Decision:** the category stays "Terminal & Apps"; a Media category can come later.
- **Checked:** doctor passes except FFmpeg, FFprobe and optional tools. Studio, already in
  /Applications, is detected. The pack passes live install and remove for Codex and a throwaway
  Claude profile. The CLI was uninstalled again through MCP.
- **Not checked:** installing Studio, and a fully green doctor (needs FFmpeg).

## Item 29: voxl (`32c597b`)

- `brew install x264 opus pkgconf esbuild && cargo install voxl-server` (libyuv is Linux-only). The
  page runs it in a terminal; MCP runs it directly. Upgrade and uninstall use cargo and leave the
  brew libraries. Latest from crates.io.
- The row shows Sawit's Screen Recording and Accessibility state with buttons to the Settings panes.
  **Start** opens a tab with a random 128-bit `VOXL_PASSWORD` and runs `voxl --addr=127.0.0.1
  --port=6901`; a test checks the address. No LAN option. Sawit never ships the binary.
- **Not checked:** the build, the install round trip and the permission prompts.

## Item 4: shadcn/ui skills (`b07064a`)

`npx skills add shadcn/ui` (repo `shadcn-ui/ui`) installs `shadcn` and `migrate-radix-to-base`. A live
test installs and removes them for Claude Code and Codex.

## Item 28: Design to Code (`b950744`, Droid fix `bea9b29`)

- Two routes: the skills CLI with `--skill design-to-code`, or a clone that copies only the
  `design-to-code` folder. The description notes the overlap with build-design and asks the user to
  restart the agent's session.
- Live test: Claude Code, Codex and Factory Droid each get only that folder, with no `websites/`.
- **Finding and fix (`bea9b29`):** the skills CLI (npm `skills` 1.7.6) installs Droid's skills to
  `~/.agents/skills`, and Factory's docs list that folder. Droid now shares the `~/.agents/skills`
  target; `~/.factory/skills`, when it exists, stays a target of its own.

---

## Item 19: the GitHub sign-in lost on restart (`99b607c`)

Suspects, checked in order:
1. **A Keychain read error looked like "signed out": confirmed.** `load` turned any `security` error
   into `None`.
2. **Any 401 deleted the token: confirmed.** Every 401 from any endpoint became REVOKED, and the
   item was deleted.
3. Another Keychain account per launch: ruled out for the normal launch (`SAWIT_HOME` isn't set in
   launchctl, a login zsh or any MCP client's entry).
4. A sign-out on quit: ruled out.
5. A quiet save failure: ruled out for the sign-in save. One quiet loss was fixed: `set` deletes
   then adds, and now retries the add once.

Also ruled out: token expiry (the OAuth App's tokens don't expire). Which suspect hit the user
can't be told without their data; 1 and 2 are both fixed.

**Built:** `Session::Unreadable` ("Couldn't read your GitHub sign-in from the Keychain" with Try
Again; the item is kept). A 401 elsewhere triggers `GET /user` with the same token, and only a 401
there deletes. Offline stays signed in, unconfirmed. `github-sign-in.log` in Sawit's data folder
records why a session ended, never the token. `app_state.github.state`.

**Checked:** unit tests with the in-memory store: a load error keeps the token; a 401 from another
endpoint keeps it; a 401 from `/user` removes it and logs; a restart with a saved token is signed in
(ok, offline, 5xx). e2e `github.test.js` (2 tests) passes. **Not checked:** a real sign-in, quit and
reopen. The e2e's throwaway HOME has no login keychain, so a saved sign-in can't be faked there.

## Item 20: shell files and GitHub (`3812290`, `82a10e5`)

- **One control per state:** signed out → only Sign In to GitHub; checking → progress; Keychain
  error or offline → the reason and Try Again; no repo → only Save Shell Files to GitHub; repo →
  sync.
- **Sync:** per-file state (Same, Changed Here, Changed on GitHub, Changed on Both, Only Here, Only
  on GitHub) from content hashes against the last synced commit. Upload keeps the secrets question
  and shows a diff summary. Download shows a diff per file, asks "Replace N local files?", writes
  only known shell files under home, keeps permissions (600 for new files), backs up each file
  (`~/.zshrc.sawit-backup-YYYYMMDD-HHMMSS`) and offers Undo. Conflicts: Keep Local, Keep GitHub's,
  Open Both. Afterwards, Source in Open Terminals (only shells at their prompt).
- **MCP:** `shell_files_sync {status|upload|download}` (explicit files, `overwrite_conflicts`,
  `allow_secrets`). `sha1` 0.11 was already in the app, so no size cost.
- **Checked:** core tests with `fixtures/tree.json`. **Not checked** against a real repository.

## Item 5: prompts on GitHub (`60c4136`)

- `prompts/<id>.md` with front matter (id, name, tags, variables, favorite, times) and a README
  index; a private `sawit-prompts` repo by default; Back Up Now, Restore (asks first), Back Up
  Automatically (30 s debounce); conflicts listed with Keep Mine / Keep GitHub's.
- **MCP:** `backup_prompts`, `restore_prompts {repo?, overwrite: [ids]}`.
- **Checked:** a round-trip test (back up, delete, restore, variables intact) with GitHub stood in
  for by the backed-up files. **Not checked** against GitHub.
- **Follow-ups:** a local delete isn't synced (Restore brings it back; needs a tombstone). Restore
  saves the sync state before the library is written.

---

## Item 14: the Permissions page (`dd37467`)

- **Claude Code files** in Claude Code's order: managed (read only), each open project's
  `.claude/settings.local.json` and `.claude/settings.json`, and each profile's `settings.json`.
- **Edits:** Add to Allow/Ask/Deny…, Move to …, Remove Rule…. Each opens a sheet with its diff;
  Save Change writes a backup (`<file>.sawit-backup`), refuses a file that changed since it was read,
  then writes. Edits splice the rule's text into span-parsed JSON, so keys, order, indentation,
  comments and trailing commas stay byte for byte.
- **Secrets:** rules that look like tokens or keys are refused.
- **Sawit's confirm tools:** 25 tools, read from the tools' `confirm` parameters by a test, each with
  what Claude Code's rules make of it. Always Ask… and Deny… write to the user settings.
- **macOS:** Screen Recording and Accessibility show their real state; Automation and Files &
  Folders say "See System Settings" (macOS won't report them without prompting); each has Open
  Settings.
- **Other agents, read only:** Codex, Gemini CLI, Qwen Code, OpenCode.
- **MCP:** `list_permissions`, `edit_permission` (diff without `confirm`; backup and write with it).
- **Decisions:** other agents are read only (TOML and nested formats, and a wrong edit changes what
  an agent may do); the confirm-tools list is a constant in core checked against MCP by a test.
- **Checked:** 12 core tests, an app test for the sheet, MCP tests, and the MCP tools exercised in a
  throwaway HOME. e2e `permissions.test.js` (add, move and remove a rule at user level, add one at
  project level, read each file back) failed on a locked screen in the catalog track and ran in the
  integrator's full runs.

---

## Item 9: Claude Pool → Agent Accounts

**Asked:** find out what "Claude Pool" is, ask the user what moves into Sawit, write a plan, get a
yes. The plan is in `sawit-wt/reports/pool-plan.md`.

**What it is:** a SwiftUI menu bar app (`/Applications/ClaudePool.app`) with a CLI (`claude-pool`,
`cp1`…`cp9`). Here it manages 4 Claude Code profiles (`~/.claude`, `~/.claude-2`…`-4`), switches
the active one through a `~/.zshrc` block, collects 5h/7d usage through each profile's
`statusLine`, syncs shared settings, and has sessions, analytics, alerts and diagnostics.

**The user's answers (the item's open questions):** the name **Agent Accounts**; no menu bar item;
keep `cp1`–`cp9`; no herdr; keep self-update and add launch at login; move everything; offer
Claude Pool's removal afterwards.

**Built (11 commits):**
- `5b19500` core: accounts, the active pointer, Claude Code's sign-in (whitelist) and plan.
- `b613ef8` the usage collector (`sawit` as `sawit-statusline`), 5h/7d usage in Claude Pool's file
  format, the tracking opt-in with backups, Check Usage Now.
- `56bdb43` the Accounts page, the in-app sign-in, the title-bar chip, `app_state.agent_accounts`.
- `279d0c9` the shell block (replaces Claude Pool's), `sawit account …`, cp1–cp9, Open at Login.
- `f8ef4fe` shared settings (links and three-way merges, with backups).
- `accc080` Sawit's status line (Claude Pool's layout format and renderer).
- `be9bdce` sessions and Continue On…, usage analytics, limit alerts, Diagnostics.
- `3f157ea` MCP: `list_agent_accounts`, `agent_account_usage`, `use_agent_account`,
  `open_agent_account`, `agent_account_sessions`, `sync_agent_settings`.
- `f7c47dc` import from Claude Pool and the confirmed removal offer; `4f5d2fc` removals use the
  shared Confirm; `ec11137` embeds the page's icons.

**Decisions:** Open at Login is new (SMAppService, off by default, state read back from the system).
Limit alerts are an in-app notice plus a best-effort `osascript` notification (no new dependency).
The collector is the `sawit` binary under another name, through a link the app refreshes at launch.
Sawit never reads, copies or moves `Claude Code-credentials*` Keychain items.

**Checked:** unit tests (core `agent_accounts` and `cli::` 57, app `accounts::` and `confirm` 13,
MCP 9) with Claude Pool's fixtures, temp homes and a stand-in `claude`. e2e `accounts.test.js`
(7 tests) failed on a locked screen in its own track. In the integrator's run it found two real bugs,
both fixed: the Diagnostics checks weren't readable by accessibility (`5ac8b79`), and the real
`/Applications` Claude Pool leaked into the test home (`35a87c0`).

**Not verified (the user's own setup):** the 8 checks in `pool-plan.md`: import from Claude Pool;
the status line and `sawit account list`; `cp2`; Set Active; Sign In; Shared Settings; Remove
Claude Pool; Open at Login.

---

## Parity pass: CLI and MCP

After the merge, a parity pass checked that the app, MCP and the `sawit` CLI reach the same
features:
- `c360fe3`: `sawit permissions` (`izin`), `sawit shell` (`cangkang`) and `sawit prompts` (`mantra`),
  with English and Indonesian subcommands, on the same core as the app and MCP. The shell-file pick
  and secret check moved into core (`shell_config::pick_files`, `secret_lines`). `654d624` documents
  them in the README.
- `a97f93e`: `app_open_page` names the Permissions and Accounts pages.
- `74c97eb`: the README's MCP table matches `tools/list` from the release `sawit-mcp` (163 tools).
- `ace91bb`: `canvas_get` returns the page's comment threads, and the README says how agents reach
  the canvas's new FigCraft tools through `canvas_command`.
