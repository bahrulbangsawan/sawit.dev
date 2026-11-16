# Plan 001: Run clippy and the test suite in CI, and gate every release on them

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 7e715d73..HEAD -- .github/workflows crates/core/tests crates/mcp/tests/tools/db.rs README.md`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S (M if the baseline fails)
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests / dx
- **Planned at**: commit `7e715d73`, 2026-11-16

## Why this matters

Sawit is a macOS desktop app (Rust, ~350K lines, ~2,560 tests). Every push to `main` cuts a release
(`.githooks/pre-push` → `scripts/release.sh` tags `vX`), and the tag's workflow builds and publishes
the app that the in-app updater installs on every user's Mac. **Nothing on that path runs
`cargo test` or `cargo clippy`**: the only CI check is a line-count script. A regression is found by
users. Also, the PostgreSQL integration tests print `skip:` and *pass* when no PostgreSQL binaries
exist, so on a fresh machine the database tests would be green without running. This plan adds a
CI workflow that runs clippy and the tests (with PostgreSQL present and skips turned into failures),
and makes the release workflow wait for it.

## Current state

- `.github/workflows/` holds `line-limit.yml`, `release.yml`, `site.yml`. None runs cargo test/clippy.
- `.github/workflows/line-limit.yml` (the style to copy — comment header, explicit permissions,
  actions pinned to a commit SHA, `persist-credentials: false`):
  ```yaml
  # Keeps every hand-written source file at 500 lines or fewer (scripts/check-line-limit.sh), on
  # every pull request and every push to main. ...
  name: Line limit

  on:
    pull_request:
    push:
      branches: [main]

  permissions:
    contents: read

  jobs:
    check:
      runs-on: ubuntu-latest
      timeout-minutes: 5
      steps:
        - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
          with:
            persist-credentials: false
        - run: sh scripts/check-line-limit.sh
  ```
- `.github/workflows/release.yml` (excerpt): `on: push: tags: ["v*"]` and `workflow_dispatch` with a
  `tag` input; `permissions: contents: write`; one job:
  ```yaml
  jobs:
    release:
      runs-on: macos-15
      env:
        TAG: ${{ inputs.tag || github.ref_name }}
      steps:
        - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
          with:
            ref: ${{ inputs.tag || github.ref }}
            persist-credentials: false
        - uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3
        - name: Build app
  ```
- A test enforces workflow hygiene: `crates/core/tests/release_it/workflows.rs:136`
  `every_workflow_pins_its_actions_and_keeps_no_token_on_disk`. For **every** `.yml` in
  `.github/workflows` it asserts: a top-level line starting with `permissions:`; every
  `uses:` (other than `./…` local ones) is pinned to a 40-hex commit; every `actions/checkout` step
  contains `persist-credentials: false`; and at least one checkout step exists. Your new workflow
  must satisfy all four.
- Rust toolchain pinned by `rust-toolchain.toml` (1.99.1, with clippy and rustfmt components);
  rustup on the runner installs it automatically.
- PostgreSQL test skips. `crates/core/tests/db_it/main.rs:34-36`:
  ```rust
  async fn cluster() -> Option<(Cluster, ConnProfile)> {
      let roots = SearchRoots::detect(PathBuf::from("/nonexistent")).await;
      let bins = discover(&roots).await.into_iter().next()?;
  ```
  Each test then does `let Some(..) = cluster().await else { eprintln!("skip: no PostgreSQL binaries"); return; };`.
  Other places that discover binaries and skip: `crates/core/tests/db_it/changes.rs:156`
  (`let bins = discover(&roots).await;`), `crates/core/tests/control_it.rs:20-21`,
  `crates/mcp/tests/tools/db.rs:73`. Find them all with
  `grep -rn "discover(&roots)" crates/core/tests crates/mcp/tests`.
  `discover` finds Homebrew kegs such as `/opt/homebrew/opt/postgresql@17/bin`.
- MySQL live tests (`crates/core/src/db/mysql/live_tests.rs`) run only when
  `SAWIT_TEST_MYSQL_URL` is set; leave them alone (no MySQL in CI).
- README `## Test` section (around line 2187) documents:
  ```
  cargo test --workspace
  cargo test --workspace -- --include-ignored
  cargo clippy --workspace --all-targets -- -D warnings
  ```
- **Do not add `cargo fmt --check`**: much existing code is not rustfmt-clean, and the maintainers
  deliberately do not format whole crates.

## Commands you will need

**Environment (mandatory for every cargo command on this machine):** the Rust toolchain lives on
an external disk. Every cargo command must be prefixed with:
```
source /Volumes/bahrul/.toolchains/env.sh && export CARGO_TARGET_DIR=/Volumes/bahrul/project/sawit-advisor-target &&
```
Never run cargo without it (it would install a toolchain on the internal disk).

**Where to run long commands:** the owner wants builds and tests run in the Sawit app's terminal,
not hidden shells. Use the `sawit` MCP tool `terminal_run` (load it with ToolSearch
`select:mcp__sawit__terminal_run`) with `new_tab: true` and `cwd` = your worktree root, and make the
command write a log and a marker, e.g.
```
source /Volumes/bahrul/.toolchains/env.sh && export CARGO_TARGET_DIR=/Volumes/bahrul/project/sawit-advisor-target && cargo test --workspace 2>&1 | tee /Volumes/bahrul/.toolchains/logs/advisor-001-test.log; echo "exit=${pipestatus[1]}" > /Volumes/bahrul/.toolchains/logs/advisor-001-test.done
```
(The shell is zsh: `${pipestatus[1]}` is cargo's exit code.) Then wait for the `.done` file
(use the Monitor tool if available, otherwise check it with Bash `test -f` every minute or so) and
read the log with Bash/Read. Never type into a terminal tab you did not just create; tab indices
change. Remove a stale `.done` file before reusing its name. Builds share one target directory with
other executors and wait on cargo's lock ("Blocking waiting for file lock") — that is normal; the
first build can take 30+ minutes.

| Purpose | Command (after the env prefix) | Expected on success |
|---|---|---|
| Workspace clippy | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Workspace tests | `cargo test --workspace` | exit 0 |
| Workflow hygiene test | `cargo test -p sawit-core --test release_it` | exit 0 |
| Line limit | `sh scripts/check-line-limit.sh` | "All N source files are at most 500 lines." |
| YAML parses | `ruby -ryaml -e 'YAML.load_file(ARGV[0])' .github/workflows/ci.yml` | exit 0, no output |

## Scope

**In scope**:
- `.github/workflows/ci.yml` (create)
- `.github/workflows/release.yml` (add a gating job only)
- `crates/core/tests/db_it/main.rs`, `crates/core/tests/db_it/changes.rs`,
  `crates/core/tests/control_it.rs`, `crates/mcp/tests/tools/db.rs` (only the "no binaries" branches)
- `README.md` (the `## Test` section only)

**Out of scope**:
- Fixing any failing test or clippy warning in source code — if the baseline fails, STOP (see below).
- `scripts/release.sh`, `.githooks/pre-push` — the local release flow stays as is.
- `cargo fmt` anywhere; `e2e/` (needs macOS accessibility permission, can't run on hosted runners).

## Git workflow

- Work on the branch your worktree is on. Commit in logical units; message style follows the repo
  (conventional commits with a plain-English sentence), e.g.
  `ci: clippy and the test suite run on every pull request and push to main, and a release waits for them`.
- Do NOT push. Never run `git push` (a push of main cuts a release).

## Steps

### Step 1: Measure the baseline

Run workspace clippy and workspace tests on the unchanged tree (two Sawit terminal runs as above).

**Verify**: both `.done` files say `exit=0`. If either is non-zero, STOP and report the failing
tests / clippy lints (names and file:line, from the log) — do not fix them.
Also record from the test log whether any `skip: no PostgreSQL binaries` lines appear
(`grep -c "skip:" <log>`), and report that count.

### Step 2: Make PostgreSQL skips fail when `SAWIT_REQUIRE_DB` is set

At each place that discovers PostgreSQL binaries for a test (list in Current state), when none are
found and the environment variable `SAWIT_REQUIRE_DB` is set (any value), `panic!` with
`"SAWIT_REQUIRE_DB is set, but no PostgreSQL binaries were found"`. Without the variable, behavior
is unchanged. In `db_it/main.rs`, do it inside `cluster()` (replace the `?` on `discover` with a
`let Some(bins) = … else { … }`), so the 15+ callers need no edit. Keep each file ≤ 500 lines.

**Verify**: `cargo test -p sawit-core --test db_it` → exit 0 (with binaries present or not).
Then, only if Step 1's log had zero `skip:` lines (binaries are present locally), there is nothing
more to check locally. If it had skips, run
`SAWIT_REQUIRE_DB=1 cargo test -p sawit-core --test db_it` and confirm it now FAILS with the new
message (then you know the switch works).

### Step 3: Add `.github/workflows/ci.yml`

Create it in the style of `line-limit.yml` (comment header explaining what it does and why), with:
- `name: CI`
- triggers: `pull_request:`, `push: branches: [main]`, and `workflow_call:` (so release.yml can reuse it)
- `permissions:` → `contents: read`
- `concurrency: group: ci-${{ github.ref }}`, `cancel-in-progress: true` — but NOT for the
  workflow_call case; simplest: omit concurrency entirely. (Omit it.)
- one job `checks`, `runs-on: macos-15`, `timeout-minutes: 120`, `env: SAWIT_REQUIRE_DB: "1"`, steps:
  1. `actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0` with `persist-credentials: false`
  2. `Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3`
  3. `name: PostgreSQL for the database tests` → `run: brew install postgresql@17`
  4. `name: Clippy` → `run: cargo clippy --workspace --all-targets -- -D warnings`
  5. `name: Tests` → `run: cargo test --workspace`

Use exactly those two pinned SHAs (they are already used in this repo).

**Verify**: YAML parse command → exit 0; `cargo test -p sawit-core --test release_it` → exit 0.

### Step 4: Gate the release on the checks

In `release.yml` add, before the `release:` job:
```yaml
  checks:
    uses: ./.github/workflows/ci.yml
```
and add `needs: checks` to the `release` job. Change nothing else in release.yml (the Publish
step's script is tested verbatim by `release_it`). Add one comment line above `checks:` saying a
release is published only after clippy and the tests pass.

**Verify**: YAML parse of release.yml → exit 0; `cargo test -p sawit-core --test release_it` → exit 0.

### Step 5: Document it

In README `## Test`, add one line under the commands: CI (`.github/workflows/ci.yml`) runs clippy
and `cargo test --workspace` on every pull request and push to main, a release waits for it, and
`SAWIT_REQUIRE_DB=1` turns the PostgreSQL tests' skip into a failure. Keep the README's plain,
full-sentence style.

**Verify**: `git diff --stat` shows only in-scope files.

## Test plan

No new Rust tests. The existing `release_it` workflow-hygiene test covers the new workflow file.
The `SAWIT_REQUIRE_DB` switch is checked in Step 2.

## Done criteria

- [ ] Step 1 baseline exit codes were both 0 (else STOPPED with the failure list)
- [ ] `.github/workflows/ci.yml` exists; `cargo test -p sawit-core --test release_it` exits 0
- [ ] `grep -n "needs: checks" .github/workflows/release.yml` → one match
- [ ] `grep -rn "SAWIT_REQUIRE_DB" crates/ .github/ README.md` → matches in the test files, ci.yml and README
- [ ] `cargo test -p sawit-core --test db_it` and `cargo test -p sawit-mcp --test tools` exit 0
- [ ] `sh scripts/check-line-limit.sh` passes
- [ ] `git status` shows only in-scope files changed

## STOP conditions

- Step 1 baseline is not green (report failures; fixing them is a separate plan).
- The `release_it` test fails because of how `uses: ./…` or `needs:` is parsed and the fix would
  require editing the test.
- Any excerpt above doesn't match the live files.

## Maintenance notes

- CI time on `macos-15` will be long (GPUI is large); `rust-cache` keeps re-runs faster. If CI
  minutes become a cost issue, drop the `push` trigger and keep `pull_request` + `workflow_call`.
- A workflow_dispatch rebuild of an old tag runs the *current* checks against the dispatched ref,
  not the tag; acceptable.
- The MySQL live tests stay opt-in (`SAWIT_TEST_MYSQL_URL`); adding a MySQL service would need
  `brew install mysql` and a started server — deferred.
