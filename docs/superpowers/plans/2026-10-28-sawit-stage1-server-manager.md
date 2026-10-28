# Sawit — Spike + Stage 1 (Server Manager) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust workspace with a GPUI↔Tokio↔Postgres integration spike, then a local Postgres server manager (instance list, start/stop/restart + live log, port conflict detection, detected or downloaded binaries).

**Architecture:** The `sawit-core` crate (no GPUI) holds pure parsers (TDD) + modules that run system commands (`lsof`, `ps`, `docker`, `pg_ctl`, `initdb`) asynchronously on Tokio. The `sawit` crate (app) uses `gpui-kit =0.8.1`; all I/O runs through `core::rt::Runtime` (Tokio on its own thread) and results are awaited in `cx.spawn` via `futures` channels.

**Tech Stack:** Rust 1.97 stable, edition 2024, `gpui-kit =0.8.1`, `tokio` 1 (rt-multi-thread, process, fs, time, macros), `tokio-postgres` 0.7.18, `futures` 0.3, `postgresql_archive` 0.22.0 (`default-features = false`, features `theseus`, `tls-rustls-ring`), `serde` + `toml`, `thiserror`, `tempfile` (dev).

**Spec:** `docs/superpowers/specs/2026-10-28-sawit-design.md`

## Global Constraints

- `sawit-core` must not depend (directly/transitively) on any `gpui*` crate. Check: `cargo tree -p sawit-core | grep -i gpui` is empty.
- UI version pinned exactly: `gpui-kit = "=0.8.1"`. Every GPUI/gpui-component API is checked against the source at tag `v0.8.1` (commit `99576db58ec8103417c41a092e891b4022d24e7e`), reference clone: `git clone --branch v0.8.1 https://github.com/longbridge/gpui-component.git`. If an API differs from the plan: stop, record it in `docs/notes/gpui-kit-0.8.1-api.md`, report, adjust.
- The UI thread never runs DB/process/file I/O. Everything goes through `Runtime::spawn`.
- Don't hardcode the Homebrew path; detect it with `brew --prefix`.
- Gate for every task: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` clean, then commit.
- Passwords are never written to a file (Stage 1: managed instances use `trust` auth via socket/localhost only).
- When in doubt about a port conflict: warn (more warnings are better than one missed).

## Review Focus

1. **Docker IPv6 wildcard (`tIPv6`, `n*:5433`)** — on macOS Docker Desktop holds dual-stack; it must be reported as shadowing `127.0.0.1:5433`, not "free". Test in Task 4 (`docker_v6_wildcard_shadows_loopback_v4`).
2. **Own instance already running** — the Start button on an instance that is already running must not report a conflict with itself; it must report "already running". Test in Task 10 (`start_when_running_reports_already_running`).
3. **Data dir paths with spaces** (`~/Library/Application Support/...`) — `pg_ctl -o` and `ps` matching must handle spaces. Test in Task 5 (`sawit_owner_matches_data_dir_with_spaces`) and Task 10 (integration using a tempdir with spaces).
4. **Unix socket path length (max 103 bytes on macOS)** — a socket dir that's too long makes start fail obscurely. Test in Task 9 (`socket_path_too_long_is_rejected`).
5. **`docker` not installed / daemon down / `lsof` returning exit 1 when empty** — must not be treated as an error; the port is still assessed. Test in Task 6 (`missing_docker_cli_still_reports`) and Task 4 (`empty_lsof_output_is_free`).

---

## File Structure

```
Cargo.toml                      workspace, [profile.release] lto="thin", strip=true, codegen-units=1
crates/core/Cargo.toml          sawit-core
crates/core/src/lib.rs          pub mod rt, pgctl, ports, owners, probe, binaries, download, instances, control, error
crates/core/src/rt.rs           Runtime: Tokio on its own thread
crates/core/src/error.rs        CoreError (thiserror)
crates/core/src/pgctl.rs        parse `pg_ctl status` output
crates/core/src/ports.rs        BindAddr, Listener, parse_lsof, analyze → PortVerdict
crates/core/src/owners.rs       Owner, parse_docker_ps, classify
crates/core/src/probe.rs        check_port: run lsof/ps/docker, combine
crates/core/src/binaries.rs     PgVersion, PgBinaries, SearchRoots, discover
crates/core/src/download.rs     install_managed via postgresql_archive
crates/core/src/instances.rs    AppPaths, Instance, ListenScope, Registry (TOML)
crates/core/src/control.rs      init_cluster, status, start, stop, restart, tail_log
crates/core/tests/control_it.rs real integration (skipped when no binary)
crates/app/Cargo.toml           sawit (bin)
crates/app/src/main.rs          bootstrap gpui_kit + Runtime global
crates/app/src/spike.rs         (Task 2; deleted in Task 11)
crates/app/src/servers/mod.rs   ServerManager view (list + detail)
crates/app/src/servers/log_view.rs
crates/app/src/servers/new_instance.rs
docs/notes/gpui-kit-0.8.1-api.md  real API findings
README.md
```

---

### Task 1: Workspace + `core::rt`

**Files:** Create `Cargo.toml`, `.gitignore` (`/target`), `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/rt.rs`, `crates/core/src/error.rs`.

**Interfaces — Produces:**
- `sawit_core::rt::Runtime` — `Runtime::start() -> Runtime` (multi-thread Tokio, 2 workers, thread name `sawit-tokio`); `fn spawn<F, T>(&self, fut: F) -> futures::channel::oneshot::Receiver<T> where F: Future<Output = T> + Send + 'static, T: Send + 'static`; `fn spawn_stream<T: Send + 'static>(&self, f: impl FnOnce(futures::channel::mpsc::UnboundedSender<T>) -> BoxFuture<'static, ()> + Send + 'static) -> futures::channel::mpsc::UnboundedReceiver<T>`; `Runtime: Clone`.
- `sawit_core::error::CoreError` (thiserror) with variants `Io(#[from] std::io::Error)`, `Command { program: String, code: Option<i32>, stderr: String }`, `Parse(String)`, `Config(String)`, `Db(#[from] tokio_postgres::Error)`, `Download(String)`; `pub type Result<T> = std::result::Result<T, CoreError>`.

