# Plan 005: A prompt restore records its sync state only after the restored prompts are saved, so a failed save can't make the next backup delete them on GitHub

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 7e715d73..HEAD -- crates/core/src/prompt_sync.rs crates/core/src/prompt_sync crates/core/src/github/sync.rs crates/core/src/kebun/mantra.rs crates/mcp/src/prompt_backup.rs crates/app/src/prompts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: bug (data loss)
- **Planned at**: commit `7e715d73`, 2026-11-16

## Why this matters

Sawit keeps a prompt library (`~/Library/Application Support/sawit/prompts.json`) and syncs it two
ways with a private GitHub repo. The sync keeps a *base* (`SyncBase`: the last synced commit and
each file's hash) in a state file. When a prompt is **missing here** but the base says it was
synced and GitHub still matches the base, the planner treats it as **deleted here**, and the next
backup (including the automatic one) deletes it on GitHub.

`prompt_sync::restore` records the restored prompts into the base and **saves the base itself**,
while the restored library is saved later by each caller. If that later library save fails or never
happens (save error, app quit, the page closed during the network round trip), the state says
"synced" for prompts that aren't in `prompts.json`, and the next backup deletes them from GitHub.
The fix: `restore` returns the new base instead of saving it, and every caller saves it **only
after** the library is on disk.

## Current state

- `crates/core/src/prompt_sync.rs:407-461` — `restore`:
  ```rust
  /// Brings in the prompts changed on GitHub (and those only there) to `library`, keeping their
  /// use counts here. `keep_github`: ids changed on both sides whose GitHub version wins. The
  /// caller saves the library.
  pub async fn restore(token: &str, library: &mut Library, settings: &BackupSettings,
                       state: &Path, keep_github: &[String], dry_run: bool) -> Result<SyncOutcome> {
      …
      let base = SyncBase::load(state);
      …                       // dry run: returns early, nothing saved
      let (mut out, synced) = apply_restore(library, &plan, &contents, keep_github)?;
      out.repo = format!("{login}/{}", settings.repo);
      let mut base = base;
      base.record(&backup.tree.commit, github::now(), &synced);
      base.save(state)?;
      Ok(out)
  }
  ```
- `SyncOutcome` (`prompt_sync.rs:252`): `#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)] pub struct SyncOutcome { pub repo, pub synced, pub synced_ids, pub conflicts, pub deleted, pub secrets, pub held, pub dry_run, pub created_repo }`.
  Built in two places (`prompt_sync.rs:313` backup and `:428` restore dry run) with struct literals.
- `SyncBase` (`crates/core/src/github/sync.rs:71`): `Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize`; `save(&self, path)` writes with plain `std::fs::write` (line 87-95). `BackupSettings::save` (`prompt_sync.rs:52-60`) is the same.
- The four callers of `restore`:
  1. CLI — `crates/core/src/kebun/mantra.rs:184-200`:
     ```rust
     match prompt_sync::restore(&token, &mut lib, &settings, &state, &args.overwrite, false).await {
         Ok(done) => {
             if !done.synced.is_empty() || !done.deleted.is_empty() {
                 if let Err(e) = lib.save(&file) { println!(…); return 1; }
                 notify_reload(paths, Store::Prompts);
             }
     ```
  2. MCP — `crates/mcp/src/prompt_backup.rs:165-181`:
     ```rust
     let done = tri!(prompt_sync::restore(&token, &mut lib, &s, &state, &a.overwrite, a.dry_run).await.map_err(|e| e.to_string()));
     if done.dry_run || (done.synced.is_empty() && done.deleted.is_empty()) {
         return ok(said("restore", &done));
     }
     if let Err(e) = self.save_library(&lib).await {
         return fail(e);
     }
     ok(said("restore", &done))
     ```
  3. App, Restore button — `crates/app/src/prompts/github.rs:184-224`: runs `restore` on a clone of
     `this.store.read(cx).library` in `rt::spawn`, then on `Ok(Ok((done, restored)))` calls
     `this.apply_restored(&done, restored, cx)`. State path: `state_path()` (github.rs:49).
  4. App, automatic pull — `crates/app/src/prompts/github_sync.rs:90-130`: same, but only calls
     `apply_restored` when `synced`/`deleted` is non-empty. State path is computed as
     `AppPaths::default_macos().root.join(prompt_sync::SYNC_STATE)`.
- `apply_restored` (`github_sync.rs:25-41`) puts each restored prompt into the store
  (`s.put(p, cx)`) and removes deleted ones; the store then saves **debounced and asynchronously**.
- `crates/app/src/prompts/store.rs` (157 lines) — `PromptStore { file, library, load_error, notice, save_scheduled }`;
  `changed()` skips saving when `load_error` is set; the save runs `library.save(&file)` via
  `rt::spawn` after `SAVE_DEBOUNCE`, and on error sets `notice`.
- The atomic writer to use for JSON state: `crate::files::write_atomic(path, bytes)`
  (`crates/core/src/files/write.rs:21`).
- Core tests for the sync logic: `crates/core/src/prompt_sync/tests.rs` (200 lines), e.g.
  `a_delete_on_github_carries_over_on_restore` uses `library()`, `library_files`, `SyncBase::record`,
  `RemoteTree { commit, files }`, `plan_prompts(&lib, &tree, &base)`, `apply_restore(...)`,
  `FileState::*`. Copy that style.

## Commands you will need

**Environment (mandatory for every cargo command):** prefix with
```
source /Volumes/bahrul/.toolchains/env.sh && export CARGO_TARGET_DIR=/Volumes/bahrul/project/sawit-advisor-target &&
```
Run cargo in the Sawit app's terminal: `sawit` MCP tool `terminal_run` (ToolSearch
`select:mcp__sawit__terminal_run`), `new_tab: true`, `cwd` = worktree root, command ending in
`2>&1 | tee /Volumes/bahrul/.toolchains/logs/advisor-005.log; echo "exit=${pipestatus[1]}" > /Volumes/bahrul/.toolchains/logs/advisor-005.done`.
Wait for the `.done` file (Monitor tool, or Bash `test -f` every minute), read the log, delete the
`.done` before reuse. Never type into a tab you didn't just create. Shared target dir: lock waits
and long first builds are normal.

