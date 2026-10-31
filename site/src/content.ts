export const REPO = 'https://github.com/bahrulbangsawan/sawit.dev'
// Every release attaches Sawit.dmg under the same name, so this always fetches the newest one.
export const DMG = `${REPO}/releases/latest/download/Sawit.dmg`
export const INSTALL = 'curl -fsSL https://sawit.dev/install.sh | sh'
// The release binary, as README's Size & memory measured it: change both together.
export const SIZE = '58 MiB'

// Capabilities, shortcuts and wording from the README; screenshots from docs/media.
export const features = [
  {
    name: 'SQL editor',
    keys: ['⌘', '↩'],
    line: 'Runs the selection, or the whole editor. Results stream in, with DETAIL and HINT on errors.',
    shot: 'sql-editor',
    alt: 'SQL editor with an aggregation query and its result table',
  },
  {
    name: 'Table browser',
    keys: [],
    line: '50 rows a page over a separate read-only connection. Export what matches to CSV or Excel.',
    shot: 'table-grid',
    alt: 'Table grid for shop.orders with 18,500 rows, row checkboxes, search and Export',
  },
  {
    name: 'Schema diagrams',
    keys: [],
    line: 'Tables and foreign keys drawn as a diagram, or read as DBML. Save it as .dbml or .svg.',
    shot: 'schema-diagram',
    alt: 'Schema diagram of customers, orders, order_items, products and reviews',
  },
  {
    name: 'Code editor',
    keys: ['⇧', '⌘', 'I'],
    line: 'About 200 languages, and Prettier, rustfmt, gofmt or Ruff on one shortcut.',
    shot: 'markdown-preview',
    alt: 'Markdown file next to its rendered preview',
  },
  {
    name: 'Terminal',
    keys: ['⌘', 'J'],
    line: 'Full alacritty emulation in a dock under your code, with split panes.',
    shot: 'editor-terminal',
    alt: 'Code editor with a TSX file and a terminal running psql underneath',
  },
  {
    name: 'Listening ports',
    keys: [],
    line: 'Every dev server you started, the project it belongs to, and a button to stop it.',
    shot: 'ports',
    alt: 'List of listening ports with their processes and projects',
  },
  {
    name: 'Git',
    keys: [],
    line: 'Status, diff, stage and commit, and a graph of the branches.',
    shot: 'git',
    alt: 'Git panel with changed files and a commit graph',
  },
  {
    name: 'Browser for agents',
    keys: [],
    line: 'A tab per project on your dev server, with Web Inspector, that AI agents drive over MCP.',
    shot: 'browser',
    alt: 'Browser tab showing a local dev server inside Sawit',
  },
] as const

// The CLI, set as the plantation's menu. Commands from `sawit --help`.
export const menu = [
  {
    group: 'Kebun · the estate',
    items: [
      { cmd: 'sawit kalimantan', means: 'tables', note: 'Lists the tables of a database' },
      { cmd: 'sawit kalimantan hutan', means: 'rows', note: 'The first 100 rows, like psql' },
      { cmd: 'sawit kalimantan hutan "izin is null"', means: 'where', note: 'The text becomes the WHERE clause' },
      { cmd: 'sawit list', means: 'everything', note: 'Instances, databases, ports and connections' },
    ],
  },
  {
    group: 'Panen · the harvest',
    items: [
      { cmd: 'sawit panen kalimantan hutan hutan.xlsx', means: 'export', note: 'A table or a query, to CSV or Excel' },
      { cmd: 'sawit backup kalimantan kalimantan.dump', means: 'pg_dump', note: 'The database, one schema or one table' },
      { cmd: 'sawit tanam kalimantan konsesi konsesi.csv', means: 'import', note: 'Plants a CSV in a table, all rows or none' },
    ],
  },
  {
    group: 'Bakar · the burning',
    items: [
      { cmd: 'sawit bakar ports', means: 'stop', note: 'Dev servers only: asks, then SIGTERM, then SIGKILL' },
      { cmd: 'sawit bakar ports 3000 -y', means: 'stop :3000', note: 'Whatever listens there, without asking' },
      { cmd: 'sawit bakar instances', means: 'pg_ctl stop', note: 'Running instances; the data is kept' },
    ],
  },
  {
    group: 'Pupuk · the fertiliser',
    items: [
      { cmd: 'sawit list tools', means: 'tools', note: 'Package managers, runtimes and AI agents' },
      { cmd: 'sawit pasang bun uv', means: 'install', note: 'Official install scripts, prerequisites first' },
      { cmd: 'sawit pupuk', means: 'upgrade', note: 'Every tool with a newer release' },
    ],
  },
]

export const faq = [
  {
    q: 'Do I need Docker?',
    a: 'No. Sawit uses PostgreSQL from Homebrew, Postgres.app or your PATH, or downloads PostgreSQL 16, 17 or 18 itself.',
  },
  {
    q: 'Which Macs does it run on?',
    a: 'Apple Silicon, macOS only. Sawit leans on the Keychain, lsof, AVPlayer and CoreGraphics.',
  },
  {
    q: 'Why does macOS say it cannot verify Sawit?',
    a: 'Sawit is not notarized yet. The install script avoids the warning. With the DMG, open Sawit once and choose Done, then Open Anyway in System Settings → Privacy & Security, or run xattr -dr com.apple.quarantine /Applications/Sawit.app.',
  },
  {
    q: 'Where do my passwords go?',
    a: 'Into the macOS Keychain, never a file. The table browser reads over a separate read-only connection.',
  },
  {
    q: 'How does it update?',
    a: 'After the first install the app updates itself. Put SAWIT_VERSION=v0.0.8 before sh to install a given version.',
  },
  {
    q: 'Can I edit a single cell in the grid?',
    a: 'Not yet. The grid deletes rows or sets one column on the rows you select; single cells go through the SQL editor.',
  },
]
