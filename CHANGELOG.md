# Changelog

Every change to Sawit, by version. Each push to `main` is a new release: the patch version is bumped and
its section is written automatically by `.githooks/pre-push` (`scripts/release.sh`) from the commit messages.

## [0.0.8] - 2026-11-01

### Features

- **cli**: sawit list keys prints every shortcut while Sawit runs
- **mcp**: keymap_list, keymap_bind and keymap_unbind read and change your key bindings
- **mcp**: terminal_split splits a terminal pane and can run a command in the new half
- **cli**: sawit list config and sawit list mcp, and list_config_files for agents
- **tools**: uninstall dev tools from the terminal (sawit cabut) and from agents (uninstall_dev_tools)
- **mcp**: canvas tools take a file, to work on a design file or any canvas file in its own tab or on disk
- **cli**: daftar and stop are list and bakar's other names, and sawit --help lists every alias and flag
- **cli**: sawit browse opens a page, a port or the project's dev server in Sawit's browser
- **cli**: sawit nyalakan starts stopped Sawit instances, and --ulang restarts them
- **mcp**: every tool tells clients whether it only reads, can destroy, or reaches past this mac
- **catalog**: the Dev Tools and MCP pages work from the keyboard
- **terminal**: quitting, closing a window and restart to update ask while a terminal runs something
- **app**: searchable catalog for dev tools and mcp servers; config files for every coding agent
- **canvas**: replace the excalidraw webview with the native figcraft canvas
- **canvas**: exports never overwrite a canvas file; headless exports find system fonts
- **canvas**: export_svg and export_png ops on the native canvas
- **mcp**: browser_inspect, browser_design and browser_export_design
- **design**: assets, system tokens and export in the design panel
- **design**: inspect, colors and fonts in the design panel
- **design**: scan, inspect and background export in the browser tab
- **design**: asset naming, data URLs and capped downloads
- **design**: inspect details, contrast, CSS and Tailwind output
- **design**: DESIGN.md, DTCG, Tailwind and shadcn exports
- **mcp**: browser_annotations reads the user's visual feedback
- **annotate**: send annotations to the terminal running Claude
- **annotate**: annotate panel with screenshots and markers
- **annotate**: element details, key styles and source hints on pick
- **browser**: browser_resize, viewport in app_state, ports open in the browser
- **canvas**: import, save as, export and copy as from the canvas menus
- **browser**: toolbar toggles, side panel, ⌘⇧. and clean screenshots
- **browser**: device sizes with page zoom
- **canvas**: layers panel with pages, selection, rename, visibility, lock and drag
- **keymap**: keymap editor; every feature's shortcuts can be rebound
- **canvas**: fills, strokes, effects, typography and export in the design panel
- **canvas**: main menu, context menu, actions palette and shortcuts sheet
- **canvas**: design panel with position, appearance and auto layout
- **terminal**: split panes replace herdr; Sawit no longer offers or installs it
- **terminal**: run, ssh and agent tools address panes
- **canvas**: draw the arc and corner radius handles the move tool answers
- **terminal**: zoom panes and close exited shells in splits
- **terminal**: herdr-style pane menu, rename and close pane
- **terminal**: split panes with headers and draggable dividers
- **canvas**: floating toolbar, top bar and context bar on the native canvas
- **terminal**: read a session's working folder
- **canvas**: open .figcraft and .excalidraw files in canvas tabs
- **canvas**: type and compose text on the native canvas
- **canvas**: copy and paste between canvases and from other apps
- **mcp**: canvas_command and canvas_commands, and canvas tools that work on .figcraft files
- **canvas**: import excalidraw boards and migrate the project canvas on first open
- **canvas**: draw the view menu's outlines, soft proof and pixel preview
- **canvas**: figcraft shortcuts on the native canvas, sharing keys with sawit by policy
- **canvas**: turn mermaid into editable board shapes without a browser
- **canvas**: pointer tools and cursors on the native canvas
- **canvas**: get, add, update, delete, clear, replace and commands ops on the native canvas
- **canvas**: selection, handles, guides and labels drawn over the native canvas
- **canvas**: native canvas view with pan, zoom, dot grid and autosave
- **canvas**: apply element skeletons as board shapes, connectors, text and sections
- **canvas**: open and autosave .nyawit/canvas.figcraft with a backup before emptying
- **shell-config**: save your shell startup files to a private github repository, warning about secrets
- **mcp**: remove a server from every client at once and stop a hanging install
- **tools**: uninstall dev tools and stop an install that hangs
- **nyawit**: keep a project's canvas, diagrams and exports in its .nyawit folder
- **update**: check hourly and install a downloaded update when sawit quits
- **tools**: upgrade dev tools from the terminal (`sawit pupuk`) and MCP (`upgrade_dev_tools`)
- **mcp**: browser_* tools drive the browser tab
- **browser**: browser tab with toolbar, per-project storage and agent ops
- **tools**: detect newer versions and upgrade installed dev tools

