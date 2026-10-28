# PG Desk — Spike + Tahap 1 (Pengelola Server) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Workspace Rust dengan spike integrasi GPUI↔Tokio↔Postgres, lalu pengelola server Postgres lokal (daftar instance, start/stop/restart + log live, deteksi konflik port, binary terdeteksi atau diunduh).

**Architecture:** Crate `pgdesk-core` (tanpa GPUI) berisi parser murni (TDD) + modul yang menjalankan perintah sistem (`lsof`, `ps`, `docker`, `pg_ctl`, `initdb`) secara async di Tokio. Crate `pgdesk` (app) memakai `gpui-kit =0.8.1`; semua I/O dijalankan lewat `core::rt::Runtime` (Tokio di thread sendiri) dan hasilnya di-await di `cx.spawn` via channel `futures`.

**Tech Stack:** Rust 1.97 stable, edition 2024, `gpui-kit =0.8.1`, `tokio` 1 (rt-multi-thread, process, fs, time, macros), `tokio-postgres` 0.7.18, `futures` 0.3, `postgresql_archive` 0.22.0 (`default-features = false`, features `theseus`, `tls-rustls-ring`), `serde` + `toml`, `thiserror`, `tempfile` (dev).

**Spec:** `docs/superpowers/specs/2026-10-28-pgdesk-design.md`

## Global Constraints

- `pgdesk-core` tidak boleh bergantung (langsung/transitif) pada crate `gpui*`. Cek: `cargo tree -p pgdesk-core | grep -i gpui` kosong.
- Versi UI dikunci persis: `gpui-kit = "=0.8.1"`. Setiap API GPUI/gpui-component dicocokkan dengan source tag `v0.8.1` (commit `99576db58ec8103417c41a092e891b4022d24e7e`), clone referensi: `git clone --branch v0.8.1 https://github.com/longbridge/gpui-component.git`. Bila API berbeda dari rencana: hentikan, catat di `docs/notes/gpui-kit-0.8.1-api.md`, laporkan, sesuaikan.
- Thread UI tidak pernah menjalankan I/O DB/proses/file. Semua lewat `Runtime::spawn`.
- Jangan hardcode path Homebrew; deteksi dengan `brew --prefix`.
- Gerbang tiap task: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` bersih, lalu commit.
- Password tidak pernah ditulis ke file (Tahap 1: instance terkelola memakai auth `trust` via socket/localhost saja).
- Bila ragu soal konflik port: peringatkan (lebih banyak peringatan lebih baik daripada satu yang terlewat).

## Review Focus

1. **Docker IPv6 wildcard (`tIPv6`, `n*:5433`)** — di macOS Docker Desktop memegang dual-stack; harus dilaporkan sebagai membayangi `127.0.0.1:5433`, bukan "bebas". Test di Task 4 (`docker_v6_wildcard_shadows_loopback_v4`).
2. **Instance milik sendiri yang sudah jalan** — tombol Start pada instance yang sudah running tidak boleh melaporkan konflik dengan dirinya sendiri; harus melapor "sudah berjalan". Test di Task 10 (`start_when_running_reports_already_running`).
3. **Path data dir dengan spasi** (`~/Library/Application Support/...`) — `pg_ctl -o` dan pencocokan `ps` harus tahan spasi. Test di Task 5 (`pgdesk_owner_matches_data_dir_with_spaces`) dan Task 10 (integrasi memakai tempdir berspasi).
4. **Panjang path socket Unix (maks 103 byte di macOS)** — socket dir yang terlalu panjang membuat start gagal samar. Test di Task 9 (`socket_path_too_long_is_rejected`).
5. **`docker` tidak terpasang / daemon mati / `lsof` mengembalikan exit 1 saat kosong** — tidak boleh dianggap error; port tetap dinilai. Test di Task 6 (`missing_docker_cli_still_reports`) dan Task 4 (`empty_lsof_output_is_free`).

---

## File Structure

```
Cargo.toml                      workspace, [profile.release] lto="thin", strip=true, codegen-units=1
crates/core/Cargo.toml          pgdesk-core
crates/core/src/lib.rs          pub mod rt, pgctl, ports, owners, probe, binaries, download, instances, control, error
crates/core/src/rt.rs           Runtime: Tokio di thread sendiri
crates/core/src/error.rs        CoreError (thiserror)
crates/core/src/pgctl.rs        parse output `pg_ctl status`
crates/core/src/ports.rs        BindAddr, Listener, parse_lsof, analyze → PortVerdict
crates/core/src/owners.rs       Owner, parse_docker_ps, classify
crates/core/src/probe.rs        check_port: jalankan lsof/ps/docker, gabungkan
crates/core/src/binaries.rs     PgVersion, PgBinaries, SearchRoots, discover
crates/core/src/download.rs     install_managed via postgresql_archive
crates/core/src/instances.rs    AppPaths, Instance, ListenScope, Registry (TOML)
crates/core/src/control.rs      init_cluster, status, start, stop, restart, tail_log
crates/core/tests/control_it.rs integrasi nyata (lewati bila tidak ada binary)
crates/app/Cargo.toml           pgdesk (bin)
crates/app/src/main.rs          bootstrap gpui_kit + Runtime global
crates/app/src/spike.rs         (Task 2; dihapus di Task 11)
crates/app/src/servers/mod.rs   ServerManager view (daftar + detail)
crates/app/src/servers/log_view.rs
crates/app/src/servers/new_instance.rs
docs/notes/gpui-kit-0.8.1-api.md  temuan API nyata
README.md
```

---

### Task 1: Workspace + `core::rt`

**Files:** Create `Cargo.toml`, `.gitignore` (`/target`), `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/rt.rs`, `crates/core/src/error.rs`.

**Interfaces — Produces:**
- `pgdesk_core::rt::Runtime` — `Runtime::start() -> Runtime` (Tokio multi-thread, 2 worker, thread nama `pgdesk-tokio`); `fn spawn<F, T>(&self, fut: F) -> futures::channel::oneshot::Receiver<T> where F: Future<Output = T> + Send + 'static, T: Send + 'static`; `fn spawn_stream<T: Send + 'static>(&self, f: impl FnOnce(futures::channel::mpsc::UnboundedSender<T>) -> BoxFuture<'static, ()> + Send + 'static) -> futures::channel::mpsc::UnboundedReceiver<T>`; `Runtime: Clone`.
- `pgdesk_core::error::CoreError` (thiserror) dengan varian `Io(#[from] std::io::Error)`, `Command { program: String, code: Option<i32>, stderr: String }`, `Parse(String)`, `Config(String)`, `Db(#[from] tokio_postgres::Error)`, `Download(String)`; `pub type Result<T> = std::result::Result<T, CoreError>`.