| Purpose | Command (after env prefix) | Expected |
|---|---|---|
| Core tests | `cargo test -p sawit-core --lib prompt_sync github::sync kebun` | all pass |
| MCP build+tests | `cargo test -p sawit-mcp --lib prompt` | all pass (0 tests is OK if none match; it must compile) |
| App tests | `cargo test -p sawit --bin sawit prompts` | all pass |
| Clippy | `cargo clippy -p sawit-core -p sawit-mcp -p sawit --all-targets -- -D warnings` | exit 0 |
| Line limit | `sh scripts/check-line-limit.sh crates/core crates/mcp crates/app` | all ≤ 500 |

## Scope

**In scope**: `crates/core/src/prompt_sync.rs`, `crates/core/src/prompt_sync/tests.rs`,
`crates/core/src/github/sync.rs` (only `SyncBase::save`), `crates/core/src/kebun/mantra.rs`,
`crates/mcp/src/prompt_backup.rs`, `crates/app/src/prompts/store.rs`,
`crates/app/src/prompts/github.rs`, `crates/app/src/prompts/github_sync.rs`.

**Out of scope**: `backup()` (it records state after GitHub accepted the commit — correct as is);
shell-config sync (`crates/core/src/shell_config/**`), which has its own base; the planner rules in
`plan_prompts`.

## Git workflow

Commit on your worktree's branch, e.g.
`fix(prompts): a restore records what it synced only once the restored prompts are saved, so the next backup never deletes them on GitHub`.
Do NOT push.

## Steps

### Step 1: A test that shows the hazard and the safe order

In `prompt_sync/tests.rs`, add a test (doc comment explaining the bug) that builds a library and a
GitHub tree where GitHub has a prompt the library lacks, then:
- with the **old base** (nothing recorded), `plan_prompts(&lib_without_it, …)` marks that prompt as
  coming from GitHub (`OnlyOnGitHub` or whatever `downloads()` is true for) — safe;
- with the base **recorded as if the restore had been saved** (call `base.record(...)` with the
  synced list from `apply_restore` on a *clone* of the library) but planning against the **unsaved**
  library, the state is `DeletedHere` — the hazard this plan prevents.
This documents why the base must be saved after the library. It should pass before and after the fix.

**Verify**: core tests → pass.

### Step 2: `restore` returns the base instead of saving it

