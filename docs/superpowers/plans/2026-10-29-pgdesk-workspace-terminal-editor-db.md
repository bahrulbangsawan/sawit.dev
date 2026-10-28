# PG Desk — Terminal, Editor, Database (Tahap T, E, 2–5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Lengkapi PG Desk all-in-one: terminal yang menjalankan herdr, editor teks proyek, koneksi + penjelajah skema, grid data virtual, editor SQL, MCP + integrasi `.env`.

**Architecture:** Logika non-UI di `pgdesk-core` (terminal PTY/emulasi, file tree, koneksi/skema/query, keychain, dotenv, MCP tools); `pgdesk` app hanya menggambar dan meneruskan input. Biner `pgdesk-mcp` terpisah memakai core.

**Spec:** `docs/superpowers/specs/2026-10-28-pgdesk-design.md` (bagian "Perluasan cakupan").

## Global Constraints
Sama dengan rencana Tahap 1 (core tanpa GPUI, versi UI dikunci, UI tidak pernah blok, gerbang build/test/clippy + commit per task). Rust 1.99.1 (rust-toolchain.toml).

## Review Focus
1. **herdr di terminal**: alt-screen, mouse SGR, resize, warna truecolor, Ctrl-kombinasi — harus bisa dioperasikan penuh. Verifikasi manual dengan herdr 0.8.2 terpasang.
2. **File besar / biner di editor**: buka file >5 MB atau non-UTF-8 tidak boleh membekukan/merusak; tampilkan pesan read-only.
3. **Tabel ratusan ribu baris**: grid hanya mengambil halaman yang terlihat (keyset/OFFSET per 200 baris), scroll tetap lancar.
4. **Password**: tidak pernah ditulis ke file; hanya Keychain.
5. **MCP**: default read-only (`SET TRANSACTION READ ONLY`); statement penulisan ditolak kecuali flag eksplisit.

### Task T1: `core::terminal` — PTY + emulasi + encoding input
Interfaces: `Session::spawn(Command, GridSize) -> Result<(Session, UnboundedReceiver<TermEvent>)>`, `write`, `resize`, `scroll`, `snapshot(&Palette) -> Snapshot`; `key_to_bytes(&KeyInput, Modes)`, `mouse_report(MouseAction, col, row, shift, Modes)`, `paste_bytes`.
Tests: lihat `crates/core/src/terminal/tests.rs` (12 test, termasuk PTY nyata).

### Task T2: Panel terminal di dock bawah (app)
`TerminalView` (canvas: ukur sel dari font mono, resize sesi saat bounds berubah, gambar latar per-run + teks per-run + kursor), fokus & keyboard → `key_to_bytes`, Cmd-V paste, Cmd-C salin seleksi (opsional), mouse → `mouse_report` atau scroll history, tab terminal ganda (+), tombol "herdr" yang membuka tab menjalankan `herdr` bila ada di PATH. Verifikasi manual: jalankan herdr, navigasi dengan keyboard & mouse, resize panel.

### Task E1: `core::files` — pohon file proyek
`list_dir(path) -> Vec<Entry{name, path, is_dir}>` (urut dir dulu, sembunyikan `.git`, `node_modules`, `target`), `read_text(path) -> Result<TextFile{text, read_only_reason}>` (batas 5 MB, deteksi non-UTF-8), `write_text_atomic(path, text)`. TDD dengan tempdir.

### Task E2: Editor teks (app)
Panel kiri bertab: Servers | Files | Database. Files: buka folder proyek (prompt), pohon lazy. Area tengah bertab (gaya Zed): tab file dengan `InputState::code_editor(lang)`, indikator belum tersimpan, Cmd-S simpan, Cmd-W tutup. Bahasa dari ekstensi (sql, ts, tsx, js, jsx, json, md, toml, rs, css, html, yaml).

### Task D1: `core::db` — profil, dotenv, keychain, skema
`ConnProfile{name, host, port, database, user}` (TOML), `parse_database_url(&str) -> Result<(ConnProfile, Option<String> password)>`, `read_dotenv_database_url(path)`, password via `keyring` (service `pgdesk`, account `user@host:port/db`), `load_schema(client) -> Vec<SchemaNode>` (schema → table/view → kolom+tipe dari `information_schema`/`pg_catalog`).

### Task D2: Panel Database + pohon skema (app)
Tambah koneksi (form / impor `.env` / dari instance PG Desk), pohon database→schema→tabel/view→kolom.

### Task D3: Grid data read-only virtual
`core::db::fetch_page(client, table, offset, limit)` + `count_estimate`; tab grid memakai `TableDelegate` (`rows_count` = estimasi/total, `visible_rows_changed` memicu fetch halaman 200 baris, cache LRU halaman).

### Task D4: Editor SQL
Tab "SQL" per koneksi: code_editor bahasa sql, Cmd-Enter jalankan (`simple_query`), hasil di grid, error & durasi; statement terpilih atau seluruh isi.

### Task M1: MCP + integrasi proyek
Biner `pgdesk-mcp` (rmcp stdio): tools `list_instances`, `start_instance`, `stop_instance`, `list_schemas`, `describe_table`, `query` (read-only default). Tombol "Tulis DATABASE_URL ke .env" pada instance (pilih folder proyek, update/append baris).

### Task Z: README + ukuran/RSS + review akhir
