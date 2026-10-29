# Changelog

Every change to Sawit, by version. Each push to `main` is a new release: the patch version is bumped and
its section is written automatically by `.githooks/pre-push` (`scripts/release.sh`) from the commit messages.

## [0.0.5] - 2026-10-29

### Fixes

- retry hdiutil when building the DMG in CI

### Performance

- shrink the app from 44 MB to 33 MB

## [0.0.4] - 2026-10-29

### Features

- welcome screen, schema diagrams, ports & network, git, SSH and psql-style CLI

## [0.0.3] - 2026-10-29

### Features

- rebrand PG Desk to Sawit (sawit.dev)

### Fixes

- releases come from bahrulbangsawan/sawit.dev

## [0.0.2] - 2026-10-29

### Other

- rebuild any release tag by hand (workflow_dispatch)

## [0.0.1] - 2026-10-29

### Features

- sawit CLI, in-app updates and per-push releases
- prompt manager — save, search, fill and copy prompts
- MCP page covers popular AI agents and editors, with their logos
- **app**: MCP page to install sawit-mcp into every client and profile
- **core**: install sawit-mcp into AI clients and every profile
- **app**: language logos for files in the tree and tabs
- **editor**: compact left-aligned line numbers and icon view buttons
- **app**: image, PDF and video viewers
- **core**: image size and PNG copies of HEIC/AVIF for the viewer
- **core**: render PDF pages with CoreGraphics
- **app**: editor with wrap, formatting, live preview and status line
- **core**: code formatting with Prettier and per-language formatters
- **core**: cancel a running query
- Sawit logo, title-bar mark and macOS app bundle
- **app**: show/hide sidebar, center tabs and terminal dock
- **app**: closable Server tab and Zed-style tab context menu
- **mcp**: sawit-mcp stdio server and DATABASE_URL to project .env
- **app**: database panel, virtual table grid and SQL editor
- **core**: connection profiles, DATABASE_URL/.env, Keychain, schema, paging and SQL
- **app**: project file tree and tabbed code editor with save
- **core**: project file listing, safe text reading and atomic save
- **app**: bottom-dock terminal tabs that run a login shell or herdr
- **core**: headless terminal emulation (PTY + alacritty_terminal) with input encoding
- **app**: Zed-style shell with server manager, port conflict panel and live logs
- **core**: pg_ctl lifecycle with port preflight and live log tail
- **core**: instance registry and app paths
- **core**: download managed PostgreSQL binaries via postgresql_archive
- **core**: discover installed PostgreSQL binaries
- **core**: probe listening ports with owner details
- **core**: identify port owners (docker, homebrew, postgres.app, sawit)
- **core**: lsof parsing and port conflict analysis
- **core**: parse pg_ctl status output

### Fixes

- **app**: editor audit — disk changes, tab overflow, file tree, video keys
- **core**: file kinds, wider language detection, safe reads and saves
- **app**: virtual DB tree, disabled unreadable tables, reconnect and cancel
- **app**: reconnect dropped connections and mark unreadable tables
- **core**: TLS per sslmode, pooler-safe settings, keepalives, privileges
- **app**: table browser shows 50-row pages ordered by primary key
- harden read-only queries, stable grid paging and input edge cases

### Other

- demo video, GIF and screenshots for the README
- i18n: English UI, messages and docs
- fat LTO, size-optimised cold crates, compressed DMG
- refresh size and memory measurements
- README for the editor, formatter, previews and viewers
- README for pagination, tabs, layout, TLS, logo and bundling
- README with build, run, architecture decisions and measurements
- extend scope with text editor, herdr-capable terminal, Zed-style design
- gpui-kit window with DataTable and async SELECT version()
- workspace scaffold and tokio runtime bridge
- Sawit design and stage 1 implementation plan

