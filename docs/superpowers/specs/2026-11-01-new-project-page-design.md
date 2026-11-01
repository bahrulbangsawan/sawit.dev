# New project page: design

A **New Project** page where a person picks a TypeScript stack, sees the matching
`create-better-t-stack` command, and creates the project in Sawit's own terminal. When the
project is created it opens as a Sawit project. The model is Better-T-Stack's web builder
(https://www.better-t-stack.dev/new), rebuilt as a Sawit page in Sawit's look, not a copy of
their site.

Reference: upstream `AmanVarshney01/create-better-t-stack` (MIT), ported from commit
`a9bd456ccc9d`, CLI version `3.49.0`: `apps/web/src/lib/{constant,stack-validation,
stack-compatibility,sanitize-stack-addons,stack-utils,stack-model}.ts`,
`packages/types/src/compatibility.ts`, the stack builder components and
`apps/cli/test/compatibility.test.ts`.

## Why

Starting a project is the one thing a developer workbench can't do yet: Sawit opens folders
that exist. Better-T-Stack already knows which TypeScript pieces fit together and writes the
command; Sawit can show its choices natively, run the command in its own terminal, and open
the result, so a person (or an agent) goes from nothing to an open project in one place.

## What the user asked for

- A native page, `new_project`, titled "New Project", opened as a centre tab like Tools, MCP
  and Skills: from the Welcome screen, the File menu, the title bar and `ctrl-cmd-n`.
- Every upstream category and option, upstream's compatibility rules (disabled reasons and
  auto-adjusts), presets and default stack, ported into `crates/core` without GPUI.
- The command pinned to the ported CLI version, run in a Sawit terminal, the project opened
  when it succeeds.
- Agents through MCP (`stack_options`, `stack_check`, `stack_create`, `app_open_page`) and
  people through the CLI (`sawit new` / `sawit semai`).

## Goals

1. The default stack and each preset produce exactly upstream's command, apart from the pinned
   version (`@3.49.0` instead of `@latest`).
2. An incompatible pick either can't happen (the card is greyed, with upstream's reason as a
   tooltip) or auto-adjusts with a visible note, as upstream does.
3. Create Project runs in a Sawit terminal tab and opens the new project when it succeeds; a
   failure says so, leaves the terminal in front and offers Try Again.
4. The run never stops at a prompt the person can't see coming: every option is passed.
5. The page fills the window and works at the minimum window (720×480 with the sidebar open).

## Non-goals

- The file-tree preview (upstream renders it on a server with its template engine), share
  links, "random stack", the YOLO toggle, saving more than the last stack.
- Keeping the option data in sync with upstream automatically. Re-porting is manual, guided by
  the commit and version at the top of the core module.
- Addon option prompts (Fumadocs, MCP, Skills, OpenTUI, Ultracite, WXT templates): `create`
  has no flag for them, so in Sawit's terminal the CLI still asks, and the person answers there.

## Anti-goals (failure even if it works)

- A command on the page that differs from what upstream prints for the same stack.
- A create that overwrites or merges into an existing folder.
- Hotlinking upstream's icon CDN.

## User-facing behaviour

### Opening it

- Welcome screen, START section: "New Project…" (new `WelcomeEvent::NewProject`).
- File menu: "New Project…". Title bar: a "New" button beside Tools, MCP and Skills.
- `ctrl-cmd-n` (the uniqueness test `no_two_shortcuts_share_a_keystroke` allows it).
- One tab, key `new_project`, icon `FolderPlus`. Reopening restores the last stack, location
  and package manager from `<AppPaths root>/new-project.json` (written with
  `files::write_text_atomic`). The project name starts at the first free
  `my-better-t-app`, `my-better-t-app-2`, … in the location, so a reopened page doesn't start
  on a folder that exists.

### Layout

The shared page frame: `p_6`, `gap_4`, title leading, actions trailing, no `max_w` cap.

- **Header:** "New Project", a one-line description naming Better-T-Stack and the pinned CLI
  version ("create-better-t-stack 3.49.0"); trailing: Presets (menu: MERN, PERN, T3, Uniwind),
  Reset, Create Project (primary).
