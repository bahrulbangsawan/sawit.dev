//! `sawit --help`: every command, subcommand, alias and option the command line takes.

pub const USAGE: &str = "\
sawit — open folders and files in Sawit, query databases, clear ports

Usage:
  sawit [options] [path[:line[:column]] ...]
  sawit <db> [table] [\"sql\"] [-y]
  sawit list [instances|db|ports|connections|tools|mcp|skills|config|keys]
  sawit bakar ports [port ...] [-y]
  sawit bakar instances [name ...] [-y]
  sawit nyalakan [name ...] [--ulang] [-y]
  sawit browse [url|port]
  sawit new [name] [--template t3] [stack flags] [--dir <folder>] [--create]
  sawit panen <db> <table|\"sql\"> <file.csv|file.xlsx> [-y]
  sawit backup <db> [schema|schema.table] <file.sql|file.dump> [-y]
  sawit tanam <db> <table> <file.csv>
  sawit pasang <tool ...> [-y]
  sawit pupuk [tool ...] [-y]
  sawit cabut <tool ...> [-y]
  sawit skills install|update|remove [--project [dir]] [-y]
  sawit github token --header | --exec <program> [args ...]

Examples:
  sawit .                  open this folder as a project (joins the open
                           window next to its other projects)
  sawit -n ~/code/shop     open a folder in a new window of its own
  sawit -r ~/code/blog     open it in place of the front window's project
  sawit src/main.rs:42     open a file at line 42
  sawit -w COMMIT_EDITMSG  wait until the file's tab is closed (git core.editor)
  ls | sawit -             open stdin as a new file
  sawit -- -notes.md       a name that starts with - (after --, all are paths)

Browser (the project's tab in Sawit, which starts if it isn't running):
  sawit browse             this project's running dev server
  sawit browse 5173        http://localhost:5173
  sawit browse example.com any address, as the address field takes it
  sawit https://example.com
                           a URL on its own opens there too

New project (the app's New Project page: Better-T-Stack, its CLI 3.49.0):
  sawit new                the page in Sawit (starts it if it isn't running)
  sawit new shop --template t3
                           print the command for a preset, named shop
  sawit new shop --frontend next --backend self --database postgres
                           print the command for a stack; the flags are
                           create-better-t-stack's (--runtime, --api, --auth,
                           --orm, --db-setup, --addons a b, --examples todo,
                           --package-manager pnpm, --no-git, …), and what the
                           compatibility rules change is said first
  sawit new shop --create  create it in this folder (--dir <folder> for
                           another), then: sawit shop

Kebun (databases), no app needed:
  sawit kyzn                               list tables
  sawit kyzn leads                         first 100 rows
  sawit kyzn leads \"phone is null\"         filter (becomes WHERE)
  sawit kyzn leads \"select * from leads where phone is null\"
  sawit kyzn \"delete from leads where id = 3\"
                                           SQL is tried read-only first; SQL
                                           that writes, or several statements,
                                           is shown, then asks (-y: run it
                                           without asking)
  <db> is a Sawit instance, a saved connection, a database named in
  ./.env DATABASE_URL (or a folder above), or a database on a running
  local Postgres. A unique prefix works: kyzn finds kyzn_crm.

  sawit list               instances, databases, ports and connections
  sawit list db            just one: instances, db, ports or connections
  sawit bakar ports        stop dev servers on ports (asks first;
                           SIGTERM, then SIGKILL for the stubborn)
  sawit bakar ports 3000   stop whatever listens on :3000
  sawit bakar instances    stop running Sawit instances (data is kept)
  sawit nyalakan           start every stopped Sawit instance; a port
                           another process holds is explained, not taken
  sawit nyalakan kyzn --ulang
                           restart kyzn (asks first while it runs; -y)

  sawit panen kyzn leads leads.xlsx        export a table (CSV or Excel)
  sawit panen kyzn \"select * from leads where phone is null\" x.csv
                                           export a query (read-only)
  sawit backup kyzn kyzn.dump              pg_dump the database (.dump is
                                           pg_restore's format, else SQL)
  sawit backup kyzn crm.leads leads.sql    one schema or table
  sawit tanam kyzn leads leads.csv         load a CSV file into a table
  An existing file is replaced only after asking (or with -y).

Tools (the app's Tools page):
  sawit list tools         package managers, runtimes, AI agents (alat)
  sawit pasang bun uv      run their official install scripts, prerequisites
                           first (asks first; -y to skip)
  sawit pupuk              upgrade every installed tool with a newer
                           release (shown as → in `list tools`)
  sawit pupuk bun          upgrade just bun (asks first; -y to skip)
  sawit cabut bun          uninstall bun: only the copy its own installer,
                           Homebrew or npm put there (asks first; -y to skip)

Sawit Skills (the footer's Install Skills; asks first, -y to skip):
  sawit skills install     every agent on this Mac: the Sawit skills in its
                           skills folder, a short block in its own instruction
                           file (~/.claude/CLAUDE.md, ~/.codex/AGENTS.md, …)
                           and Sawit's MCP server where a client lacks it
  sawit skills update      the same: brings everything to this Sawit's version
  sawit skills install --project
                           only the block, in this folder's project: the one
                           of AGENTS.md, CLAUDE.md, .cursorrules and .github/
                           copilot-instructions.md that has it, else the one
                           there, else a new AGENTS.md
  sawit skills remove      the skills, the blocks (a file Sawit made goes once
                           it holds nothing else) and the MCP entries install
                           added; an entry that was there before stays

More lists, shown only when asked:
  sawit list mcp           the MCP page: servers and the AI clients that
                           have them (colokan)
  sawit list skills        the Skills page: skill packs, how each installs
                           and the agents that have them (skill, bibit)
  sawit list config       shell startup files and coding agents' config
                           files, for this folder's project (configs, konfig)
  sawit list explorer      Cloudflare Local Explorer: the ones Workers dev
                           servers serve, and this folder's Workers, their
                           bindings and Wrangler version (cf, jelajah)

GitHub's MCP server (the MCP page writes these into AI clients' configs):
  sawit github token --header
                           print {\"Authorization\":\"Bearer …\"}, the app's
                           GitHub sign-in, for Claude Code's headersHelper
                           and Codex's http_headers_helper
  sawit github token --exec npx -y mcp-remote <url> \\
      --header 'Authorization:${SAWIT_GITHUB_AUTHORIZATION}'
                           run a program with \"Bearer …\" in that variable
  Signed out, it prints nothing and exits 1. Any program that can run it as
  you gets the token, as it can read the Keychain item it comes from.

Keys (the app's Keymap Editor; Sawit must be running):
  sawit list keys          every shortcut: its keys, what it runs, where,
                           and whether it's a default, yours or turned off
  sawit list keys | grep -i split
                           find one (aliases: key, keymap, shortcuts, pintasan)

Aliases (every word has an English and an Indonesian name):
  list = ls, daftar · bakar = stop · nyalakan = start · browse = intip
  new = semai · --create = --buat · --dir = --lokasi
  panen = export, ekspor · backup = dump, cadangkan · tanam = import, impor
  pasang = install · pupuk = upgrade · cabut = uninstall
  skills = skill, bibit: install = pasang · update = upgrade, pupuk ·
    remove = uninstall, cabut · --project = --proyek
  list: all = semua · instances = instance, kebun · db = dbs, database,
    databases, blok · ports = port, lahan · connections = connection,
    conn, koneksi, konsesi · tools = tool, alat, perkakas · mcp = colokan ·
    skills = skill, bibit ·
    config = configs, konfig · keys = key, keymap, shortcuts, pintasan ·
    explorer = explorers, cloudflare, cf, jelajah, penjelajah
  bakar: ports = port, lahan · instances = instance, kebun
  -y = --yes, --ya (don't ask first) · --ulang = --restart
  `sawit ./panen` opens a folder named panen; so for every word here.

Options:
  -n, --new         always open in a new window
  -r, --reuse       use the frontmost window, replacing its active project
  -w, --wait        wait until every opened file is closed
  -                 read stdin into a temporary file and open it
  --                the rest are paths, even when they start with -
      --foreground  run the app in this terminal (debugging)
  -v, --version     print the version
  -h, --help        print this help
";
