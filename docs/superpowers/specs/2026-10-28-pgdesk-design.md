# PG Desk — Desain

Tanggal: 2026-10-28. Sumber: spesifikasi proyek dari pengguna (disalin ringkas di bawah)
ditambah permintaan lanjutan dan keputusan yang saya ambil (ditandai **[keputusan saya]**)
karena pengguna meminta: "Lanjutkan saja tanpa sesi ini, kamu yang tentukan."

## Tujuan & alasan

GUI PostgreSQL desktop macOS (Apple Silicon) yang ringan untuk development lokal **tanpa Docker**.
Pengguna: satu developer dengan beberapa proyek yang masing-masing punya DB lokal, dan pernah
tersambung diam-diam ke DB yang salah karena Homebrew bind `127.0.0.1:5433` sementara Docker bind `*:5433`.

Permintaan lanjutan pengguna (verbatim intinya):
- "GUI PostgreSQL, yang bisa manage table postgresql, bisa berjalan tanpa docker, ada mcp"
- "untuk project tanstack saya kedepannya tidak perlu menginstall docker atau hal lain"
- "Pastikan ini paling ringan dibanding yang lain."

## Sasaran

1. **Pengelola server**: daftar instance (nama, versi, port, data dir, status); start/stop/restart via
   `pg_ctl` dengan log live; deteksi konflik port sebelum start (proses lain, Homebrew service, container
   Docker), termasuk kasus `127.0.0.1` vs `*` pada port yang sama; binary terpasang (Homebrew/Postgres.app,
   dideteksi, tidak di-hardcode) **dan** binary yang diunduh aplikasi sendiri.
2. **Koneksi & penjelajah skema**: profil (host, port, db, user; password di macOS Keychain); impor
   `DATABASE_URL` dari `.env` pilihan pengguna; pohon database → schema → tabel/view + kolom & tipe.
3. **Grid data read-only**: tabel virtual, paging dari server, lancar untuk ratusan ribu baris.
4. **Editor SQL**: multi-baris, jalankan, tampilkan hasil/error/durasi. Highlighting = stretch.
5. **MCP & integrasi proyek** **[keputusan saya: tahap 5, setelah MVP inti]**: server MCP (stdio) agar
   agent AI bisa melihat instance, skema, dan menjalankan query; tulis `DATABASE_URL` ke `.env` proyek
   TanStack dari instance terkelola.

"Manage table" **[keputusan saya]**: di MVP dipenuhi oleh penjelajah skema + grid + editor SQL (DDL/DML
lewat SQL). Edit data inline tetap di luar cakupan MVP sesuai spesifikasi awal; bisa jadi tahap berikutnya.

## Non-sasaran

Edit data inline (MVP), desain skema visual, DB selain Postgres, sinkronisasi cloud.

## Anti-sasaran (gagal walau "jalan")

- Thread UI terblokir oleh I/O DB atau proses.
- Konflik port lolos tanpa peringatan → pengguna tersambung ke DB yang salah.
- Password tersimpan plaintext.
- Aplikasi berat (Electron/JVM-class). Lihat target ringan di bawah.
- Proyek TanStack masih butuh Docker/Homebrew untuk punya Postgres.

## Batasan teknis

- Rust stable (mesin: 1.97.1), satu workspace Cargo; crate `core` tanpa dependensi GPUI, crate `app` UI saja.
- Target macOS Apple Silicon; path Homebrew dideteksi (`brew --prefix`), tidak di-hardcode.
- Jangan menulis API GPUI dari ingatan: kunci versi persis dan cocokkan dengan source versi itu.
- Setiap tahap: `cargo build`, `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings` bersih, lalu commit.

## Keputusan arsitektur

