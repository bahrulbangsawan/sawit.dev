# Plan 003: An unreadable instances, connections or accounts file is never saved over, and those files are written atomically

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 7e715d73..HEAD -- crates/core/src/instances.rs crates/core/src/db/profile/mod.rs crates/core/src/recent.rs crates/core/src/agent_accounts/import.rs crates/app/src/servers/store.rs crates/app/src/db/store/mod.rs crates/app/src/db/store/profiles.rs crates/app/src/accounts/mod.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S–M
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug (data loss)
- **Planned at**: commit `7e715d73`, 2026-11-16

## Why this matters

Sawit (a macOS developer app, Rust/GPUI) keeps the user's PostgreSQL instances in
`~/Library/Application Support/sawit/instances.toml`, saved database connections in
`profiles.toml`, and Claude Code accounts in `agent-accounts/accounts.json`. Two processes write
these files: the app, and `sawit-mcp` (one process per AI agent session). Today:

1. When the app can't parse one of these files at startup, it silently starts with an **empty**
   list, and the next add/remove/edit **saves the empty-plus-one list over the file**, losing every
   saved instance, connection or account.
2. `instances.toml`, `profiles.toml` and `recent.toml` are written through a fixed temp name
   (`*.toml.tmp`) with no fsync, so two processes saving at once share a temp file and can produce a
   torn file — which then triggers (1).
3. The app saves its in-memory instance list, so an instance `sawit-mcp` just added can be dropped.

The prompts store already does this right (refuses to save while its file failed to load); this
plan applies the same rule to the other stores and uses the repo's existing atomic writer.

## Current state

**The good pattern to copy** — `crates/app/src/prompts/store.rs`:
```rust
pub struct PromptStore {
    file: PathBuf,
    pub library: Library,
    /// Set when the file exists but can't be read: nothing is written then,
    /// so a damaged file is never replaced by an empty library.
    pub load_error: Option<String>,
    …
}
…
fn changed(&mut self, cx: &mut Context<Self>) {
    cx.notify();
    if self.load_error.is_some() || self.save_scheduled {
        return;
    }
```
and `reload()` sets `self.load_error = None` after a successful load.

**The atomic writer to use** — `crates/core/src/files/write.rs:21`, exported as
`crate::files::write_atomic(path: &Path, bytes: &[u8]) -> Result<()>`: private temp file with a
unique name (`O_EXCL`), fsync, rename; keeps permissions (new file 0644); a symlink's target is
replaced.

**Core savers using a fixed temp name** — identical in three files:
- `crates/core/src/instances.rs:180-190` (`Registry::save`)
- `crates/core/src/db/profile/mod.rs:423-431` (`Profiles::save`)
- `crates/core/src/recent.rs:28-37` (`save`)
```rust
pub fn save(&self, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(self).map_err(|e| CoreError::Config(e.to_string()))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}
```
`Registry::load` (`instances.rs:171-178`) returns `Ok(default)` for a missing file and `Err` for an
unreadable one. Same for `Profiles::load` and `Accounts::load`.

**App: instances** — `crates/app/src/servers/store.rs` (497 lines — the file limit is 500, so this
file must not grow; the changes below shrink it):
- `new()` line 63: `let registry = Registry::load(&paths.instances_file()).unwrap_or_else(|e| { eprintln!("registry: {e}"); Registry::default() });`
- `forget()` line ~338: `self.registry.remove(name); if let Err(e) = self.registry.save(&self.paths.instances_file()) { self.notice = Some(format!("Unable to save the instance list. {e}")); }`
- create, line ~414: `this.registry.add(inst).map_err(|e| e.to_string())?; this.registry.save(&this.paths.instances_file()).map_err(|e| e.to_string())?;`
- adopt, line ~486: the same two statements.
- `reload()` line 306 re-reads the file after `sawit-mcp` changes it.

**App: connections** — `crates/app/src/db/store/mod.rs` (223 lines):
- line 79: `let profiles = Profiles::load(&file).unwrap_or_default();` then `Self::with_profiles(file, profiles)` (line 92, struct literal lists every field; `DbStore::detached(profiles)` builds one for tests).
- line 121: `fn save(&mut self) { if let Err(e) = self.profiles.save(&self.file) { self.notice = Some(format!("Unable to save connections. {e}")); } }` — every connection add/edit/remove calls this.
- `crates/app/src/db/store/profiles.rs:243` `reload()` re-reads; on `Err` it sets a notice and returns.