- **Left column** (a fixed 340 px beside a flexible right column; below 760 px of page width it
  stacks above the right column and the whole page scrolls as one):
  - Project Name, with upstream's validation shown inline; Location, a path button that opens
    `cx.prompt_for_paths(PathPromptOptions { directories: true, .. })`. Location defaults to the
    parent of the open project, else `~/Developer` if it exists, else `~`.
  - Command: monospace, wrapping, with Copy Command and the package-manager menu
    (`crate::package_managers::menu` over `package_manager::detect()`, limited to bun, pnpm and
    npm). A muted line says what Create Project adds so it never stops to ask.
  - The note: what the last pick changed ("Runtime set to 'none' (managed by convex)") and
    upstream's desktop build note when it applies.
  - Selected stack as removable tags (upstream's `getSelectedTechs`; × removes as upstream's
    badge does).
  - Category nav: one chip per category (a dot when chosen), which scrolls the right column to
    that section.
  - Create status: "Creating my-app…" with a spinner, "Creation failed, see the terminal" with
    Try Again, or why Create can't start ("~/Developer/my-app already exists").
- **Right column:** a filter box (`cmd-f`), then one section per category in upstream's order
  (Web Frontend, Native Frontend, Backend, Runtime, API, Database, ORM, Database Setup, Web
  Deploy, Server Deploy, Auth, Payments, Package Manager, Addons, Examples, Git, Install).
  Cards wrap (`h_flex().flex_wrap()`), each a logo tile, name, one-line description, "Default"
  or "Experimental" tags as upstream shows them, a check when selected. Disabled cards stay,
  greyed with a dashed border, and the reason is their tooltip and part of their
  accessibility label.
- **Keyboard:** `cmd-f` focuses the filter; ↑/↓ move through the visible cards and ↩ picks one
  (`catalog::init` bindings for the `NewProjectPage` context, plain keys only outside inputs).
  Cards carry a role and an `aria_label` (`catalog::label`).

### Picking

Picking follows upstream's `getTechSelectionUpdate` + `applyStackUpdate`: a disabled card does
nothing; web and native frontend are single picks that toggle back to none (upstream 3.49.0
sanitizes both to one value each; the brief's "multi-select" is upstream's list shape, and
`--frontend` still gets one web and one native); addons and examples toggle, with the task
runner group (Turborepo, Nx, Vite+) and the observability group (evlog, Axiom) keeping the
last one picked; Git and Install toggle; everything else is a single pick. Each pick resolves
the stack to a fixed point and the changes become the note.

### Create

1. Create Project refuses, inline, when the name is invalid, the location isn't a folder, or
   `<location>/<name>` exists.
2. The page emits `NewProjectEvent::Create`; the workspace leaves the welcome screen, shows the
   dock and runs the command with `TerminalPanel::start_job(JobTag { key:
   "new-project:<path>", name: "New Project: <name>" }, location, command, Some(Origin::User))`.
