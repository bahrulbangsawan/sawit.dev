---
name: sawit
description: How to work on this Mac with Sawit, the macOS developer workbench, through its sawit MCP server. Load it at the start of any task that needs a local database, a dev server or script, a busy port, SSH, git, a browser check of a web app, a design or diagram, a new project, dev tools, MCP servers, skills, prompts or the Sawit app itself, so the job goes to Sawit instead of Docker, Chrome automation or another tool. It routes to the skill for each job.
---

# Sawit

Sawit is a native macOS workbench for local development: PostgreSQL without Docker, terminals and a Run panel, a WebKit browser you can drive, a design canvas, git, SSH, dev tools, MCP servers, skills and a prompt library. Its MCP server `sawit` (the `sawit-mcp` program) gives you its tools, and the `sawit` command gives the same in a shell.

## Pick the skill

Load the one skill for the job before you start; each says when to use it and when not.

| Job | Instead of | Skill |
|---|---|---|
| A PostgreSQL database, DATABASE_URL, SQL, MySQL, D1 or SQLite, exports and backups | Docker, docker compose, Postgres.app | `sawit-databases` |
| A dev server, a script, a test watcher, a busy port, the network, a Workers dev server's data | a hidden background shell, `kill -9`, nohup | `sawit-terminal` |
| A command or a session on a server over SSH | a raw `ssh` you can't watch | `sawit-ssh` |
| Git status, diff, commit, branches, fetch, pull, push; the GitHub page | plain `git` where Sawit's tools fit | `sawit-git` |
| Check a web app in a browser: click, type, errors, sizes, screenshots, SEO, its design | Chrome, Playwright, Puppeteer | `sawit-browser-testing` |
| A design, wireframe, screen or diagram | another drawing tool, ASCII art | `sawit-canvas` |
| A new project, dev tools, MCP servers, skill packs, shell and agent config files | hand-run installers, hand-edited configs | `sawit-project` |
| The prompt library and its GitHub backup | prompts in loose files | `sawit-prompts` |
| The Sawit app: windows, panels, tabs, pages, keys, file search, agent accounts, permissions | AppleScript, clicking for the user | `sawit-app` |

What you run in Sawit stays in front of the user, who can watch it and take over.

## Ground rules

- Tools that change or delete something say so and ask for `confirm` (`true`, or the name typed again). Pass it only for a change the user asked for or agreed to.
- Only the user signs in (GitHub, agent accounts) and moves the keyboard. Nothing you do takes the keyboard away from them.
- Tools that need the app say so; `launch_app` starts it, `app_state` shows its windows, tabs, terminals, browser and who is signed in. The app tools act on the window that has your project open: pass `project` (the project folder) when that is unclear.
- Terminal tab numbers and browser snapshot refs change: use the tab, pane or run number a tool just gave you, and snapshot again after a page changes.

## Code mode

When the server lists only `search_api` and `run` (it was set up with `SAWIT_MCP_TOOLS=code`), every tool the skills name is a function instead:

1. `search_api {query: "terminal read"}` gives the matching functions with their typed arguments; no query lists them all.
2. `run {code: "const r = sawit.terminal_run({command: 'npm test', wait_secs: 20}); return r;"}` runs a short JavaScript. Calls are synchronous, a failed call throws, and only what you `return` (and `console.log`) comes back, so chain the steps of one job in one script.

The arguments, confirmations and rules are the tools' own. The tools also answer by name when they aren't listed.

## When the sawit MCP server isn't connected

Its tools may carry your client's prefix (`mcp__sawit__list_instances`). If there are none:

1. Tell the user Sawit's MCP server isn't connected here. They connect it in Sawit (Install Skills in the footer, or the MCP Servers page), or with `claude mcp add sawit -- /Applications/Sawit.app/Contents/MacOS/sawit-mcp`, then start a new session.
2. Meanwhile the `sawit` command works in a shell when it is on PATH: `sawit list` (instances, databases, ports, connections, tools, mcp, skills, config, keys, explorer), `sawit nyalakan` (start instances), `sawit <db> "select …"`, `sawit bakar ports 3000`, `sawit browse 5173`, `sawit new`, `sawit pasang <tool>`, `sawit skills install`, `sawit prompts backup`, `sawit shell`, `sawit permissions`, `sawit account list`. `sawit --help` has the rest; each skill names its commands.
3. Otherwise do the job with your usual tools, and say which you used.