### Fixes

- **db**: writing DATABASE_URL leaves a .env that Sawit can't read as it is, instead of replacing the whole file
- **mcp**: a canvas tool that names a file refuses to run when the Sawit app running is too old to know it
- **mcp**: browser_annotations names the key your keymap gives Annotate
- **mcp**: an agent's new terminal tab starts in the agent's own project, not the one in front
- **mcp**: terminal_close asks for confirm before it ends a program or the terminal you type in
- **mcp**: removing a host, prompt, connection or saved command, stopping processes and replacing DATABASE_URL need a confirmation
- **mcp**: start_instance says whether a held start can still be started anyway
- **tools**: the Tools page no longer offers to uninstall Homebrew's npm, which would remove Node.js
- **mcp**: a client config that doesn't parse is reported by line and column, never by quoting its line
- **mcp**: with no project named, an agent's canvas tools use the canvas of the project it works in
- **cli**: sawit asks before running a statement that writes
- **mcp**: canvas_get, canvas_clear and canvas_export say they act on the page on show
- **mcp**: browser_export_design asks for replace: true before it replaces an earlier export
- **canvas**: what an agent deletes from the canvas is kept in a .bak first, with Sawit open or closed
- **browser**: browser_network shows agents a page's passwords and tokens as [redacted]
- **mcp**: the browser tools tell agents that ⇧⌘. opens Annotate and what clearing the console or network log drops
- **editor**: kept changes for a file that can't be edited say where they are
- **editor**: Save in the close or quit question saves a read-only tab
- **keymap**: keymap.json is saved as Sawit saves every file, keeping its permissions
- **canvas**: the note of a canvas file saved again says to delete it again
- **workspace**: a project opened through a symlink opens at the folder it leads to
- **tools**: closing the window or quitting while Dev Tools installs asks first, and stops the installer
- **browser**: an agent's browser is its own project's, not the one you're looking at
- **browser**: the link question says what cancel does, and reload lets the page's links ask again
- **keymap**: the keymap editor an agent opens leaves the keyboard where you have it
- **browser**: send to terminal goes to the page's project and never into an SSH session
- **config**: saving shell files with a sign-in GitHub refuses asks for a new sign-in, not a sign-out
- **install**: in Sawit's own terminal, the install command leaves Sawit running and says where to run it
- **mcp**: a claude or codex CLI outside nvm's folders finds nvm's node, so installs through it work
- **mcp**: updating a server moves the PATH Sawit wrote with npx, so it still starts after nvm moves on
- **run**: stop while a restart waits for the old run calls the restart off
- **mcp**: app_activate_tab shows another project's tab with its project, as a click does
- **mcp**: app_close_tab says when it asked the user first, instead of "closed"
- **mcp**: only Claude Desktop and LM Studio entries pin where node is; editors keep your shell's PATH
- **files**: open project folder replaces the folder of the project that asked
- **tools**: retry after a failed uninstall asks to run that uninstall again, never the installer
- **files**: ⌘Z pressed during a change that fails no longer undoes the change before it
- **mcp**: git_switch, git_unstage and browser_navigate say they can replace what was there
- **terminal**: an agent's command never goes into the shell you're typing in
- **ui**: the status bar's project path is a Copy Path button, the full path in its tooltip
- **editor**: changes a quit kept come back with a warning when the file changed on disk in the meantime
- **files**: the name box lines up with the tree's outlined rows
- **editor**: a quit that can't ask keeps unsaved changes, and the next launch brings them back
- **editor**: quitting, Restart to Update and closing a window ask to save unsaved files first
- **files**: image, pdf, video and find in folder tabs follow a renamed or moved file
- **files**: the files tab's open project folder goes through the workspace's checks
- **files**: the files tab's git items follow the project's repository as it changes
- **files**: expand all leaves linked folders closed, and the refresh reads open folders in one go
- **files**: a file opened from the files tab takes the keyboard, and the tree shows when it has it
- **mcp**: the MCP page says what the built-in Sawit server lets agents do
- **github**: a saved sign-in GitHub no longer accepts asks you to sign in again, and git stops sending it
- **canvas**: on a design file, board shapes and diagrams say what the canvas is instead of pointing at a hidden menu
- **terminal**: a new or switched-to terminal tab scrolls into view in the tab strip
- **workspace**: a tab you open or switch to scrolls into view, not the one left of it
- **workspace**: the keyboard never stays on what is hidden
- **files**: ⌫ asks before moving a file or folder to the trash, and ⌘⌫ doesn't
- **install**: the install script points at Install CLI in the status bar, where it is
- **files**: while a name is typed in the tree, its other shortcuts leave it alone
- **install**: the install script waits for Sawit to quit before replacing it, and runs only once whole
- **files**: a paste or name that fails keeps the cut and the name typed, and an opened folder is read again
- **files**: the tree's keys act only on an entry whose row is shown
- **files**: work that lands after the files tab changes folder leaves the new folder alone
- **files**: file changes run one at a time, so ⌘Z undoes the change it follows
- **files**: redo of a trash moves the file to the trash again instead of taking its tabs along
- **mcp**: agents are told what a closed app keeps, that browser_* need the app, and what shell_config is
- **mcp**: browser_navigate with no url opens the agent's project's dev server
- **mcp**: browser_wait_for answers at once what waiting won't change
- **mcp**: canvas_command reads the files an engine command names as the agent means them
- **mcp**: canvas_export writes only an absolute path, and takes PNG in any case
- **mcp**: canvas_load, write_env and browser_screenshot read paths as every other tool does
- **canvas**: canvas_load brings back the board a clear kept in canvas.figcraft.bak
- **mcp**: launch_app opens the path it names, after --, and says what it opened
- **mcp**: terminal_run starts its shell in the folder it names, or runs nothing
- **mcp**: a project named through a link, in another case or with .. is the open one
- **mcp**: two agents editing one canvas while sawit is closed get true answers and lose no edit
- **search**: find in folder shows the latest search's hits and stops reading at its limit
- **files**: add to .gitignore and .git/info/exclude work in worktrees, on links and odd names
- **files**: duplicating or pasting a folder that holds a pipe or socket finishes, leaving them out
- **files**: a rename to another file's name in other case no longer replaces that file
- **workspace**: what is shown again has its keyboard back
- **catalog**: a page an agent opens leaves the keyboard where the user has it
- **canvas**: an agent's load or command that names a pipe or a link to a device is refused at once
- **mcp**: a detection that finishes under an open sign-in sheet no longer takes the app down
- **tools**: Zed or Cap from a Homebrew cask isn't offered their installer's removal
- **catalog**: paths under a home folder of a similar name aren't shortened, and the MCP search finds tags
- **ui**: the status bar's Git button shows ⌘3 in its tooltip, as the other buttons there do
- **config**: Config Files lists each agent profile as its own group, and only real profiles
- **canvas**: a panic in opening a file or in an agent's op is an error, not an abort of the app
- **canvas**: a Mermaid diagram stopped for taking too long isn't rendered a second time
- **canvas**: a blank file another app made keeps its page colour in the dark theme
- **canvas**: a save records the page on show, so a file opens on it again
- **canvas**: an agent's op waits while the user draws a path with the pen, instead of joining its undo step
- **canvas**: a batch step can't write over the canvas file through a path taken from another step
- **config**: deleting a config file for good names it by its path
- **config**: saving shell files to GitHub warns about secrets on every save, and finds more of them
- **browser**: browser_evaluate no longer leaves a false SyntaxError on the page
- **browser**: page scripts can no longer borrow the agent's functions to annotate
- **tools**: closing the Dev Tools tab mid-install asks and stops the installer, and one tool installs at a time
- **mcp**: a failed or stopped change shows on the server's row, naming the client
- **mcp**: an install's result lands on its own client when Detect Again reorders the list
- **mcp**: remote servers aren't written into Claude Desktop and other program-only clients without npx
- **workspace**: shortcuts keep working once the web page, tab or panel you typed in is hidden
- **mcp**: AWS installed in another Region reads as installed there, and the Region is checked
- **mcp**: the MCP page finds claude, codex and npx where the Dev Tools page does
- **tools**: uninstall runs a tool's own removal only on the copy its installer put in place
- **tools**: an installer that prints a line that isn't UTF-8 keeps its output and its pipe
- **tools**: dev tools work when the login shell is fish, nu, PowerShell or tcsh, and a failed detection says so
- **mcp**: two changes to one client config at once no longer lose an entry or break the file
- **mcp**: removing a server keeps the others in order, and config files keep their indentation
- **mcp**: an update keeps what you added to a server's entry, and Remove from N asks first
- **canvas**: closing or quitting a canvas it couldn't save keeps its edits in a copy
- **canvas**: opening a canvas file that is a pipe or a link to a device is refused at once
- **canvas**: a canvas that found no file refuses to save over one made there since
- **canvas**: a clear or a load keeps the five boards before it, not only the last
- **canvas**: an agent's cut or paste while the user types ends the typing first, so the next click keeps it
- **files**: saves, canvas backups and exports never write through a link a cloned repository holds
- **design**: exports of a dev site on a .test, ddev or lvh.me name keep its assets
- **browser**: a page can no longer open annotate and add an annotation of its own
- **browser**: an agent's click in a web page leaves your keyboard where it is
- **keymap**: the editor saves a binding for an action that had none
- **mcp**: agents name the terminal they type into, and app_run_script says where it runs
- **terminal**: the quit and restart question comes forward with its window
- **browser**: Annotate's key in a web page is the keymap's, not a fixed ⇧⌘.
- **browser**: ⌘C, ⌘V, ⌘X and ⌘A act in the web page, not in the field or panel used before it
- **canvas**: an agent's key on the canvas does what the keyboard does with the default keys
- **canvas**: the canvas's tooltips, menus, palette and shortcuts sheet show the keys as you bound them
- **keymap**: default shortcuts follow keyboard layouts that type keys elsewhere, as macOS menus do
- **keymap**: a chord you add no longer delays the canvas's ⌘K, and the editor warns of chords on bound keys
- **git**: the commit and comment boxes say ⌘↩ again
- **keymap**: Config Files' shortcuts go by its name in the keymap editor and keymap.json
- **workspace**: the title bar's Keymap Editor, Tools and MCP buttons bring the tab area back
- **keymap**: ⌘E no longer opens keymap.json from everywhere
- **keymap**: ⎋ and ↩ end key recording, and a hidden recorder lets go of the keyboard
- **keymap**: the keymap editor never writes over a keymap.json changed outside it
- **keymap**: a default turned off with null in keymap.json shows as removed
- **keymap**: a canvas key can be pointed at another command in the keymap editor
- **keymap**: the keymap editor no longer deletes your other binding of the same keys
- **keymap**: editing a binding records the new keys instead of adding them to the old ones
- **mcp**: app and terminal tools go to the window with the agent's project open
- **ssh**: a server's forwarded ports are let go seconds after its last session
- **ssh**: reconnect uses the server's settings as they are now
- **run**: restart waits for the old run to stop, however long that takes
- **terminal**: agents' terminal commands leave your keyboard alone and say where they ran
- **cli**: sawit --wait on a canvas file waits while its canvas is open
- **canvas**: one canvas per file holds for a path named through a link
- **canvas**: a canvas file's tab follows its file, and says when it saves a file that went
- **canvas**: the canvas you go to takes the keyboard
- **canvas**: a drag cut off by hiding the canvas's tab ends there, and agents' edits go through again
- **canvas**: a template, a table or an auto layout added while text is typed ends the typing first
- **canvas**: ⇧Space and ⌥⌘↩ no longer ask for a prototype player the canvas doesn't have
- **canvas**: a name typed in the layers panel stays when the next click lands on a layer
- **annotate**: send to terminal prefers the agent's terminal to an idle shell
- **design**: an export never writes through a symbolic link in the project
- **design**: exports of two dev servers on one host no longer wipe each other
- **annotate**: closing the browser or clearing no longer drops unsent notes without asking
- **browser**: an agent's read-only browser call no longer opens a browser over your tab
- **browser**: each project gets its own browser tab, cookies, screenshots and exports
- **ports**: clicking a port opens it in the browser even when the browser is open
- **design**: exports no longer save what is on your mac or network for a web page, nor send your curlrc
- **design**: size caps on stylesheet fetches and asset downloads hold for gzip answers
- **browser**: browser_evaluate works on pages whose CSP forbids eval
- **annotate**: page text can no longer add fake annotations to the notes sent to an agent
- **browser**: page scripts can no longer forge annotations or read the agent's token
- **browser**: window.open no longer replaces the page with a blank, a search or a local file
- **browser**: pages can no longer launch other apps without asking
- **browser**: an address webkit can't read is searched instead of crashing sawit
- **mcp**: terminal_run answers whatever wait_secs it gets
- **run**: running a script again runs it in its own folder
- **terminal**: dropping files on a pane makes it the tab's focused pane
- **terminal**: a shell an agent typed into is kept when the first project opens
- **terminal**: commands sent to the terminal never land in a run panel script's shell
- **terminal**: a tab's menu and buttons act on that tab after the strip changes
- **terminal**: a tab's × closes only that tab and hands on the keyboard it had
- **terminal**: closing a tab left of the active one keeps you in your tab
- **pages**: every page follows the window's width
- **files**: ⌫ after a folder change no longer trashes the previous folder's selection
- **keymap**: the editor no longer offers canvas commands the canvas has no use for
- **mcp**: the instructions say terminal tools address panes, not only tabs
- **mcp**: canvas_export writes a ~/ path in the home folder, not in a folder named ~
- **files**: ⌫ with nothing selected no longer moves the project folder to the trash
- **terminal**: commands sent to the terminal go to a shell at its prompt, never into a running program
- **files**: ⌫ while typing a name no longer trashes the selected entry or the project folder
- **browser**: ⇧⌘. toggles annotate when sawit has the keyboard, not only the page
- **keymap**: shifted symbols read as the keys pressed, ⇧⌘. rather than ⌘>
- **keymap**: shortcut hints follow rebinds
- **app**: typing a search in config files no longer opens or trashes the selected file
- **mcp**: no tool writes over a canvas file
- **terminal**: send to terminal pastes into the tab's focused pane
- **mcp**: app_open_page offers the keymap page
- **canvas**: an agent's edit waits for the user's drag to end
- **canvas**: agent ops and the layers panel end typing through one path
- **mcp**: the offline canvas saver refuses what would lose data
- **canvas**: a migrated arrow into a wide diamond joins it again
- **canvas**: migrated loops and sequence messages stay as excalidraw drew them
- **canvas**: end the text being typed before a right-click or a menu command
- **canvas**: design panel edits leave the canvas's open gestures alone
- **canvas**: removing a paint or an effect keeps the bindings after it
- **design**: sheet fetch rejects userinfo and checks the address unless the page is local
- **design**: local files are copied only for a page that is a file by the tab's URL
- **design**: skipped videos aren't failed downloads; chosen videos are saved
- **browser**: ⌘⇧. no longer matches ⌘⇧< on non-US layouts
- **design**: inspect picking only on the Inspect segment
- **browser**: a shot gives up after 10 s per step; a pick adds its row without one
- **design**: fetched sheets reach the page only as CSS, and local hosts only for their own page
- **design**: foreground is the most-used text color readable on the background
- **mcp**: a failed design export is reported as a tool error, with the failures first
- **design**: Reveal in Finder runs off the UI thread and reports failure
- **design**: saves report failures; reveal errors show; export line follows the host
- **design**: hover, pin and HTML copy no longer overtake each other; save notes
- **design**: screenshot the scanned page before downloads; page answers time out
- **design**: read oklch/lab/p3 colors, measure used fonts, cap the scan in bytes
- **design**: restrict asset URLs to http(s), end curl options, reuse base64_decode
- **design**: palette roles from opaque colors; muted skips the body colors
- **annotate**: a failed paste copies the annotations instead
- **annotate**: shots take turns so the overlay is never in a capture
- **annotate**: clip picked text in the page; component-only source hints render
- **browser**: clicks in the panel no longer take focus from its fields
- **browser**: the view keeps the keyboard so ⌘⇧. works from Sawit
- **browser**: overlay picks the walked-to parent, stays lazy and never takes pointer events
- **canvas**: end a colour pick before other input joins its undo step
- **canvas**: keep the context bar inside a narrow canvas and scroll its controls
- **mcp**: only terminal_close's pane defaults to the whole tab
- **terminal**: idle splits stay idle; panes keep the keyboard and their names
- **canvas**: keep the toolbar inside a narrow canvas and scroll its buttons
- **canvas**: keep the context bar over the toolbar
- **canvas**: centre a pasted image or svg in the view
- **canvas**: select the line on a triple click while typing, as figcraft does
- **canvas**: finish the pen's path on delete and resize with cmd and an arrow
- **terminal**: an exited pane closing keeps the keyboard where it is
- **terminal**: true 20x4 pane minimum, nested drag limits, live header folder
- **canvas**: widen the context bar's text-size menu to fit "Mixed"
- **canvas**: show the caret at the start of a line and in a new sticky
- **canvas**: save canvas files atomically and never over changes made on disk
- **canvas**: cut points, not the layer, with cmd-x in vector edit mode
- **canvas**: import arrowheads and open an empty old canvas blank
- **canvas**: paint only a frame of the page and document on show
- **canvas**: keep a lossy open's original apart from the emptying backup
- **canvas**: put back the file's versions and branches when a command op fails
- **canvas**: draw mermaid loops along their route and keep messages off notes
- **canvas**: cut frame names to their frames and guard the overlay build
- **canvas**: keep command ops from opening or switching documents
- **canvas**: leave select.area off the shortcut sheet
- **canvas**: keep an auto gap when a grid cell aligns a frame
- **canvas**: refuse layer drops that leave the layers where they are
- **canvas**: route canvas keys as figcraft's own keyboard does
- **terminal**: tab, shift-tab and cmd-c reach the shell and TUIs instead of the window
- **ports**: label deno tasks as `deno task dev`, not go-task's `task dev`
- **browser**: reject unknown key modifiers, tidy empty snapshot containers and the blank-page log line

