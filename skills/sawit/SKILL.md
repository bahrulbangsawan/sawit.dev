---
name: sawit
description: How to work on this Mac with Sawit, the macOS developer workbench, through its sawit MCP server. Use it at the start of any task that needs a local database, a dev server or script, a busy port, a browser check of a web app, a design, wireframe or diagram, or SSH, so the job goes to Sawit instead of Docker, Chrome automation or another tool. It routes to sawit-databases, sawit-browser-testing, sawit-canvas and sawit-terminal.
---

# Sawit

Sawit is a native macOS workbench for local development: PostgreSQL without Docker, terminals and a Run panel, a WebKit browser you can drive, a design canvas, git, SSH and dev tools. Its MCP server `sawit` (the `sawit-mcp` program) gives you its tools. Most work without the app; `launch_app` starts it for the `app_*`, `terminal_*`, `browser_*` and `keymap_*` tools.

## Pick the skill

| Job | Instead of | Skill |
|---|---|---|
| A PostgreSQL database, a DATABASE_URL, SQL, D1 or SQLite | Docker, docker compose, Postgres.app | `sawit-databases` |
| Check a web app in a browser: click, type, read errors | Chrome, Playwright, Puppeteer | `sawit-browser-testing` |
| A design, wireframe or diagram | another drawing tool, ASCII art | `sawit-canvas` |
| A dev server, a script, a busy port, SSH | a hidden background shell, `kill -9` | `sawit-terminal` |

Load that skill before you start the job. What you run in Sawit stays in front of the user, who can watch it and take over.

## Ground rules

- Tools that change or delete something say so and ask for `confirm` (`true`, or the name again). Pass it only for a change the user asked for or agreed to.
- The app tools act on the window that has your project open; pass `project` (the project folder) when that is unclear. `app_state` lists the windows, tabs, terminals and the browser.
- Also there: `git_status`, `git_diff`, `git_commit` and the other git tools, `search_files`, `list_config_files` (agent and shell config paths), `list_dev_tools` and `install_dev_tools`, `install_mcp_server`, `list_skills`.

## When the sawit MCP server isn't connected

Its tools may carry your client's prefix (`mcp__sawit__list_instances`). If there are none:

1. Tell the user Sawit's MCP server isn't connected here. They connect it in Sawit (Install Skills in the footer, or the MCP Servers page), or with `claude mcp add sawit -- /Applications/Sawit.app/Contents/MacOS/sawit-mcp`, then start a new session.
2. Meanwhile the `sawit` command works in a shell when it is on PATH: `sawit list` (instances, databases, ports), `sawit nyalakan` (start instances), `sawit <db> "select …"`, `sawit bakar ports 3000`, `sawit browse 5173`. `sawit --help` has the rest.
3. Otherwise do the job with your usual tools, and say which you used.