- [ ] **Step 1: Write failing tests** di `rt.rs`:
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
- [ ] **Step 2:** `cargo test -p pgdesk-core rt` → gagal kompilasi (Runtime belum ada).
- [ ] **Step 3:** Implement. Runtime dibangun di thread khusus yang memegang `Runtime` Tokio dan parkir selamanya; simpan `Handle`.
- [ ] **Step 4:** `cargo test -p pgdesk-core` → PASS; clippy bersih.
- [ ] **Step 5:** Commit `chore: workspace scaffold and tokio runtime bridge`.

### Task 2: Spike integrasi GPUI + DataTable + `SELECT version()`

Tujuan: membuktikan integrasi, bukan fitur. Tanpa test otomatis; verifikasi manual.

**Files:** Create `crates/app/Cargo.toml` (`gpui-kit = "=0.8.1"`, `pgdesk-core = { path = "../core" }`, `tokio-postgres`, `futures`), `crates/app/src/main.rs`, `crates/app/src/spike.rs`, `docs/notes/gpui-kit-0.8.1-api.md`.

**Interfaces — Consumes:** `Runtime::spawn`. **Produces:** pola yang terbukti (dicatat di notes): cara menyimpan `Runtime` sebagai global GPUI (`impl Global`), pola `cx.spawn(async move |this, cx| { let v = rx.await; this.update(cx, |..| ..) })`.

