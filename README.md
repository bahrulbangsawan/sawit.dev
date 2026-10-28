<p align="center"><img src="assets/logo/pgdesk.svg" width="128" alt="PG Desk"></p>

# PG Desk

Aplikasi desktop macOS (Apple Silicon) all-in-one untuk development lokal dengan PostgreSQL, **tanpa Docker**:
pengelola server Postgres, penjelajah & grid database, editor SQL, editor teks proyek, dan terminal
(yang sanggup menjalankan [herdr](https://herdr.dev)). Dibangun dengan Rust + [GPUI](https://gpui.rs)
(`gpui-kit` 0.8.1), tampilan mengikuti Zed (palet resmi "One Dark").

## Fitur

- **Server** — daftar instance (nama, versi, port, data dir, status), start/stop/restart via `pg_ctl`,
  log live. Binary Postgres dari Homebrew / Postgres.app / `PATH` dideteksi otomatis, atau **diunduh oleh
  PG Desk** (PostgreSQL 16/17/18) sehingga proyek tidak butuh Docker maupun Homebrew. Data dir yang sudah ada
  (mis. cluster Homebrew) bisa ditambahkan.
- **Deteksi konflik port sebelum start** — siapa yang mendengarkan port itu (proses lain, Homebrew service,
  container Docker beserta namanya, Postgres.app, instance PG Desk lain), termasuk kasus *membayangi*:
  satu proses bind `127.0.0.1:5433` sementara yang lain bind `*:5433`, sehingga klien diam-diam sampai ke
  database yang salah. Start ditahan dengan penjelasan; konflik di alamat yang sama tidak bisa dipaksa.
- **Database** — koneksi dari `DATABASE_URL`, file `.env`, atau instance PG Desk yang berjalan; password di
  **macOS Keychain** (tidak pernah di file). TLS mengikuti `sslmode` di URL (`disable`/`prefer`/`require`
  seperti libpq; `verify-full` memeriksa sertifikat terhadap trust store sistem), koneksi yang putus
  disambung ulang otomatis, dan pooler (PgBouncer, Supavisor, Neon) didukung. Pohon database → schema →
  tabel/view → kolom & tipe (virtual, lancar untuk ribuan tabel); tabel tanpa izin SELECT dinonaktifkan dengan
  tooltip. Query yang berjalan bisa dibatalkan (tombol Batal) di editor SQL dan tabel.
- **Tabel read-only berhalaman** — 50 baris per halaman (pertama/sebelumnya/berikutnya/terakhir), diurutkan
  menurut primary key sehingga halaman stabil, lewat koneksi read-only terpisah. Halaman pertama tampil
  sebelum jumlah baris selesai dihitung; hitungan dibatasi 3 detik lalu memakai estimasi (`±`).
- **Editor SQL** — ⌘↩ menjalankan teks terpilih atau seluruh isi; hasil, error (dengan DETAIL/HINT), durasi;
  maksimal 10.000 baris ditampilkan (dibaca secara streaming, sisanya tidak ditampung di memori).
- **Editor teks** — `pgdesk <folder>` atau ikon folder untuk membuka proyek. Pohon file virtual (ikon per jenis,
  ikut berubah saat file dibuat/dihapus dari terminal), highlighting ±200 bahasa (syntect; Dockerfile, Makefile,
  Vue, Svelte, Kotlin, dll.), soft wrap tanpa scroll horizontal, tanpa scrollbar, toolbar gaya Zed (breadcrumb,
  `Ln, Col`, bahasa). ⌘S simpan atomik (symlink tetap symlink), file yang berubah di disk dimuat ulang (atau
  diperingatkan bila ada perubahan belum disimpan).
- **Formatter (⇧⌘I)** — Prettier untuk JS/TS/JSON/CSS/SCSS/Less/HTML/Vue/Markdown/YAML/GraphQL; sql-formatter
  (dialek PostgreSQL) untuk SQL, Taplo untuk TOML, rustfmt, gofmt, Ruff, shfmt, clang-format untuk lainnya.
  Memakai salinan di `node_modules/.bin` proyek bila ada (versi dan config proyek berlaku), lalu `PATH`, lalu
  `npx`/`bunx`. Satu langkah undo.
- **Pratinjau & viewer** — Markdown, HTML dan SVG: Kode / Berdampingan / Pratinjau dengan pratinjau langsung
  (gambar relatif dan tabel ikut tampil). Gambar (PNG/JPEG/WebP/GIF/HEIC/AVIF…; pas atau 100%), PDF (halaman
  dirender sesuai layar dengan CoreGraphics, zoom 50–300%), video (AVPlayer, dengan suara; Spasi, seek, mute).
- **Tab** — klik kanan untuk Close, Close Others (⌥⌘T), Close Left/Right (⌘K E / ⌘K T), Close Clean (⌘K U),
  Close All (⌘K W), Make Tab Read-Only, Pin Tab (⌘K ⇧↩); ⌘W menutup tab aktif. Tutup massal tidak menyentuh
  tab yang di-pin atau belum disimpan; menutup file yang belum disimpan meminta konfirmasi. Tab Server bisa
  ditutup dan muncul lagi saat server dipilih.
- **Layout** — tombol di title bar atau ⌘B (sidebar), ⌘J (terminal/herdr), ⇧⌘J (area tab & tabel).
- **Terminal** — dock bawah dengan tab; shell login atau **herdr** (tombol `herdr` bila terpasang). Emulasi
  lengkap via `alacritty_terminal`: warna 256/truecolor, alt-screen, mouse SGR, bracketed paste, resize.
- **MCP** — biner `pgdesk-mcp` (stdio) untuk agent AI: `list_instances`, `start_instance`, `stop_instance`,
  `list_connections`, `describe_schema`, `query` (**selalu read-only**, ≤500 baris), `write_env`.
- **Integrasi proyek (TanStack dll.)** — tombol `.env` pada instance (atau tool MCP `write_env`) menulis
  `DATABASE_URL=postgres://postgres@localhost:<port>/postgres` ke `.env` proyek.

## Build & jalankan

Prasyarat: Rust (versi dikunci di `rust-toolchain.toml` = 1.99.1; rustup memasangnya otomatis) dan
Xcode **Command Line Tools** saja — Xcode penuh tidak perlu karena shader Metal dikompilasi saat runtime
(`runtime_shaders`). Docker dan Homebrew tidak diperlukan.

```bash
cargo run -p pgdesk                 # aplikasi
cargo run -p pgdesk -- ~/code/app   # langsung membuka folder proyek
cargo build --release               # target/release/pgdesk dan target/release/pgdesk-mcp
scripts/bundle-macos.sh             # target/release/PG Desk.app (dengan ikon) untuk /Applications
```

Data aplikasi: `~/Library/Application Support/pgdesk/` (`instances.toml`, `profiles.toml`, `clusters/`,
`logs/`, `bin/` untuk binary unduhan, `run/` untuk socket). Set `PGDESK_HOME=/path` untuk memakai folder lain
(development/test).

Instance baru hanya mendengarkan `localhost`, superuser `postgres`, autentikasi `trust` (lokal saja).

### MCP

```bash
claude mcp add pgdesk -- /path/to/target/release/pgdesk-mcp
```

## Test

```bash
cargo test --workspace                    # unit + integrasi (memulai cluster Postgres sementara bila ada binary)
cargo test --workspace -- --include-ignored   # + unduh binary sungguhan & Keychain
cargo clippy --workspace --all-targets -- -D warnings
```

Test integrasi memakai binary Postgres yang ditemukan (Homebrew/Postgres.app/PATH/unduhan) dan dilewati
dengan pesan bila tidak ada.

## Arsitektur & keputusan utama

```
crates/core   pgdesk-core — tanpa GPUI, dites dengan cargo test biasa
  ports, owners, probe   lsof/ps/docker → analisis konflik & penjelasan
  pgctl, control         status/initdb/start/stop/restart + tail log
  binaries, download     deteksi binary + unduh via postgresql_archive
  instances              registry TOML, path aplikasi, validasi
  db                     profil, DATABASE_URL/.env, Keychain, skema, paging, SQL, read-only
  files                  daftar folder, baca aman, simpan atomik
  terminal               PTY + alacritty_terminal → snapshot, encoding keyboard/mouse
  rt                     runtime Tokio di thread sendiri
crates/app    pgdesk — UI GPUI saja (menggambar & meneruskan input)
crates/mcp    pgdesk-mcp — server MCP stdio di atas core
```

| Keputusan | Alasan |
|---|---|
| `gpui-kit =0.8.1` (bukan `gpui` 0.2.2 di crates.io) | gpui-component 0.8.1 dibangun di atas snapshot `gpui-pre =0.3.10`; gpui-kit mengunci set yang cocok. Semua API dicocokkan dengan source tag `v0.8.1` (catatan: `docs/notes/gpui-kit-0.8.1-api.md`). |
| Rust 1.99.1 dikunci | `gpui-pre-macos 0.3.10` memakai API yang belum stabil di 1.97. |
| Tokio di thread terpisah + channel `futures` | Thread UI tidak pernah menunggu I/O; hasil di-await di `cx.spawn`. Terbukti di spike: 29 tick UI & 33 frame selama query 3 detik. |
| `tokio-postgres` (bukan `sqlx`) | Lebih ringan, `simple_query_raw` untuk SQL bebas secara streaming. |
| `postgresql_archive` (bukan `postgresql_embedded`) | Hanya butuh unduh+ekstrak; `postgresql_embedded` membawa `sqlx` dan mengatur siklus hidup sendiri. |
| `lsof -F` + `ps` + `docker ps` | Tanpa root; wildcard IPv6 dianggap dual-stack (ragu → peringatkan). |
| PATH dari login shell saat start | Aplikasi yang dibuka dari Finder mendapat PATH minimal; tanpa ini `docker`/`brew`/`herdr` tidak ditemukan. |
| Terminal: `alacritty_terminal` + canvas | Seperti terminal Zed; core menghasilkan snapshot, app hanya menggambar. Variabel `HERDR_*`/`TMUX`/`ZELLIJ*` dari proses induk dibuang agar herdr tidak menolak "nested". |
| Highlighting: syntect | ±200 bahasa dengan ukuran kecil, tanpa grammar tree-sitter per bahasa. |
| Keychain via `security-framework` | Target macOS; lebih kecil dari `keyring` 4.x. |
| Ikon: hanya yang dipakai | Bundle penuh Lucide menambah ±1 MiB. |
| TLS: rustls + `ring` | `ring` sudah ada untuk unduhan binary, jadi tidak menambah pustaka kripto kedua. |
| Logo: SVG buatan tangan | Gajah (PostgreSQL) dari dua lingkaran, satu kurva telinga, satu goresan belalai, di atas kursor prompt (editor + terminal); warna One Dark. `assets/logo/`. |

## Ukuran & memori

Diukur di MacBook Apple Silicon (macOS 26.6), build release (`lto = "thin"`, `strip`):

| | |
|---|---|
| `pgdesk` | 23,3 MB (satu file, tanpa runtime JVM/Chromium/Node) |
| `pgdesk-mcp` | 3,3 MB |
| Memori diam (1 jendela, 1 tab terminal, 10 detik setelah buka) | RSS 80 MB · physical footprint 61 MB (puncak 109 MB saat startup) |
| Build release bersih | ±4 menit |

Angka aplikasi lain tidak dicantumkan karena belum diukur di mesin yang sama.

## Batasan yang diketahui

- macOS saja (Keychain, `lsof`, Postgres.app). 
- Grid read-only; edit data lewat editor SQL.
- Paging memakai `OFFSET/LIMIT`; tabel tanpa primary key (dan view) memakai urutan fisik, stabil selama
  datanya tidak berubah.
- Pada pooler mode transaction, setelan sesi (`SET`) tidak dijamin bertahan; browser tabel tetap hanya
  menjalankan `SELECT`.
- Terminal belum mendukung seleksi teks dengan mouse untuk disalin (program di dalamnya, mis. herdr, tetap menerima mouse).