- Add to `SyncOutcome`:
  ```rust
  /// A restore's new sync state (not on a dry run): save it with [`SyncBase::save`] only after the
  /// library is saved, or the next backup reads the prompts it brought in as deleted here.
  #[serde(skip)]
  pub restored_base: Option<SyncBase>,
  ```
  (fix the two struct literals: `..Default::default()` or `restored_base: None`).
- In `restore`, replace `base.save(state)?;` with `out.restored_base = Some(base);`. `state` is
  still used to *load* the base. Update the doc comment: "The caller saves the library, then
  `restored_base`."
- Make `SyncBase::save` and `BackupSettings::save` write with `crate::files::write_atomic`.

**Verify**: `cargo build`-level check via core tests → pass (callers in other crates will fail to
compile only if they built `SyncOutcome` literally; they don't).

### Step 3: CLI and MCP save the base after the library

- `mantra.rs`: on `Ok(done)`: if prompts changed, save the library (as today; on error return 1
  **without** saving the base); then, if `done.restored_base` is `Some(b)`, `b.save(&state)` — on
  error print it the same way and return 1. (When nothing changed, saving the base alone is safe.)
- `prompt_backup.rs`: same order: dry run → return; if changed → `save_library` (on error `fail`,
  base not saved); then save `restored_base` (on error `fail(e.to_string())`); then `ok(...)`.

**Verify**: MCP and core test commands compile and pass.

### Step 4: App saves the base only after its own library save

- In `store.rs`, add a method (doc comment in the file's style):
  ```rust
  /// Saves the library now (no debounce) and then, once it is on disk, `base` at `state`: a
  /// restore's sync state must never be ahead of prompts.json. Nothing is saved while the
  /// library failed to load.
  pub fn save_restored(&mut self, base: SyncBase, state: PathBuf, cx: &mut Context<Self>)
  ```
  If `load_error` is set: set `notice`, return. Otherwise clone `library` and `file`, and in
  `rt::spawn` run `library.save(&file)` then, only on success, `base.save(&state)`; on any error set
  `notice` (as `changed()` does). Keep store.rs ≤ 500 lines.
- `github.rs` (Restore button) and `github_sync.rs` (auto pull): after `apply_restored(...)` (or,
  in the auto pull, also when nothing changed but restore succeeded), call
  `this.store.update(cx, |s, cx| s.save_restored(base, state, cx))` with `done.restored_base`
  (take it out of `done` before passing `&done` elsewhere; it's `Option`, skip when `None`).
  Use `state_path()` / the same `AppPaths…SYNC_STATE` path each already uses.
- Add an app test if the store can be built in a `#[gpui_kit::test]` (look for an existing
  `PromptStore` test; if none exists, skip the app test and say so in your report).

**Verify**: app tests → pass; `grep -rn "base.save(state)" crates/core/src/prompt_sync.rs` → only the `backup()` call remains (one match).

### Step 5: Lint and limits

**Verify**: clippy → exit 0; line limit → passes. Only rustfmt files that were clean before.

## Test plan

- Core: the hazard/safe-order test (Step 1).
- Compile-level: every caller handles `restored_base`.
- App: `save_restored` writes nothing when `load_error` is set (if a store test harness exists).

## Done criteria

- [ ] `grep -n "restored_base" crates/core/src/prompt_sync.rs crates/core/src/kebun/mantra.rs crates/mcp/src/prompt_backup.rs crates/app/src/prompts/*.rs` → matches in all four callers' files and the definition
- [ ] `grep -c "base.save(state)" crates/core/src/prompt_sync.rs` → 1 (backup only)
- [ ] all test commands pass; clippy exit 0; line limit passes
- [ ] `git status` shows only in-scope files

## STOP conditions

- Excerpts don't match the live code.
- `SyncOutcome` is deserialized anywhere (then `#[serde(skip)]` needs `Default` handling you must
  not guess at) — `grep -rn "SyncOutcome" crates` and check.
- A caller can't order the saves without restructuring its async flow beyond these files.

## Maintenance notes

- Rule for the future: sync state may never be ahead of the local data it describes. `backup()`
  already follows it (state after GitHub accepted the commit).
- If the app ever keeps the library somewhere other than prompts.json, `save_restored` must follow.
- Shell-config sync (`crates/core/src/shell_config/sync.rs`) has an analogous base; review it
  against this rule separately.