- [ ] **Step 1:** Siapkan DB uji: `initdb -D <scratch>/spikedb -U postgres -A trust && pg_ctl -D <scratch>/spikedb -o "-p 55432 -k <scratch>" -l <scratch>/spike.log start`.
- [ ] **Step 2:** Window (pola `examples/table_in_scrollable` v0.8.1): tombol "Run", tombol "Run slow" (`SELECT pg_sleep(3), version()`), `DataTable` dua kolom (`name`, `value`) berisi hasil + durasi, teks status "loading…". URL dari env `PGDESK_SPIKE_URL`, default `host=localhost port=55432 user=postgres dbname=postgres`.
- [ ] **Step 3:** `cargo run -p pgdesk`. Verifikasi: klik "Run slow", lalu selama 3 detik jendela bisa di-resize dan tombol "Run" tetap responsif; hasil `PostgreSQL 16.x` muncul di tabel. Ambil screenshot.
- [ ] **Step 4:** Tulis `docs/notes/gpui-kit-0.8.1-api.md`: API yang dipakai, mana yang berbeda dari dugaan, waktu build bersih, ukuran biner debug.
- [ ] **Step 5:** Gerbang build/test/clippy; commit `spike: gpui-kit window with DataTable and async SELECT version()`. Laporkan temuan ke pengguna sebelum lanjut.

### Task 3: Parser `pg_ctl status`

**Files:** `crates/core/src/pgctl.rs`.

**Interfaces — Produces:** `pub enum PgCtlStatus { Running { pid: u32 }, Stopped, NotACluster, MissingDir }`; `pub fn parse_status(exit_code: i32, output: &str) -> Result<PgCtlStatus>` (output = stdout+stderr digabung).

- [ ] **Step 1: Failing tests** (string nyata dari pg_ctl 16.14):
```rust
assert_eq!(parse_status(0, "pg_ctl: server is running (PID: 1234)\n/opt/x/bin/postgres \"-D\" \"/d\"\n")?, PgCtlStatus::Running { pid: 1234 });
assert_eq!(parse_status(3, "pg_ctl: no server running\n")?, PgCtlStatus::Stopped);
assert_eq!(parse_status(4, "pg_ctl: directory \"/x\" is not a database cluster directory\n")?, PgCtlStatus::NotACluster);
assert_eq!(parse_status(4, "pg_ctl: directory \"/x\" does not exist\n")?, PgCtlStatus::MissingDir);
assert!(parse_status(0, "garbage").is_err());
```
- [ ] **Step 2–4:** Run → FAIL; implement (exit code menentukan kelas, teks menentukan detail; PID dari `(PID: n)`); run → PASS.
- [ ] **Step 5:** Commit `feat(core): parse pg_ctl status output`.

### Task 4: Model alamat, parser `lsof`, analisis konflik

**Files:** `crates/core/src/ports.rs`.

**Interfaces — Produces:**
- `pub enum BindAddr { AnyV4, AnyV6, LoopbackV4, LoopbackV6, Specific(std::net::IpAddr) }`; `fn families(&self) -> (bool /*v4*/, bool /*v6*/)` — **`AnyV6` dianggap dual-stack (v4+v6)** karena lsof tidak menampilkan `IPV6_V6ONLY` (aturan "ragu → peringatkan").
- `pub struct Listener { pub pid: u32, pub command: String, pub addr: BindAddr, pub port: u16 }`
- `pub fn parse_lsof(output: &str) -> Vec<Listener>` untuk format `lsof -nP -iTCP:<p> -sTCP:LISTEN -F pctn` (baris `p<pid>`, `c<cmd>`, lalu per fd `f..`, `t<IPv4|IPv6>`, `n<addr>:<port>`; `*` + `tIPv4` → AnyV4, `*` + `tIPv6` → AnyV6, `[::1]` → LoopbackV6).
- `pub enum PortVerdict { Free, Conflict { same_addr: Vec<Listener>, shadowing: Vec<Listener> }, Shadowing { listeners: Vec<Listener> } }`
- `pub fn analyze(port: u16, intended: &[BindAddr], listeners: &[Listener]) -> PortVerdict` — aturan: untuk tiap pasangan (intended A, listener L) dengan port sama dan family beririsan: `A == L` → same_addr; salah satu wildcard dan yang lain bukan wildcard yang sama → shadowing. Ada same_addr → `Conflict`; else ada shadowing → `Shadowing`; else `Free`. Listener dengan family tak beririsan diabaikan.

- [ ] **Step 1: Failing tests**, termasuk fixture nyata dari mesin ini:
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

### Task 5: Identifikasi pemilik port

**Files:** `crates/core/src/owners.rs`.

