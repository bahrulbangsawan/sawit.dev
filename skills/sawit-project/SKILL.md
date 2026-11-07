---
name: sawit-project
description: Set up projects and this Mac with Sawit: a new TypeScript project from Better-T-Stack's builder, developer tools (Bun, uv, Node, agents' CLIs) installed, upgraded or removed, MCP servers added to AI clients, skill packs installed into agents, and the shell and agents' config files listed and synced to GitHub. Use it before you curl an installer, hand-edit an MCP config or scaffold a project yourself (stack_create, install_dev_tools, install_mcp_server, install_skill).
---

# New projects and this Mac's setup, with Sawit

Sawit's New Project, Dev Tools, MCP Servers and Skills pages, and its Config Files panel, have tools that do what their buttons do, with the same checks. They all work without the app. Everything here installs, downloads or edits the user's own files: ask before each change.

## A new project

1. `stack_options` lists every category and option, the presets (MERN, PERN, T3, Uniwind) and the default stack, and `cli`: the `create-better-t-stack` version in use, npm's latest and what an upgrade dropped.
2. `stack_check` with the choices, by the CLI's own flag names (`frontend`, `backend`, `database`, `orm`, `auth`, `addons`, `package_manager`, …, or a `template`): the stack the compatibility rules resolve it to, what they changed and why, disabled options, the project name's problem, and the command.
3. `stack_create` with `location` (the parent folder), the same choices and `confirm: true` once the user agrees: with the app running, the New Project page runs it in a terminal tab of its own and opens the project (`open: false` only makes it); without the app it runs here and returns the output. It never writes over a folder that exists. `cli_version` runs another release for this one create; only the user upgrades the page's version (its Upgrade button).
4. Then the project's database (`sawit-databases`) and dev server (`sawit-terminal`).

## Developer tools

1. `list_dev_tools`: each tool, whether it is installed, its version, a newer release, prerequisites and the exact install, upgrade and uninstall commands.
2. `install_dev_tools` with `tools` (ids) and `confirm: true` runs each tool's official installer, prerequisites first; `package_manager` picks npm, pnpm, bun or yarn for the tools npm installs globally. One that asks for a password (Homebrew) comes back as a command for a terminal the user types in.
3. `upgrade_dev_tools` (no ids: every tool with a newer release) and `uninstall_dev_tools` (deletes the tool and what it installed; can't be undone) need `confirm: true` too.

## MCP servers and skills

- `list_mcp_servers`: Sawit's own server and the catalog, and which AI clients (Claude Code and Codex per profile, Cursor, VS Code, Zed, …) have each. `install_mcp_server` with `server` and `client` (`profile`, `region`, `package_manager`) edits that client's config, keeping a backup; GitHub's server uses Sawit's sign-in and is refused while the user is signed out. `uninstall_mcp_server` needs `confirm` equal to the server id.
- `list_skills`: the skill packs, their skills and sources, the install command per way and package manager, and which agents have each. `install_skill` with `skill`, `agents`, `method` and `confirm: true`; `uninstall_skill` needs `confirm` equal to the pack id.
- `install_sawit_skills` installs these Sawit skills into every agent, the short block into each agent's instruction file and Sawit's MCP server where it's missing (`scope: "project"`: only the block in one project's file; `remove: true` takes it out). Needs `confirm: true`.

## Config files

- `list_config_files` lists the shell startup files (which shell reads each, and when) and every coding agent's config files, by path, never their contents; `filter` narrows it. Read or edit a file yourself only when the task needs it, and say which.
- `shell_files_sync` with `action`: status (each shell file against the user's private sawit-shell-config repository), upload or download (`files` by name). An upload with lines that look like secrets is refused unless `allow_secrets`, and a file changed on both sides needs `overwrite_conflicts`: ask the user for both. A download backs each file up first; a file deleted on one side since the last sync is deleted on the other.

## Without the sawit MCP server

Tell the user, as the `sawit` skill says. In a shell: `sawit new` (the page) or `sawit new shop --template t3 --create`, `sawit list tools`, `sawit pasang <tool>` (install), `sawit pupuk` (upgrade), `sawit cabut <tool>` (uninstall), `sawit list mcp`, `sawit list skills`, `sawit skills install`, `sawit list config` and `sawit shell` (upload, download). Each asks first; `-y` answers yes.