| Area | Keputusan | Alasan |
|---|---|---|
| UI | `gpui-kit = "=0.8.1"` (fasad resmi Longbridge: GPUI + gpui-component + assets) | Temuan: `gpui-component` 0.8.1 **tidak** memakai `gpui` 0.2.2 di crates.io, tapi snapshot `gpui-pre =0.3.10` + `gpui-pre-platform` dengan fitur `runtime_shaders` (tidak butuh Xcode penuh; mesin ini hanya punya Command Line Tools). `gpui-kit` mengunci set yang cocok. Source referensi: tag `v0.8.1`, commit `99576db58ec8103417c41a092e891b4022d24e7e`. |
| Driver DB | `tokio-postgres` 0.7.x, `NoTls` untuk lokal | Lebih ringan dari `sqlx` (tanpa makro compile-time/pool bawaan); `simple_query` cocok untuk editor SQL bebas; cursor/portal untuk paging. |
| Async | Runtime Tokio multi-thread di thread sendiri (`core::rt`), hasil dikirim lewat `futures::channel::oneshot`/`mpsc` dan di-await di `cx.spawn` GPUI | Channel `futures` tidak terikat executor; UI tak pernah blok. |
| Binary Postgres | Deteksi Homebrew (`brew --prefix` → `opt/postgresql@*`), Postgres.app (`/Applications/Postgres.app/Contents/Versions/*`), `PATH`; plus unduh sendiri via **`postgresql_archive`** (fitur `theseus`, `tls-rustls-ring`) | Evaluasi `postgresql_embedded` 0.22: membawa `sqlx` dan mengatur siklus hidup sendiri (bertabrakan dengan kontrol `pg_ctl` kita). Crate dasarnya `postgresql_archive` cukup untuk unduh+ekstrak binary → lebih ringan. Ini juga yang membuat proyek TanStack tak perlu Docker/Homebrew. |
| Kontrol proses | `initdb`, `pg_ctl start -l <log> -o "-p <port> -c listen_addresses=<...> -k <sockdir>"`, `pg_ctl stop -m fast`, `pg_ctl status`; log live = tail file log | `pg_ctl start` melepas proses; tail file log bekerja baik untuk instance yang dimulai di luar aplikasi juga. |
| Deteksi port | `lsof -nP -iTCP:<port> -sTCP:LISTEN -F pcn` (format field, stabil untuk diparse) + `ps -o command= -p <pid>` + `docker ps --format` bila `docker` ada | Tidak butuh root untuk proses milik user. Terverifikasi di mesin ini: Docker muncul sebagai `com.docker.backend` dengan `n*:5433`. |
| Penyimpanan | TOML di `~/Library/Application Support/pgdesk/` (`instances.toml`, `profiles.toml`); password di Keychain via crate `keyring` | Terbaca manusia, mudah di-diff; rahasia tidak pernah ke file. |
| Instance terkelola | Data dir di `~/Library/Application Support/pgdesk/clusters/<name>`; binary unduhan di `.../pgdesk/bin/<versi>`; `listen_addresses=localhost`, socket dir di bawah dir aplikasi | Terisolasi dari Homebrew; tidak bentrok socket `/tmp`. |
| MCP | crate `rmcp` (stdio), biner terpisah `pgdesk-mcp` memakai `core`; query read-only secara default (`SET TRANSACTION READ ONLY`) | Agent tidak bisa merusak data tanpa izin eksplisit. |

### Semantik konflik port

Alamat bind dimodelkan: wildcard IPv4 (`*`/`0.0.0.0`), wildcard IPv6 (`[::]`), loopback IPv4/IPv6, alamat spesifik.
Untuk port target dan alamat yang akan dipakai instance kita:
- **Bebas**: tidak ada listener.
- **Bentrok**: ada listener pada alamat yang sama persis → start akan gagal; jelaskan pemiliknya.
- **Membayangi (shadowing)**: listener lain di alamat dengan spesifisitas berbeda (mis. kita `127.0.0.1`, Docker `*`)
  → start bisa *berhasil* tapi klien yang memakai `localhost` dan yang memakai IP lain sampai ke server berbeda.
  Ini kasus yang pernah menggigit pengguna; peringatannya harus menyebut kedua pihak dan mana yang menang untuk `127.0.0.1`.
Pemilik diidentifikasi: Docker (+ nama container dari `docker ps`), Homebrew (path biner di bawah `brew --prefix`),
Postgres.app, instance PG Desk sendiri (data dir cocok), atau proses lain (nama + PID).

### Target "paling ringan" [keputusan saya]

Diukur, dicatat di README: ukuran biner release (`lto = "thin"`, `strip = true`), RSS saat idle dengan satu jendela.
Target: RSS idle < 150 MB, tanpa runtime JVM/Chromium. Angka pembanding aplikasi lain tidak diklaim tanpa diukur di mesin yang sama.

## Urutan kerja

0. Spike integrasi (jendela GPUI + DataTable + `SELECT version()` via Tokio, UI tidak beku) → catat temuan API.
1. Tahap 1 (rencana detail: `docs/superpowers/plans/2026-10-28-pgdesk-stage1-server-manager.md`).
2–5. Rencana tiap tahap ditulis setelah tahap sebelumnya selesai, memakai temuan API nyata dari tahap sebelumnya.

## Diserahkan ke builder

Tata letak visual detail, teks UI, struktur modul internal di luar antarmuka yang ditetapkan rencana.