**Interfaces — Consumes:** `BindAddr`, `Listener`. **Produces:**
- `pub struct DockerPort { pub container: String, pub host: BindAddr, pub host_port: u16 }`; `pub fn parse_docker_ps(output: &str) -> Vec<DockerPort>` untuk `docker ps --format '{{.Names}}\t{{.Ports}}'`.
- `pub enum Owner { Docker { containers: Vec<String> }, Homebrew { formula: Option<String> }, PostgresApp, PgDesk { instance: String }, Other }`
- `pub struct OwnerContext { pub brew_prefix: Option<PathBuf>, pub docker_ports: Vec<DockerPort>, pub managed: Vec<(String, PathBuf)> /*nama, data_dir*/ }`
- `pub fn classify(listener: &Listener, command_line: &str, ctx: &OwnerContext) -> Owner` — urutan: PgDesk (command_line memuat data_dir instance, baik dikutip maupun tidak) → Docker (command diawali `com.docker`, `vpnkit`, `docker-proxy`, atau `OrbStack`; containers = docker_ports dengan host_port sama) → Homebrew (command_line diawali brew_prefix; formula dari segmen setelah `Cellar/` atau `opt/`) → PostgresApp (memuat `/Postgres.app/`) → Other.

- [ ] **Step 1: Failing tests:**
```rust
const PS: &str = "kyzn-crm-postgres\t127.0.0.1:5434->5432/tcp\nrekamedika-postgres\t0.0.0.0:5433->5432/tcp, [::]:5433->5432/tcp\n";
#[test] fn parses_docker_ps_multi_mapping() { /* 3 DockerPort; rekamedika: AnyV4:5433 dan AnyV6:5433 */ }
#[test] fn docker_backend_named_by_container() { /* command "com.docker.backend" port 5433 → Docker{containers:["rekamedika-postgres"]} */ }
#[test] fn homebrew_formula_from_cellar_path() { /* brew_prefix /opt/homebrew, cmdline "/opt/homebrew/Cellar/postgresql@16/16.14/bin/postgres -D /opt/homebrew/var/postgresql@16" → Homebrew{formula:Some("postgresql@16")} */ }
#[test] fn pgdesk_owner_matches_data_dir_with_spaces() { /* data_dir "/Users/u/Library/Application Support/pgdesk/clusters/app", cmdline memuat path itu → PgDesk{instance:"app"} */ }
#[test] fn postgres_app_detected() {}
#[test] fn unknown_is_other() {}
#[test] fn no_brew_prefix_never_homebrew() {}
```
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Commit `feat(core): identify port owners (docker, homebrew, postgres.app, pgdesk)`.

### Task 6: `probe::check_port` (menjalankan perintah sistem)

**Files:** `crates/core/src/probe.rs`.

**Interfaces — Consumes:** Task 4, 5. **Produces:**
- `pub struct OwnedListener { pub listener: Listener, pub owner: Owner, pub command_line: String }`
- `pub struct PortReport { pub port: u16, pub verdict: PortVerdict, pub owners: Vec<OwnedListener> }` + `fn explain(&self) -> String` (kalimat bahasa Indonesia yang menyebut pemilik, alamat, dan—untuk Shadowing—"koneksi ke 127.0.0.1 akan sampai ke X, koneksi lewat alamat lain ke Y").
- `pub async fn check_port(port: u16, intended: &[BindAddr], managed: Vec<(String, PathBuf)>) -> Result<PortReport>` — `lsof` exit 1 + output kosong = tidak ada listener; `ps -o command= -p <pid>` per PID; `docker ps` hanya bila ada owner bertipe Docker dan `docker` ada di PATH, timeout 3 detik, gagal → `docker_ports` kosong (bukan error). `brew --prefix` di-cache dalam `OnceCell`.

- [ ] **Step 1: Failing tests** (`#[tokio::test]`):
  - `own_listener_detected_as_conflict`: bind `std::net::TcpListener` ke `127.0.0.1:0`; `check_port(p, &[LoopbackV4], vec![])` → `Conflict`, owners memuat PID proses test (`std::process::id()`), owner `Other`.
  - `free_port_reports_free`: ambil port bebas (bind lalu drop) → `Free`, owners kosong.
  - `missing_docker_cli_still_reports`: jalankan inti dengan PATH tanpa `docker` (fungsi internal `check_port_with(env: &ProbeEnv, ..)` yang menerima nama program; `ProbeEnv { docker: Option<String>, ... }`) → tetap mengembalikan verdict.
  - `explain_mentions_both_sides_for_shadowing`: susun `PortReport` manual (Docker AnyV6 vs intent LoopbackV4) → teks memuat `rekamedika-postgres` dan `127.0.0.1`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Commit `feat(core): probe listening ports with owner details`.