**App: accounts** — `crates/app/src/accounts/mod.rs` (444 lines):
- `new()` calls `this.reload(); this.seed_default();`
- `reload()` (line 184): `match Accounts::load(&self.root) { Ok(a) => self.accounts = a, Err(e) => self.error = Some(e.to_string()) }`
- `seed_default()` (line 198): if the list is empty and `~/.claude` exists, adds a "default" account and calls `self.save()` — **this runs right after a failed load and overwrites the file**.
- `save()` (line 229): `if let Err(e) = self.accounts.save(&self.root) { self.error = Some(format!("Couldn't save the accounts: {e}")); }`
- `read_usage()` (line ~211) re-reads every 5 s: `if let Ok(on_disk) = Accounts::load(&self.root) && on_disk != self.accounts { self.accounts = on_disk; … }`

**Core: account import** — `crates/core/src/agent_accounts/import.rs:287`:
`let mut all = Accounts::load(root).unwrap_or_default();` … then saves.

## Commands you will need

**Environment (mandatory for every cargo command):** prefix with
```
source /Volumes/bahrul/.toolchains/env.sh && export CARGO_TARGET_DIR=/Volumes/bahrul/project/sawit-advisor-target &&
```
Run cargo in the Sawit app's terminal: `sawit` MCP tool `terminal_run` (ToolSearch
`select:mcp__sawit__terminal_run`), `new_tab: true`, `cwd` = worktree root, command ending in
`2>&1 | tee /Volumes/bahrul/.toolchains/logs/advisor-003.log; echo "exit=${pipestatus[1]}" > /Volumes/bahrul/.toolchains/logs/advisor-003.done`.
Wait for the `.done` file (Monitor tool, or Bash `test -f` every minute), read the log, delete the
`.done` before reusing it. Never type into a tab you didn't just create. Shared target dir: waiting
on cargo's lock and long first builds are normal.

| Purpose | Command (after env prefix) | Expected |
|---|---|---|
| Core tests | `cargo test -p sawit-core --lib instances db::tests::profiles recent agent_accounts` | all pass |
| App tests | `cargo test -p sawit --bin sawit servers db::store accounts` | all pass |
| Clippy | `cargo clippy -p sawit-core -p sawit --all-targets -- -D warnings` | exit 0 |
| Line limit | `sh scripts/check-line-limit.sh crates/core crates/app` | all ≤ 500 |

(If a test filter matches nothing, cargo prints "0 passed" — check the filters match real module
paths with `cargo test -p <crate> --lib -- --list | grep <word>` and adjust.)

## Scope

**In scope**: the eight files in the drift check.

**Out of scope**: `crates/mcp/**` (it already re-reads before saving; it benefits from the atomic
writer automatically); `crates/core/src/github/sync.rs` and `crates/core/src/prompt_sync.rs`
(another plan changes them); adding file locks (deferred); any UI redesign — reuse the existing
`notice` / `error` fields for messages.

## Git workflow

Commit on your worktree's branch, one commit per step is fine, e.g.
`fix(databases): a connections file that can't be read is never saved over`. Do NOT push.

## Steps

### Step 1: Core savers use the atomic writer

In `Registry::save`, `Profiles::save` and recent.rs's `save`, keep `create_dir_all(parent)` and the
`toml::to_string_pretty` line, and replace the temp-file + rename lines with
`crate::files::write_atomic(path, text.as_bytes())`. Update their doc comments if they mention the
temp file.

**Verify**: core tests → pass; `grep -rn 'with_extension("toml.tmp")' crates/core/src` → no match.

### Step 2: `Registry::update` — re-read, change, save

In `crates/core/src/instances.rs`, add:
```rust
/// Reads the list at `path` again, applies `change` and saves it: what another process
/// (`sawit-mcp`) saved since this one last read is kept, and a file that can't be read is
/// never saved over (its error is returned).
pub fn update<T>(path: &Path, change: impl FnOnce(&mut Registry) -> Result<T>) -> Result<(Registry, T)> {
    let mut registry = Registry::load(path)?;
    let out = change(&mut registry)?;
    registry.save(path)?;
    Ok((registry, out))
}
```
Add unit tests in instances.rs's `mod tests` (follow the existing ones, which use `tempfile`):
(a) `update` on an unreadable file (write `"not = [toml"`) returns `Err` and leaves the file's bytes
unchanged; (b) an entry saved by another writer between a first load and `update` is kept.

**Verify**: core tests → pass including the 2 new ones.

### Step 3: App instances use `Registry::update`