### Performance

- **canvas**: read the system's fonts when a canvas first opens, not at every launch
- **mcp**: the MCP page reads each client config once per detection, off the async workers
- **canvas**: a hidden canvas tab lets go of its render buffers

## [0.0.7] - 2026-10-29

### Features

- **mcp**: run sql, start or stop an instance and diagnose the network in the running app

### Other

- update README, screenshots and demo for v0.0.6 screens: git, canvas, dev tools, mcp and exports

## [0.0.6] - 2026-10-29

### Features

- **cli**: sawit panen, backup, tanam, list tools and pasang
- **mcp**: tools for exports, backups, csv import, drop and truncate, dev tools and mcp servers
- quieter project labels in front of tab groups
- shell config menu to reveal, duplicate, trash or delete a file
- mcp page installs aws, cloudflare, neon, figma and other servers, with sign-in
- dev tools page to install claude code, bun, uv and more in one click
- **db**: export to csv or excel, pg_dump backups, csv import, drop, truncate and bulk row edits

### Fixes

- **diagrams**: wrap zoomed-out table names at underscores

### Performance

- turn mermaid into canvas shapes with pele and ship only the english canvas

### Other

- update README, screenshots and demo for exports, backups, dev tools, mcp catalog and new cli words
- add the /consolidate command
- format with rustfmt and fix clippy's octal-escape lint in tests
- remove the unused logo, probe example and old planning docs

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