### Task 7: Penemuan binary terpasang

**Files:** `crates/core/src/binaries.rs`.

**Interfaces — Produces:**
- `pub struct PgVersion { pub major: u32, pub minor: u32 }` (Display `16.14`); `pub fn parse_version(output: &str) -> Option<PgVersion>` untuk `pg_ctl (PostgreSQL) 16.14 (Homebrew)` dan `pg_ctl (PostgreSQL) 18.0`.
- `pub enum BinarySource { Homebrew, PostgresApp, Path, Managed }`
- `pub struct PgBinaries { pub bin_dir: PathBuf, pub version: PgVersion, pub source: BinarySource }`; `fn tool(&self, name: &str) -> PathBuf`.
- `pub struct SearchRoots { pub brew_prefix: Option<PathBuf>, pub postgres_app: PathBuf, pub path_dirs: Vec<PathBuf>, pub managed: PathBuf }`; `pub async fn SearchRoots::detect(managed: PathBuf) -> SearchRoots` (brew via `brew --prefix` bila `brew` di PATH).
- `pub fn candidate_bin_dirs(roots: &SearchRoots) -> Vec<(PathBuf, BinarySource)>` — `<brew>/opt/postgresql*/bin`, `<postgres_app>/Contents/Versions/*/bin`, `<managed>/*/bin`, lalu tiap dir PATH yang memuat `pg_ctl`; hanya dir yang punya `pg_ctl` dan `initdb`; dedup via `canonicalize` (sumber pertama menang).
- `pub async fn discover(roots: &SearchRoots) -> Vec<PgBinaries>` — jalankan `pg_ctl --version`, urut versi menurun.

- [ ] **Step 1: Failing tests:** `parse_version` (2 contoh + `None` untuk sampah); `candidate_bin_dirs` dengan pohon palsu di `tempfile::TempDir` (file `pg_ctl` + `initdb` kosong): ada brew `opt/postgresql@16/bin` dan `opt/postgresql@17/bin`, Postgres.app `Versions/18/bin`, managed `16.4.0/bin`, dir PATH yang merupakan symlink ke dir brew → hasil 4 entri, symlink tidak duplikat, dir tanpa `initdb` diabaikan; `discover_finds_real_homebrew` ditandai `#[ignore]` (dijalankan manual di mesin ini, harus menemukan 16.14).
- [ ] **Step 2–4:** FAIL → implement → PASS (`cargo test -p pgdesk-core -- --include-ignored binaries` juga lulus di mesin ini).
- [ ] **Step 5:** Commit `feat(core): discover installed PostgreSQL binaries`.

### Task 8: Unduh binary terkelola

**Files:** `crates/core/src/download.rs`; `crates/core/Cargo.toml` (`postgresql_archive = { version = "=0.22.0", default-features = false, features = ["theseus", "tls-rustls-ring"] }`).

**Interfaces — Consumes:** `PgBinaries`, `parse_version`. **Produces:** `pub async fn install_managed(version_req: &str, managed_root: &Path) -> Result<PgBinaries>` — `get_archive(postgresql_archive::configuration::theseus::URL, &VersionReq::parse(version_req)?)`, ekstrak ke `<managed_root>/.staging-<versi>` lalu `rename` atomik ke `<managed_root>/<versi>`; bila sudah ada, kembalikan tanpa unduh; verifikasi dengan `pg_ctl --version`. Sebelum menulis, cocokkan nama modul/konstanta dengan source `postgresql_archive` v0.22.0.

- [ ] **Step 1: Failing tests:** `existing_install_is_reused_without_network` (pohon palsu `<root>/16.4.0/bin/{pg_ctl,initdb}` dengan `pg_ctl` skrip shell yang mencetak `pg_ctl (PostgreSQL) 16.4`, chmod +x → hasil `Managed`, versi 16.4); `downloads_real_binaries` `#[ignore]` (`"=16"` ke tempdir, lalu `initdb --version` jalan).
- [ ] **Step 2–4:** FAIL → implement → PASS, termasuk test ignored dijalankan sekali manual; catat ukuran unduhan & waktu di notes.
- [ ] **Step 5:** Commit `feat(core): download managed PostgreSQL binaries via postgresql_archive`.

### Task 9: Registry instance & path aplikasi

**Files:** `crates/core/src/instances.rs`.