In `servers/store.rs`, change `forget`, create and adopt to call
`Registry::update(&self.paths.instances_file(), |r| { r.add(inst) })` (or `r.remove(name); Ok(())`)
and on `Ok((registry, _))` set `self.registry = registry`; on `Err(e)` keep today's error handling
(`notice` for forget, `Err(e.to_string())` for create/adopt). In `new()`, keep the fallback to an
empty list but also put the error into `notice` (`"Unable to read the instance list. {e}"`, the
same wording `reload()` uses) so the user sees it. The file must stay ≤ 500 lines: these changes
grow it (497 → ~506), so put a private `update_registry` helper and `forget()` in a child module,
declared with `mod registry;` in `servers/store.rs`, at `crates/app/src/servers/store/registry.rs`
(`impl ServerStore { … }` there; a child module can reach the struct's private fields). That new
file is in scope.

**Verify**: app tests → pass; `grep -n "registry.save\|registry\n *.save" crates/app/src/servers/store.rs` → no match; line-limit passes.

### Step 4: App connections refuse to save after a failed load

In `db/store/mod.rs`: add `load_error: Option<String>` to `DbStore` (doc comment as in PromptStore);
in `new()` replace `unwrap_or_default()` with a match that keeps `Profiles::default()` on `Err` and
records the error (also set `notice` to `"Unable to read connections. {e}"`); pass it through
`with_profiles` (add a parameter or set it after — `detached()` uses `None`). In `save()`, if
`load_error` is set, don't write; set `notice` to say connections can't be saved until
profiles.toml can be read. In `profiles.rs` `reload()`, clear `load_error` after a successful load.

Add a unit test next to existing DbStore tests (pattern: `crates/app/src/db/panel/tests.rs:34`
uses `DbStore::detached(Profiles { items: vec![p] })`): with a temp dir containing an unreadable
`profiles.toml`, a store whose `load_error` is set does not change the file when `save()` runs. If
`save` is private and unreachable from a test module, make the test a `#[cfg(test)] mod` inside
`db/store/mod.rs` or a sibling file.

**Verify**: app tests → pass including the new one.

### Step 5: Accounts refuse to save after a failed load

In `accounts/mod.rs`: add `load_error: Option<String>`; `reload()` sets it on `Err` (and clears it on
`Ok`); `seed_default()` returns early when `load_error` is set; `save()` refuses while it is set
(sets `error` to a message naming accounts.json). `read_usage()`'s successful re-read should clear
`load_error` too (the file became readable). In `crates/core/src/agent_accounts/import.rs:287`,
replace `.unwrap_or_default()` with `?`.

Add a test in `crates/app/src/accounts/tests.rs` if the model can be built there without a GPUI
window (look at how existing tests in that file build it); otherwise add a core test that
`import::apply` returns `Err` and leaves an unreadable `accounts.json` unchanged.

**Verify**: core + app tests → pass.

### Step 6: Lint and limits

**Verify**: clippy → exit 0; line-limit → passes. Only rustfmt files that were rustfmt-clean
before your edit (`rustfmt --edition 2024 --check <file>` before editing).

## Test plan

- instances.rs: `update` refuses an unreadable file and keeps another writer's entry.
- DbStore: no save over an unreadable profiles.toml.
- Accounts (app test or core import test): no save over an unreadable accounts.json.
- Existing tests in all touched modules keep passing.

## Done criteria

- [ ] `grep -rn 'with_extension("toml.tmp")' crates/core/src` → no match
- [ ] `grep -n "unwrap_or_default" crates/app/src/db/store/mod.rs crates/core/src/agent_accounts/import.rs` → no match on the `Profiles::load` / `Accounts::load` lines
- [ ] core and app test commands pass, with ≥ 3 new tests
- [ ] `cargo clippy -p sawit-core -p sawit --all-targets -- -D warnings` exits 0
- [ ] `sh scripts/check-line-limit.sh crates/core crates/app` passes
- [ ] `git status` shows only in-scope files

## STOP conditions

- Excerpts don't match the live code.
- `servers/store.rs` cannot be kept ≤ 500 lines without moving code to a new file (report; don't split on your own).
- A change would need edits in `crates/mcp`.
- Existing tests rely on the empty-list fallback saving over a bad file.

## Maintenance notes

- Any new store that loads a user file must follow the same rule: a load error blocks saving.
- `Registry::update` is the way to change the instance list from any process; MCP's own
  re-read-then-save in `crates/mcp/src/instances/mod.rs` could switch to it later.
- Deferred: an advisory lock around read-modify-write across processes (`crates/core/src/mcp_clients/lock.rs` has one to reuse).