3. The page shows "Creating…" and polls `job_status` every 250 ms (the Run panel's pattern):
   the job ends when its command was seen running and the shell is idle again, or when the
   terminal is gone, or after 20 s idle without ever running.
4. Then: the folder has a `package.json` → the page emits `NewProjectEvent::Open(dir)` and the
   workspace opens it with `crate::cli::open_folder_in(dir, Some(window), cx)` in a `cx.defer`,
   as the Welcome handler does. Else the terminal stays in front and the page says "Creation
   failed, see the terminal" with Try Again.
5. While creating, the page is at work: closing its tab asks first, and quitting or closing the
   window lists it (`new_project_page::at_work` beside the Tools and Skills pages').

### The command

`command(stack)` (the package manager is part of the stack, as upstream's is) is upstream's `generateStackCommand` with the runner pinned:
`bun create better-t-stack@3.49.0`, `pnpm create better-t-stack@3.49.0`,
`npx create-better-t-stack@3.49.0`; `--yes` alone for the default stack with bun, every flag
otherwise. `run_command(stack)` is what Create Project types: the command with `npx -y`
for npm (npx asks before downloading otherwise), then `--directory-conflict error --open none`
(the CLI's directory prompt and its "Open project with?" picker), and `--manual-db` when the
database setup would ask how to provision (Turso, Neon, Prisma Postgres, Supabase, MongoDB
Atlas) and Alchemy doesn't manage it. Checked against the CLI's own `--help` and `--dry-run`.

## Architecture

### What lives where

- `crates/core/src/stack_builder/` (new; `rules.rs`, `tests.rs` and the fixture `upstream.json` beside it): the port. Typed enums per category (generated by a
  small macro with `ALL`, `id`, `parse` and serde as upstream's ids), `Stack` (one web and one
  native frontend, `Vec<Addon>`, `Vec<Example>`, `git`/`install` as bools, `package_manager`, `project_name`),
  the catalog (`CATEGORIES`, `options(category)` with name, description, default,
  experimental), `PRESETS`, `Stack::default()`, and:
  - `resolve(stack) -> Resolved { stack, changes, notes }`: sanitize, then upstream's
    `analyzeStackCompatibility` until nothing changes (10 passes at most); `notes` is the
    desktop build note (upstream's per-category notes are always empty).
  - `disabled_reason(stack, category, option) -> Option<String>`.
  - `select` and `remove`: upstream's card click and badge ×.
  - `selected(stack)`: the tags. `command(stack)`, `run_command(stack)` and `quiet_flags(stack)`.
  - `validate_project_name`, `Choices` (a partial stack by upstream's CLI flag names, for MCP
    and the CLI; `backend: "self"` maps to the fullstack backend of the web frontend).
- `crates/core/src/new_project.rs` (new): what the page, MCP and the CLI share around a create:
  the remembered state file, the default location, the first free name, the target check, the
  "created" check (`package.json`), and a headless create (run the command in a login shell,
  stdin closed, with a timeout) for `stack_create` without the app and `sawit new --create`.
- `crates/app/src/new_project_page.rs` (new): the page. Draws only; every rule is core's.
- `crates/app/src/workspace/new_project.rs` (new): opening the tab, the event handling, the
  at-work check. `workspace.rs` gets small additive hooks (the `mod`, the binding, the action
  handler, the title-bar button, the Welcome arm, the `flags()`/`close()` lines).
- `assets/stack_logos/` (new) with `SOURCES.md`: Simple Icons (CC0) SVGs where one exists,
  else a Lucide glyph or the option's initial. Never upstream's CDN.

### Agents and the CLI

- `Page::NewProject` (+ round-trip test), the `open_page` arm, `PAGES` and the
  `app_open_page` description in `crates/mcp/src/app.rs`.
- `Command::NewProject { location, stack }` (the stack carries its name and package manager): the app opens the page with
  that stack and starts the create as Create Project does (keyboard left alone, `Origin::Agent`),
  answering the terminal tab and pane, the project folder and the command.
- `crates/mcp/src/new_project.rs` (new), working without the app: `stack_options` (categories,
  options, defaults, presets, the pinned version), `stack_check` (a partial stack → resolved
  stack, changes, notes, disabled reasons, command and run command, name error), and
  `stack_create` (`confirm: true`; with the app running, the app's terminal and the project
  opened; else the headless create, returning its output).
- `sawit new` / `sawit semai`: no arguments opens the page (starting the app); with flags
  (`--template`, upstream's stack flag names, `--package-manager`, `--dir`) it prints the
  resolved command and what changed (Indonesian, as the rest of `sawit`); `--create` runs it
  here and says how to open the result. Both words join `SUBCOMMANDS`; `sawit ./new` still
  opens a folder.

## Constraints

- No `unwrap` outside tests; every `pub` item documented; Title Case labels and tooltips.
- Shared files (`workspace.rs`, `app_control.rs`, `mcp/src/lib.rs`, `mcp/src/app.rs`,
  `welcome.rs`, `e2e/specs`) get small additive edits only; new code lives in new files.
- Credits: Better-T-Stack's MIT licence in `NOTICE` and README `## Credits`.

## Testing

- Core: golden commands for the default stack and each preset with bun, pnpm and npm; upstream's
  CLI rejection cases (`apps/cli/test/compatibility.test.ts`) as builder cases (each is
  auto-adjusted or disabled with upstream's reason); and a differential fixture generated by
  running upstream's own TypeScript (60 random stacks: resolved stack, changes, every disabled
  reason, tags, desktop note, name error, command; 40 random click walks of 10 picks or
  removals each). Run commands checked by the CLI's `--dry-run` outside the test suite.
- App: GPUI page tests (picking, a disabled card does nothing, the filter, keyboard, Create
  refusing an existing folder, the create flow opening the project or failing) and a
  `debug_bounds` layout test at 720 px and 1470 px; a workspace test that the tab opens and
  counts as at work while creating.
- MCP and CLI: tool tests, parser tests, `PAGES` in `e2e/specs/pages.test.js` (the e2e suite
  never presses Create Project).
- The real app: screenshots at 1470 px and 720×480, one real create (default stack, bun) in a
  scratch folder, opened as a project, then deleted.

## Calls I made (for review)

- **Web and native frontend are single picks**, as upstream 3.49.0 sanitizes them.
- **Package manager appears twice**: the menu beside the command (the brief's) and upstream's
  Package Manager category of cards; both change the same choice, and a package manager this
  Mac lacks is greyed with "Not installed on this Mac".
- **The page shows upstream's command; Create Project runs `run_command`**, which adds only
  what keeps the CLI from asking, and the page says what it adds.
- **The project name isn't remembered**; it starts at the first free name in the location.
- **Upstream's preset names keep their own project name out**: applying a preset keeps the
  name typed.

## Implementation plans

`docs/superpowers/plans/2026-11-01-new-project-page.md`: core rules, page, create flow, agent
and CLI parity.