**Interfaces — Produces:**
- `pub struct AppPaths { pub root: PathBuf }` — `default_macos() -> AppPaths` (`$HOME/Library/Application Support/pgdesk`), `instances_file()`, `cluster_dir(name)`, `log_file(name)` (`logs/<name>.log`), `socket_dir()` (`run`), `managed_bin_root()` (`bin`).
- `pub enum ListenScope { Localhost, All }` — `bind_addrs() -> Vec<BindAddr>` (Localhost → `[LoopbackV4, LoopbackV6]`, All → `[AnyV4, AnyV6]`), `listen_addresses() -> &'static str` (`"localhost"` / `"*"`).
- `#[derive(Serialize, Deserialize)] pub struct Instance { pub name: String, pub bin_dir: PathBuf, pub data_dir: PathBuf, pub port: u16, pub listen: ListenScope, pub managed: bool }`
- `pub struct Registry { pub instances: Vec<Instance> }` — `load(path) -> Result<Registry>` (file tak ada → kosong), `save(&self, path)` (tulis ke `.tmp` lalu rename), `add(&mut self, inst) -> Result<()>`, `remove(&mut self, name)`, `get(&self, name) -> Option<&Instance>`, `port_clashes(&self) -> Vec<(String, String, u16)>`.
- `pub fn validate_name(name: &str) -> Result<()>` — `^[a-z0-9_-]{1,40}$`.
- `pub fn validate_socket_dir(dir: &Path, port: u16) -> Result<()>` — panjang `"<dir>/.s.PGSQL.<port>"` ≤ 103 byte.

- [ ] **Step 1: Failing tests:** `roundtrip_toml`; `missing_file_loads_empty`; `duplicate_name_rejected`; `duplicate_data_dir_rejected`; `same_port_listed_in_port_clashes` (dua instance port 5433 → satu tuple); `invalid_names_rejected` (`"My DB"`, `""`, 41 karakter); `socket_path_too_long_is_rejected` (dir 100 karakter → Err; `/Users/rulisme/Library/Application Support/pgdesk/run` → Ok).
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Commit `feat(core): instance registry and app paths`.

### Task 10: Kontrol proses & tail log

**Files:** `crates/core/src/control.rs`, `crates/core/tests/control_it.rs`.

**Interfaces — Consumes:** `Instance`, `AppPaths`, `PgCtlStatus`, `check_port`, `PortReport`. **Produces:**
- `pub async fn init_cluster(inst: &Instance) -> Result<()>` — `initdb -D <data> -U postgres -A trust -E UTF8 --locale=C`.
- `pub async fn status(inst: &Instance) -> Result<PgCtlStatus>`
- `pub enum Preflight { Check, Force }`
- `pub enum StartOutcome { Started, AlreadyRunning { pid: u32 }, Blocked(PortReport) }`
- `pub async fn start(inst: &Instance, paths: &AppPaths, managed: Vec<(String, PathBuf)>, preflight: Preflight) -> Result<StartOutcome>` — urutan: `status` → Running → `AlreadyRunning` (tanpa cek port); `check_port` (listener milik instance ini diabaikan) → verdict ≠ Free dan `Check` → `Blocked`; lalu `pg_ctl start -w -t 30 -D <data> -l <log> -o "<opts>"` dengan opts = `-p <port> -c listen_addresses=<..> -k '<socket_dir>'` (path dikutip tunggal; tolak path yang memuat `'`).
- `pub async fn stop(inst: &Instance) -> Result<()>` (`pg_ctl stop -w -m fast`; Stopped → Ok), `pub async fn restart(..) -> Result<StartOutcome>` (stop lalu start dengan preflight sama).
- `pub async fn tail_log(path: PathBuf, tx: UnboundedSender<String>)` — mulai dari 64 KiB terakhir, poll 250 ms, kirim per baris, berhenti saat `tx` tertutup; tahan file belum ada / dipotong (rotasi).

- [ ] **Step 1: Failing tests** — unit: `tail_log_emits_appended_lines` (tempfile, append 2 baris, terima 2 baris), `tail_log_waits_for_missing_file`. Integrasi `control_it.rs` (lewati dengan `eprintln!("skip: no binaries")` bila `discover` kosong; tempdir dengan nama `"pg desk it"` berspasi; port bebas acak):
  - `lifecycle_init_start_query_stop`: init → status `Stopped` → start `Started` → `tokio_postgres` `SELECT 1` via localhost → stop → `Stopped`.
  - `start_when_running_reports_already_running`.
  - `start_blocked_by_foreign_listener`: bind `127.0.0.1:<port>` dari test → `Blocked(report)` dengan `Conflict`; dengan `Force` → start dicoba (hasil apa pun boleh, tidak panik).
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Commit `feat(core): pg_ctl lifecycle with port preflight and live log tail`.