- [ ] **Step 1: Write failing tests** in `rt.rs`:
```rust
#[test] fn spawn_returns_value_without_blocking_caller_thread() {
    let rt = Runtime::start();
    let rx = rt.spawn(async { tokio::time::sleep(Duration::from_millis(50)).await; 42 });
    assert_eq!(futures::executor::block_on(rx).unwrap(), 42);
}
#[test] fn spawn_stream_delivers_items_in_order() {
    let rt = Runtime::start();
    let rx = rt.spawn_stream(|tx| Box::pin(async move { for i in 0..3 { tx.unbounded_send(i).unwrap(); } }));
    assert_eq!(futures::executor::block_on(rx.collect::<Vec<_>>()), vec![0, 1, 2]);
}
```
- [ ] **Step 2:** `cargo test -p sawit-core rt` → fails to compile (Runtime doesn't exist yet).
- [ ] **Step 3:** Implement. The runtime is built on a dedicated thread that owns the Tokio `Runtime` and parks forever; store the `Handle`.
- [ ] **Step 4:** `cargo test -p sawit-core` → PASS; clippy clean.
- [ ] **Step 5:** Commit `chore: workspace scaffold and tokio runtime bridge`.

### Task 2: Integration spike GPUI + DataTable + `SELECT version()`

Purpose: prove the integration, not a feature. No automated tests; manual verification.

**Files:** Create `crates/app/Cargo.toml` (`gpui-kit = "=0.8.1"`, `sawit-core = { path = "../core" }`, `tokio-postgres`, `futures`), `crates/app/src/main.rs`, `crates/app/src/spike.rs`, `docs/notes/gpui-kit-0.8.1-api.md`.

**Interfaces — Consumes:** `Runtime::spawn`. **Produces:** proven patterns (recorded in notes): how to store `Runtime` as a GPUI global (`impl Global`), the pattern `cx.spawn(async move |this, cx| { let v = rx.await; this.update(cx, |..| ..) })`.

- [ ] **Step 1:** Set up a test DB: `initdb -D <scratch>/spikedb -U postgres -A trust && pg_ctl -D <scratch>/spikedb -o "-p 55432 -k <scratch>" -l <scratch>/spike.log start`.
- [ ] **Step 2:** Window (pattern from `examples/table_in_scrollable` v0.8.1): a "Run" button, a "Run slow" button (`SELECT pg_sleep(3), version()`), a two-column `DataTable` (`name`, `value`) holding the result + duration, a "loading…" status text. URL from env `SAWIT_SPIKE_URL`, default `host=localhost port=55432 user=postgres dbname=postgres`.
- [ ] **Step 3:** `cargo run -p sawit`. Verify: click "Run slow", then for 3 seconds the window can be resized and the "Run" button stays responsive; the `PostgreSQL 16.x` result appears in the table. Take a screenshot.
- [ ] **Step 4:** Write `docs/notes/gpui-kit-0.8.1-api.md`: APIs used, which differ from assumptions, clean build time, debug binary size.
- [ ] **Step 5:** Build/test/clippy gate; commit `spike: gpui-kit window with DataTable and async SELECT version()`. Report findings to the user before continuing.

### Task 3: `pg_ctl status` parser

**Files:** `crates/core/src/pgctl.rs`.

**Interfaces — Produces:** `pub enum PgCtlStatus { Running { pid: u32 }, Stopped, NotACluster, MissingDir }`; `pub fn parse_status(exit_code: i32, output: &str) -> Result<PgCtlStatus>` (output = stdout+stderr combined).

- [ ] **Step 1: Failing tests** (real strings from pg_ctl 16.14):
```rust
assert_eq!(parse_status(0, "pg_ctl: server is running (PID: 1234)\n/opt/x/bin/postgres \"-D\" \"/d\"\n")?, PgCtlStatus::Running { pid: 1234 });
assert_eq!(parse_status(3, "pg_ctl: no server running\n")?, PgCtlStatus::Stopped);
assert_eq!(parse_status(4, "pg_ctl: directory \"/x\" is not a database cluster directory\n")?, PgCtlStatus::NotACluster);
assert_eq!(parse_status(4, "pg_ctl: directory \"/x\" does not exist\n")?, PgCtlStatus::MissingDir);
assert!(parse_status(0, "garbage").is_err());
```
- [ ] **Step 2–4:** Run → FAIL; implement (exit code decides the class, text decides the detail; PID from `(PID: n)`); run → PASS.
- [ ] **Step 5:** Commit `feat(core): parse pg_ctl status output`.

### Task 4: Address model, `lsof` parser, conflict analysis

**Files:** `crates/core/src/ports.rs`.

**Interfaces — Produces:**
- `pub enum BindAddr { AnyV4, AnyV6, LoopbackV4, LoopbackV6, Specific(std::net::IpAddr) }`; `fn families(&self) -> (bool /*v4*/, bool /*v6*/)` — **`AnyV6` is treated as dual-stack (v4+v6)** because lsof doesn't show `IPV6_V6ONLY` (the "in doubt → warn" rule).
- `pub struct Listener { pub pid: u32, pub command: String, pub addr: BindAddr, pub port: u16 }`
- `pub fn parse_lsof(output: &str) -> Vec<Listener>` for the format `lsof -nP -iTCP:<p> -sTCP:LISTEN -F pctn` (lines `p<pid>`, `c<cmd>`, then per fd `f..`, `t<IPv4|IPv6>`, `n<addr>:<port>`; `*` + `tIPv4` → AnyV4, `*` + `tIPv6` → AnyV6, `[::1]` → LoopbackV6).
- `pub enum PortVerdict { Free, Conflict { same_addr: Vec<Listener>, shadowing: Vec<Listener> }, Shadowing { listeners: Vec<Listener> } }`
- `pub fn analyze(port: u16, intended: &[BindAddr], listeners: &[Listener]) -> PortVerdict` — rules: for each pair (intended A, listener L) with the same port and overlapping families: `A == L` → same_addr; one is a wildcard and the other isn't the same wildcard → shadowing. Any same_addr → `Conflict`; else any shadowing → `Shadowing`; else `Free`. Listeners with non-overlapping families are ignored.

- [ ] **Step 1: Failing tests**, including real fixtures from this machine:
```rust
const DOCKER: &str = "p14087\nccom.docker.backend\nf74\ntIPv6\nn*:5433\n";
const MIXED: &str = "p58423\ncPython\nf3\ntIPv4\nn127.0.0.1:57360\nf4\ntIPv6\nn*:57360\nf5\ntIPv4\nn*:57360\n";
#[test] fn parses_docker_v6_wildcard() { /* 1 listener, pid 14087, AnyV6, port 5433 */ }
#[test] fn parses_mixed_fds_of_one_process() { /* 3 listeners: LoopbackV4, AnyV6, AnyV4 */ }
#[test] fn empty_lsof_output_is_free() { assert_eq!(analyze(5433, &[BindAddr::LoopbackV4], &parse_lsof("")), PortVerdict::Free); }
#[test] fn docker_v6_wildcard_shadows_loopback_v4() { /* intended [LoopbackV4, LoopbackV6] vs DOCKER → Shadowing */ }
#[test] fn same_loopback_is_conflict() { /* intended LoopbackV4 vs listener LoopbackV4 → Conflict{same_addr:1} */ }
#[test] fn wildcard_intent_vs_loopback_listener_shadows() { /* intended AnyV4 vs LoopbackV4 → Shadowing */ }
#[test] fn v6_loopback_listener_ignored_for_v4_only_intent() { /* intended LoopbackV4 vs LoopbackV6 → Free */ }
#[test] fn parses_ipv6_loopback_brackets() { /* "tIPv6\nn[::1]:5432" → LoopbackV6 */ }
```
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Commit `feat(core): lsof parsing and port conflict analysis`.

### Task 5: Port owner identification

**Files:** `crates/core/src/owners.rs`.

**Interfaces — Consumes:** `BindAddr`, `Listener`. **Produces:**
- `pub struct DockerPort { pub container: String, pub host: BindAddr, pub host_port: u16 }`; `pub fn parse_docker_ps(output: &str) -> Vec<DockerPort>` for `docker ps --format '{{.Names}}\t{{.Ports}}'`.
- `pub enum Owner { Docker { containers: Vec<String> }, Homebrew { formula: Option<String> }, PostgresApp, Sawit { instance: String }, Other }`
- `pub struct OwnerContext { pub brew_prefix: Option<PathBuf>, pub docker_ports: Vec<DockerPort>, pub managed: Vec<(String, PathBuf)> /*name, data_dir*/ }`
- `pub fn classify(listener: &Listener, command_line: &str, ctx: &OwnerContext) -> Owner` — order: Sawit (command_line contains the instance's data_dir, quoted or not) → Docker (command starts with `com.docker`, `vpnkit`, `docker-proxy`, or `OrbStack`; containers = docker_ports with the same host_port) → Homebrew (command_line starts with brew_prefix; formula from the segment after `Cellar/` or `opt/`) → PostgresApp (contains `/Postgres.app/`) → Other.

- [ ] **Step 1: Failing tests:**
```rust
const PS: &str = "kyzn-crm-postgres\t127.0.0.1:5434->5432/tcp\nrekamedika-postgres\t0.0.0.0:5433->5432/tcp, [::]:5433->5432/tcp\n";
#[test] fn parses_docker_ps_multi_mapping() { /* 3 DockerPort; rekamedika: AnyV4:5433 and AnyV6:5433 */ }
#[test] fn docker_backend_named_by_container() { /* command "com.docker.backend" port 5433 → Docker{containers:["rekamedika-postgres"]} */ }
#[test] fn homebrew_formula_from_cellar_path() { /* brew_prefix /opt/homebrew, cmdline "/opt/homebrew/Cellar/postgresql@16/16.14/bin/postgres -D /opt/homebrew/var/postgresql@16" → Homebrew{formula:Some("postgresql@16")} */ }
#[test] fn sawit_owner_matches_data_dir_with_spaces() { /* data_dir "/Users/u/Library/Application Support/sawit/clusters/app", cmdline contains that path → Sawit{instance:"app"} */ }
#[test] fn postgres_app_detected() {}
#[test] fn unknown_is_other() {}
#[test] fn no_brew_prefix_never_homebrew() {}
```
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Commit `feat(core): identify port owners (docker, homebrew, postgres.app, sawit)`.

### Task 6: `probe::check_port` (runs system commands)

**Files:** `crates/core/src/probe.rs`.

**Interfaces — Consumes:** Task 4, 5. **Produces:**
- `pub struct OwnedListener { pub listener: Listener, pub owner: Owner, pub command_line: String }`
- `pub struct PortReport { pub port: u16, pub verdict: PortVerdict, pub owners: Vec<OwnedListener> }` + `fn explain(&self) -> String` (an English sentence naming the owner, the address, and—for Shadowing—"connections to 127.0.0.1 will reach X, connections via other addresses will reach Y").
- `pub async fn check_port(port: u16, intended: &[BindAddr], managed: Vec<(String, PathBuf)>) -> Result<PortReport>` — `lsof` exit 1 + empty output = no listener; `ps -o command= -p <pid>` per PID; `docker ps` only when there's a Docker-type owner and `docker` is on PATH, 3-second timeout, failure → empty `docker_ports` (not an error). `brew --prefix` is cached in a `OnceCell`.

- [ ] **Step 1: Failing tests** (`#[tokio::test]`):
  - `own_listener_detected_as_conflict`: bind a `std::net::TcpListener` to `127.0.0.1:0`; `check_port(p, &[LoopbackV4], vec![])` → `Conflict`, owners include the test process PID (`std::process::id()`), owner `Other`.
  - `free_port_reports_free`: grab a free port (bind then drop) → `Free`, owners empty.
  - `missing_docker_cli_still_reports`: run the core with a PATH without `docker` (internal function `check_port_with(env: &ProbeEnv, ..)` that takes program names; `ProbeEnv { docker: Option<String>, ... }`) → still returns a verdict.
  - `explain_mentions_both_sides_for_shadowing`: build a `PortReport` by hand (Docker AnyV6 vs intent LoopbackV4) → text contains `rekamedika-postgres` and `127.0.0.1`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Commit `feat(core): probe listening ports with owner details`.

### Task 7: Installed binary discovery

**Files:** `crates/core/src/binaries.rs`.

**Interfaces — Produces:**
- `pub struct PgVersion { pub major: u32, pub minor: u32 }` (Display `16.14`); `pub fn parse_version(output: &str) -> Option<PgVersion>` for `pg_ctl (PostgreSQL) 16.14 (Homebrew)` and `pg_ctl (PostgreSQL) 18.0`.
- `pub enum BinarySource { Homebrew, PostgresApp, Path, Managed }`
- `pub struct PgBinaries { pub bin_dir: PathBuf, pub version: PgVersion, pub source: BinarySource }`; `fn tool(&self, name: &str) -> PathBuf`.
- `pub struct SearchRoots { pub brew_prefix: Option<PathBuf>, pub postgres_app: PathBuf, pub path_dirs: Vec<PathBuf>, pub managed: PathBuf }`; `pub async fn SearchRoots::detect(managed: PathBuf) -> SearchRoots` (brew via `brew --prefix` when `brew` is on PATH).
- `pub fn candidate_bin_dirs(roots: &SearchRoots) -> Vec<(PathBuf, BinarySource)>` — `<brew>/opt/postgresql*/bin`, `<postgres_app>/Contents/Versions/*/bin`, `<managed>/*/bin`, then each PATH dir containing `pg_ctl`; only dirs that have both `pg_ctl` and `initdb`; dedup via `canonicalize` (first source wins).
- `pub async fn discover(roots: &SearchRoots) -> Vec<PgBinaries>` — run `pg_ctl --version`, sort by version descending.

- [ ] **Step 1: Failing tests:** `parse_version` (2 examples + `None` for garbage); `candidate_bin_dirs` with a fake tree in `tempfile::TempDir` (empty `pg_ctl` + `initdb` files): brew `opt/postgresql@16/bin` and `opt/postgresql@17/bin`, Postgres.app `Versions/18/bin`, managed `16.4.0/bin`, a PATH dir that is a symlink to a brew dir → 4 entries, the symlink isn't duplicated, a dir without `initdb` is ignored; `discover_finds_real_homebrew` marked `#[ignore]` (run manually on this machine, must find 16.14).
- [ ] **Step 2–4:** FAIL → implement → PASS (`cargo test -p sawit-core -- --include-ignored binaries` also passes on this machine).
- [ ] **Step 5:** Commit `feat(core): discover installed PostgreSQL binaries`.

### Task 8: Download managed binaries

**Files:** `crates/core/src/download.rs`; `crates/core/Cargo.toml` (`postgresql_archive = { version = "=0.22.0", default-features = false, features = ["theseus", "tls-rustls-ring"] }`).

**Interfaces — Consumes:** `PgBinaries`, `parse_version`. **Produces:** `pub async fn install_managed(version_req: &str, managed_root: &Path) -> Result<PgBinaries>` — `get_archive(postgresql_archive::configuration::theseus::URL, &VersionReq::parse(version_req)?)`, extract to `<managed_root>/.staging-<version>` then atomic `rename` to `<managed_root>/<version>`; if it already exists, return without downloading; verify with `pg_ctl --version`. Before writing, check module/constant names against the `postgresql_archive` v0.22.0 source.

- [ ] **Step 1: Failing tests:** `existing_install_is_reused_without_network` (fake tree `<root>/16.4.0/bin/{pg_ctl,initdb}` with `pg_ctl` a shell script printing `pg_ctl (PostgreSQL) 16.4`, chmod +x → result `Managed`, version 16.4); `downloads_real_binaries` `#[ignore]` (`"=16"` into a tempdir, then `initdb --version` runs).
- [ ] **Step 2–4:** FAIL → implement → PASS, including running the ignored test once manually; record download size & time in notes.
- [ ] **Step 5:** Commit `feat(core): download managed PostgreSQL binaries via postgresql_archive`.

### Task 9: Instance registry & app paths

**Files:** `crates/core/src/instances.rs`.

**Interfaces — Produces:**
- `pub struct AppPaths { pub root: PathBuf }` — `default_macos() -> AppPaths` (`$HOME/Library/Application Support/sawit`), `instances_file()`, `cluster_dir(name)`, `log_file(name)` (`logs/<name>.log`), `socket_dir()` (`run`), `managed_bin_root()` (`bin`).
- `pub enum ListenScope { Localhost, All }` — `bind_addrs() -> Vec<BindAddr>` (Localhost → `[LoopbackV4, LoopbackV6]`, All → `[AnyV4, AnyV6]`), `listen_addresses() -> &'static str` (`"localhost"` / `"*"`).
- `#[derive(Serialize, Deserialize)] pub struct Instance { pub name: String, pub bin_dir: PathBuf, pub data_dir: PathBuf, pub port: u16, pub listen: ListenScope, pub managed: bool }`
- `pub struct Registry { pub instances: Vec<Instance> }` — `load(path) -> Result<Registry>` (missing file → empty), `save(&self, path)` (write to `.tmp` then rename), `add(&mut self, inst) -> Result<()>`, `remove(&mut self, name)`, `get(&self, name) -> Option<&Instance>`, `port_clashes(&self) -> Vec<(String, String, u16)>`.
- `pub fn validate_name(name: &str) -> Result<()>` — `^[a-z0-9_-]{1,40}$`.
- `pub fn validate_socket_dir(dir: &Path, port: u16) -> Result<()>` — length of `"<dir>/.s.PGSQL.<port>"` ≤ 103 bytes.

- [ ] **Step 1: Failing tests:** `roundtrip_toml`; `missing_file_loads_empty`; `duplicate_name_rejected`; `duplicate_data_dir_rejected`; `same_port_listed_in_port_clashes` (two instances on port 5433 → one tuple); `invalid_names_rejected` (`"My DB"`, `""`, 41 characters); `socket_path_too_long_is_rejected` (100-character dir → Err; `/Users/rulisme/Library/Application Support/sawit/run` → Ok).
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Commit `feat(core): instance registry and app paths`.

### Task 10: Process control & log tail

**Files:** `crates/core/src/control.rs`, `crates/core/tests/control_it.rs`.

**Interfaces — Consumes:** `Instance`, `AppPaths`, `PgCtlStatus`, `check_port`, `PortReport`. **Produces:**
- `pub async fn init_cluster(inst: &Instance) -> Result<()>` — `initdb -D <data> -U postgres -A trust -E UTF8 --locale=C`.
- `pub async fn status(inst: &Instance) -> Result<PgCtlStatus>`
- `pub enum Preflight { Check, Force }`
- `pub enum StartOutcome { Started, AlreadyRunning { pid: u32 }, Blocked(PortReport) }`
- `pub async fn start(inst: &Instance, paths: &AppPaths, managed: Vec<(String, PathBuf)>, preflight: Preflight) -> Result<StartOutcome>` — order: `status` → Running → `AlreadyRunning` (no port check); `check_port` (listeners owned by this instance are ignored) → verdict ≠ Free and `Check` → `Blocked`; then `pg_ctl start -w -t 30 -D <data> -l <log> -o "<opts>"` with opts = `-p <port> -c listen_addresses=<..> -k '<socket_dir>'` (path single-quoted; reject paths containing `'`).
- `pub async fn stop(inst: &Instance) -> Result<()>` (`pg_ctl stop -w -m fast`; Stopped → Ok), `pub async fn restart(..) -> Result<StartOutcome>` (stop then start with the same preflight).
- `pub async fn tail_log(path: PathBuf, tx: UnboundedSender<String>)` — start from the last 64 KiB, poll every 250 ms, send line by line, stop when `tx` is closed; tolerate a missing / truncated file (rotation).

- [ ] **Step 1: Failing tests** — unit: `tail_log_emits_appended_lines` (tempfile, append 2 lines, receive 2 lines), `tail_log_waits_for_missing_file`. Integration `control_it.rs` (skip with `eprintln!("skip: no binaries")` when `discover` is empty; tempdir named `"sawit dev it"` with spaces; random free port):
  - `lifecycle_init_start_query_stop`: init → status `Stopped` → start `Started` → `tokio_postgres` `SELECT 1` via localhost → stop → `Stopped`.
  - `start_when_running_reports_already_running`.
  - `start_blocked_by_foreign_listener`: bind `127.0.0.1:<port>` from the test → `Blocked(report)` with `Conflict`; with `Force` → start is attempted (any result is fine, no panic).
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Commit `feat(core): pg_ctl lifecycle with port preflight and live log tail`.

### Task 11: Server manager UI

**Files:** `crates/app/src/main.rs`, `crates/app/src/servers/{mod.rs,log_view.rs,new_instance.rs}`; delete `spike.rs`.

**Interfaces — Consumes:** all the core APIs above (only through `Runtime`). gpui-component components are chosen after reading the v0.8.1 source (`table`, `button`, `dialog`, `input`, `select`, `notification`, `scroll`, `resizable`); record every difference in notes.

Required behavior:
- On open: load `Registry` + `discover` in the background; instance list (name, version, port, data dir, status) with status refreshed every 3 seconds in the background.
- Instance detail: Start/Stop/Restart buttons (disabled while an operation is running), live log (`tail_log` via `spawn_stream`, at most 5,000 lines in memory, auto-scroll to the bottom).
- A `Blocked` start → dialog with `PortReport::explain()` and the owner list (PID, command, container), "Cancel" and "Start anyway" buttons; for `Conflict` there is no "Start anyway" button.
- "New instance": name, port (default: the first port ≥ 5432 that is `Free` per `check_port`), binary (`discover` list + a "Download PostgreSQL 16/17/18" option via `install_managed`, indeterminate progress), listen scope → `init_cluster` → save Registry.
- "Add existing data dir" (adopt a Homebrew cluster etc.): pick a folder (`cx.prompt_for_paths` or its v0.8.1 equivalent), validate `status ≠ NotACluster`, port read from `postgresql.conf` if present (`port = N`), else 5432.
- A warning in the list when `port_clashes` is not empty.

- [ ] **Step 1:** Implement; every UI callback that touches core calls `Runtime::spawn` and updates the entity in `cx.spawn`.
- [ ] **Step 2:** Manual verification on this machine (step script in notes): create instance `demo` on port 5433 → the dialog must name `rekamedika-postgres` (Docker, `*:5433`) and explain the shadowing; recreate it on a free port → start → log appears live → stop → restart; the window stays responsive while downloading a binary. Screenshot each step.
- [ ] **Step 3:** Build/test/clippy gate; `cargo tree -p sawit-core | grep -i gpui` empty.
- [ ] **Step 4:** Commit `feat(app): server manager with port conflict dialog and live logs`.

### Task 12: README, size & memory footprint

**Files:** `README.md`, `Cargo.toml` (release profile).

- [ ] **Step 1:** `cargo build --release -p sawit`; record the binary size (`ls -lh target/release/sawit`) and idle RSS 10 seconds after opening (`ps -o rss= -p <pid>`); target < 150 MB.
- [ ] **Step 2:** README: prerequisites (Rust stable; no full Xcode needed thanks to `runtime_shaders`; no Docker/Homebrew needed when using downloaded binaries), `cargo run -p sawit`, running tests (including `--include-ignored`, which needs network/binaries), architecture decisions (table from the spec + spike findings), measured size/RSS numbers, map of stages 2–5.
- [ ] **Step 3:** Build/test/clippy gate; commit `docs: README with build, run, architecture decisions`.

---

## After this plan

Write the Stage 2 plan (connections + Keychain + `.env` + schema tree) using real findings from `docs/notes/gpui-kit-0.8.1-api.md`, then Stage 3 (virtual grid: `TableDelegate::visible_rows_changed`/`load_more` + server cursor), Stage 4 (SQL editor), Stage 5 (MCP `rmcp` + write `DATABASE_URL` to the TanStack project's `.env`).