### Task 11: UI pengelola server

**Files:** `crates/app/src/main.rs`, `crates/app/src/servers/{mod.rs,log_view.rs,new_instance.rs}`; hapus `spike.rs`.

**Interfaces — Consumes:** semua API core di atas (hanya lewat `Runtime`). Komponen gpui-component dipilih setelah membaca source v0.8.1 (`table`, `button`, `dialog`, `input`, `select`, `notification`, `scroll`, `resizable`); catat setiap selisih di notes.

Perilaku yang harus ada:
- Saat buka: load `Registry` + `discover` di background; daftar instance (nama, versi, port, data dir, status) dengan status di-refresh tiap 3 detik di background.
- Detail instance: tombol Start/Stop/Restart (disabled saat operasi berjalan), log live (`tail_log` via `spawn_stream`, maksimal 5.000 baris di memori, auto-scroll ke bawah).
- Start yang `Blocked` → dialog berisi `PortReport::explain()` dan daftar pemilik (PID, perintah, container), tombol "Batal" dan "Tetap start"; untuk `Conflict` tombol "Tetap start" tidak ada.
- "Instance baru": nama, port (default: port pertama ≥ 5432 yang `Free` menurut `check_port`), binary (daftar `discover` + opsi "Unduh PostgreSQL 16/17/18" via `install_managed`, progres indeterminate), listen scope → `init_cluster` → simpan Registry.
- "Tambah data dir yang sudah ada" (adopsi cluster Homebrew dll.): pilih folder (`cx.prompt_for_paths` atau padanannya di v0.8.1), validasi `status ≠ NotACluster`, port dibaca dari `postgresql.conf` bila ada (`port = N`), else 5432.
- Peringatan di daftar bila `port_clashes` tidak kosong.

- [ ] **Step 1:** Implement; tiap callback UI yang menyentuh core memanggil `Runtime::spawn` dan memperbarui entity di `cx.spawn`.
- [ ] **Step 2:** Verifikasi manual di mesin ini (skrip langkah di notes): buat instance `demo` port 5433 → dialog harus menyebut `rekamedika-postgres` (Docker, `*:5433`) dan menjelaskan pembayangan; buat ulang di port bebas → start → log muncul live → stop → restart; jendela tetap responsif selama unduh binary. Screenshot tiap langkah.
- [ ] **Step 3:** Gerbang build/test/clippy; `cargo tree -p pgdesk-core | grep -i gpui` kosong.
- [ ] **Step 4:** Commit `feat(app): server manager with port conflict dialog and live logs`.

### Task 12: README, ukuran & jejak memori

**Files:** `README.md`, `Cargo.toml` (profil release).

- [ ] **Step 1:** `cargo build --release -p pgdesk`; catat ukuran biner (`ls -lh target/release/pgdesk`) dan RSS idle 10 detik setelah buka (`ps -o rss= -p <pid>`); target < 150 MB.
- [ ] **Step 2:** README: prasyarat (Rust stable; tidak perlu Xcode penuh berkat `runtime_shaders`; tidak perlu Docker/Homebrew bila memakai binary unduhan), `cargo run -p pgdesk`, menjalankan test (termasuk `--include-ignored` yang butuh jaringan/binary), keputusan arsitektur (tabel dari spec + temuan spike), angka ukuran/RSS hasil ukur, peta tahap 2–5.
- [ ] **Step 3:** Gerbang build/test/clippy; commit `docs: README with build, run, architecture decisions`.

---

## Setelah rencana ini

Tulis rencana Tahap 2 (koneksi + Keychain + `.env` + pohon skema) memakai temuan nyata dari `docs/notes/gpui-kit-0.8.1-api.md`, lalu Tahap 3 (grid virtual: `TableDelegate::visible_rows_changed`/`load_more` + cursor server), Tahap 4 (editor SQL), Tahap 5 (MCP `rmcp` + tulis `DATABASE_URL` ke `.env` proyek TanStack).
