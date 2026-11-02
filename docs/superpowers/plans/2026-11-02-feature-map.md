# Feature map: the files each feature owns, before and after the split

Before: `main` at 6935d18, 322 hand-written source files, 133 over 500 lines. After: `main` with this map, 1148 files, 0 over 500 lines (`scripts/check-line-limit.sh` keeps it so).

Each feature was split by one or more lanes, each owning its files alone. A file that was over 500 lines became a module folder (`foo.rs` -> `foo/mod.rs` plus parts, tests in `foo/tests.rs` or `foo/tests/`); public paths stayed the same through `pub use`. Line counts before are the baseline's: 666702d (the GitHub MCP server, from another session) later grew `mcp_page.rs`, `mcp_clients.rs`, `mcp_catalog.rs`, `kebun.rs` and both `cli.rs` before they were split.

| Feature | Lanes | Files before | Over 500 before | Files after | Largest after |
|---|---|---|---|---|---|
| board/canvas | 6 | 54 | 36 | 281 | 488 |
| terminal | 2 | 15 | 7 | 65 | 439 |
| browser | 3 | 27 | 16 | 152 | 496 |
| db | 2 | 37 | 15 | 106 | 497 |
| mcp | 3 | 33 | 15 | 118 | 494 |
| workspace/shell | 5 | 69 | 26 | 190 | 495 |
| core services | 3 | 38 | 16 | 156 | 486 |
| site | 1 | 13 | 2 | 42 | 258 |

## board/canvas

### board-canvas

- `crates/app/src/board/mod.rs` (6065 -> 372)
- `crates/app/src/board/events.rs` (501) -> `crates/app/src/board/events/`: 2 files, largest 442
- `crates/app/src/board/commands.rs` (583) -> `crates/app/src/board/commands/`: 2 files, largest 324
- `crates/app/src/board/worker.rs` (681) -> `crates/app/src/board/worker/`: 2 files, largest 387
- `crates/app/src/board/toolbar.rs` (601) -> `crates/app/src/board/toolbar/`: 3 files, largest 308
- `crates/app/src/board/option_icons.rs` (674) -> `crates/app/src/board/option_icons/`: 2 files, largest 451
- `crates/app/src/board/context_bar.rs` (433)
- `crates/app/src/board/overlay_paint.rs` (268)
- `crates/app/src/board/top_bar.rs` (262)
- `crates/app/src/board/paint.rs` (146)
- `crates/app/src/board/text_input.rs` (488)

New files: 40, largest 451 lines.

### board-panels

- `crates/app/src/board/layers_panel.rs` (2469) -> `crates/app/src/board/layers_panel/`: 10 files, largest 421
- `crates/app/src/board/presence.rs` (1626) -> `crates/app/src/board/presence/`: 6 files, largest 476
- `crates/app/src/board/menus.rs` (1611) -> `crates/app/src/board/menus/`: 9 files, largest 361
- `crates/app/src/board/design_panel/appearance.rs` (167)
- `crates/app/src/board/design_panel/effects.rs` (567) -> `crates/app/src/board/design_panel/effects/`: 2 files, largest 377
- `crates/app/src/board/design_panel/export.rs` (711) -> `crates/app/src/board/design_panel/export/`: 3 files, largest 264
- `crates/app/src/board/design_panel/layout.rs` (383)
- `crates/app/src/board/design_panel/mod.rs` (1592 -> 276)
- `crates/app/src/board/design_panel/number_field.rs` (641) -> `crates/app/src/board/design_panel/number_field/`: 4 files, largest 314
- `crates/app/src/board/design_panel/paints.rs` (1146) -> `crates/app/src/board/design_panel/paints/`: 5 files, largest 361
- `crates/app/src/board/design_panel/position.rs` (119)
- `crates/app/src/board/design_panel/typography.rs` (484)

New files: 45, largest 476 lines.

### board-ops

- `crates/board/src/ops.rs` (5328) -> `crates/board/src/ops/`: 25 files, largest 416
- `crates/board/src/store.rs` (2273) -> `crates/board/src/store/`: 10 files, largest 411
- `crates/board/src/doc.rs` (780) -> `crates/board/src/doc/`: 3 files, largest 377
- `crates/board/src/clipboard.rs` (667) -> `crates/board/src/clipboard/`: 2 files, largest 389
- `crates/board/src/summary.rs` (547) -> `crates/board/src/summary/`: 2 files, largest 392
- `crates/board/src/actions.rs` (341)
- `crates/board/src/policy.rs` (265)
- `crates/board/src/fonts.rs` (83)
- `crates/board/src/lib.rs` (33)

New files: 42, largest 416 lines.

### board-formats

- `crates/board/src/mermaid.rs` (2859) -> `crates/board/src/mermaid/`: 15 files, largest 337
- `crates/board/src/excalidraw.rs` (2540) -> `crates/board/src/excalidraw/`: 13 files, largest 417
- `crates/board/src/skeleton.rs` (2212) -> `crates/board/src/skeleton/`: 11 files, largest 321
- `crates/core/src/mermaid.rs` (928) -> `crates/core/src/mermaid/`: 5 files, largest 260
- `crates/core/tests/mermaid_it.rs` (123)

New files: 44, largest 417 lines.

### board-interaction

- `crates/board/src/overlay.rs` (2648) -> `crates/board/src/overlay/`: 14 files, largest 313
- `crates/board/src/input.rs` (1948) -> `crates/board/src/input/`: 10 files, largest 357
- `crates/board/src/view.rs` (955) -> `crates/board/src/view/`: 5 files, largest 317
- `crates/board/src/shortcuts.rs` (931) -> `crates/board/src/shortcuts/`: 5 files, largest 452
- `crates/board/src/toolbar.rs` (985) -> `crates/board/src/toolbar/`: 7 files, largest 339
- `crates/board/src/context_bar.rs` (1196) -> `crates/board/src/context_bar/`: 7 files, largest 357

New files: 48, largest 452 lines.

### board-model

- `crates/board/src/layers.rs` (1507) -> `crates/board/src/layers/`: 9 files, largest 394
- `crates/board/src/menu.rs` (1095) -> `crates/board/src/menu/`: 7 files, largest 320
- `crates/board/src/design/effects.rs` (915) -> `crates/board/src/design/effects/`: 6 files, largest 298
- `crates/board/src/design/export.rs` (677) -> `crates/board/src/design/export/`: 2 files, largest 445
- `crates/board/src/design/fields.rs` (230 -> 235)
- `crates/board/src/design/layout.rs` (814) -> `crates/board/src/design/layout/`: 4 files, largest 387
- `crates/board/src/design/lists.rs` (273 -> 275)
- `crates/board/src/design/mod.rs` (1104 -> 273)
- `crates/board/src/design/paints.rs` (1015) -> `crates/board/src/design/paints/`: 7 files, largest 304
- `crates/board/src/design/testing.rs` (63)
- `crates/board/src/design/typography.rs` (346)

New files: 41, largest 445 lines.


## terminal

### terminal-panel

- `crates/app/src/terminal/panel.rs` (3425) -> `crates/app/src/terminal/panel/`: 14 files, largest 439
- `crates/app/src/terminal/guard.rs` (887) -> `crates/app/src/terminal/guard/`: 6 files, largest 355
- `crates/app/src/terminal/mod.rs` (63)
- `crates/app/src/terminal/agents.rs` (231)
- `crates/app/src/terminal/testing.rs` (121)

New files: 20, largest 439 lines.

### terminal-view

- `crates/app/src/terminal/tab.rs` (2807) -> `crates/app/src/terminal/tab/`: 13 files, largest 396
- `crates/app/src/terminal/view.rs` (1279) -> `crates/app/src/terminal/view/`: 8 files, largest 340
- `crates/core/src/terminal/agents.rs` (1072) -> `crates/core/src/terminal/agents/`: 5 files, largest 406
- `crates/core/src/terminal/input.rs` (320)
- `crates/core/src/terminal/layout.rs` (406)
- `crates/core/src/terminal/mod.rs` (30)
- `crates/core/src/terminal/palette.rs` (77)
- `crates/core/src/terminal/session.rs` (685) -> `crates/core/src/terminal/session/`: 5 files, largest 226
- `crates/core/src/terminal/tab.rs` (215)
- `crates/core/src/terminal/tests.rs` (917) -> `crates/core/src/terminal/tests/`: 6 files, largest 384

New files: 37, largest 406 lines.


## browser

### browser-app

- `crates/app/src/browser/agents.rs` (299)
- `crates/app/src/browser/annotate.rs` (1979) -> `crates/app/src/browser/annotate/`: 9 files, largest 350
- `crates/app/src/browser/design/assets.rs` (533) -> `crates/app/src/browser/design/assets/`: 2 files, largest 420
- `crates/app/src/browser/design/colors.rs` (450) -> `crates/app/src/browser/design/colors/`: 2 files, largest 422
- `crates/app/src/browser/design/flow.rs` (1696) -> `crates/app/src/browser/design/flow/`: 7 files, largest 391
- `crates/app/src/browser/design/fonts.rs` (507) -> `crates/app/src/browser/design/fonts/`: 2 files, largest 427
- `crates/app/src/browser/design/inspect.rs` (661) -> `crates/app/src/browser/design/inspect/`: 3 files, largest 475
- `crates/app/src/browser/design/mod.rs` (629 -> 496)
- `crates/app/src/browser/design/system.rs` (574) -> `crates/app/src/browser/design/system/`: 3 files, largest 365
- `crates/app/src/browser/engine.rs` (365 -> 461)
- `crates/app/src/browser/mod.rs` (3223 -> 463)
- `crates/app/src/browser/panel.rs` (274)
- `crates/app/src/browser/presence.rs` (387)
- `crates/app/src/browser/toolbar.rs` (475 -> 482)

New files: 47, largest 475 lines.

### browser-agent

- `crates/core/src/browser.rs` (2810) -> `crates/core/src/browser/`: 25 files, largest 343
- `crates/core/src/browser_agent.js` (808) -> `crates/core/src/browser/page/agent.js`, `crates/core/src/browser/page/agent/`: 10 files, largest 229
- `crates/core/src/browser_overlay.js` (1597) -> `crates/core/src/browser/page/overlay.js`, `crates/core/src/browser/page/overlay/`: 14 files, largest 217
- `crates/core/src/browser_design.js` (1148) -> `crates/core/src/browser/page/design.js`, `crates/core/src/browser/page/design/`: 12 files, largest 389

New files: 61, largest 389 lines.

### browser-design

- `crates/core/src/annotations.rs` (1032) -> `crates/core/src/annotations/`: 6 files, largest 345
- `crates/core/src/site_design/assets.rs` (1732) -> `crates/core/src/site_design/assets/`: 13 files, largest 332
- `crates/core/src/site_design/color.rs` (459 -> 466)
- `crates/core/src/site_design/export.rs` (773) -> `crates/core/src/site_design/export/`: 6 files, largest 275
- `crates/core/src/site_design/inspect.rs` (328)
- `crates/core/src/site_design/mod.rs` (44)
- `crates/core/src/site_design/scan.rs` (267 -> 276)
- `crates/core/src/site_design/tailwind.rs` (447)
- `crates/core/src/site_design/tokens.rs` (1413) -> `crates/core/src/site_design/tokens/`: 7 files, largest 406

New files: 32, largest 406 lines.


## db

### db-app

- `crates/app/src/db/actions.rs` (143)
- `crates/app/src/db/diagram.rs` (1401) -> `crates/app/src/db/diagram/`: 7 files, largest 427
- `crates/app/src/db/lifecycle.rs` (379)
- `crates/app/src/db/logos.rs` (102)
- `crates/app/src/db/mod.rs` (18)
- `crates/app/src/db/panel.rs` (2561) -> `crates/app/src/db/panel/`: 8 files, largest 479
- `crates/app/src/db/sql.rs` (411)
- `crates/app/src/db/store.rs` (749) -> `crates/app/src/db/store/`: 3 files, largest 285
- `crates/app/src/db/table.rs` (2073) -> `crates/app/src/db/table/`: 9 files, largest 438
- `crates/app/src/servers/detail.rs` (628) -> `crates/app/src/servers/detail/`: 3 files, largest 341
- `crates/app/src/servers/list.rs` (140)
- `crates/app/src/servers/mod.rs` (10)
- `crates/app/src/servers/store.rs` (497)
- `crates/app/src/local_explorer.rs` (107)

New files: 30, largest 479 lines.

### db-core

- `crates/core/src/db/admin.rs` (359)
- `crates/core/src/db/conn.rs` (797) -> `crates/core/src/db/conn/`: 4 files, largest 309
- `crates/core/src/db/d1.rs` (403)
- `crates/core/src/db/diagram.rs` (631) -> `crates/core/src/db/diagram/`: 5 files, largest 174
- `crates/core/src/db/explorer.rs` (404 -> 394)
- `crates/core/src/db/export.rs` (800) -> `crates/core/src/db/export/`: 4 files, largest 347
- `crates/core/src/db/mod.rs` (125 -> 145)
- `crates/core/src/db/profile.rs` (1112) -> `crates/core/src/db/profile/`: 5 files, largest 407
- `crates/core/src/db/query.rs` (1041) -> `crates/core/src/db/query/`: 7 files, largest 292
- `crates/core/src/db/remote.rs` (271)
- `crates/core/src/db/rows.rs` (255)
- `crates/core/src/db/sqlite.rs` (1021) -> `crates/core/src/db/sqlite/`: 6 files, largest 309
- `crates/core/src/db/tests.rs` (844) -> `crates/core/src/db/tests/`: 5 files, largest 311
- `crates/core/src/db/tls.rs` (224)
- `crates/core/src/cloudflare.rs` (1293) -> `crates/core/src/cloudflare/`: 7 files, largest 411
- `crates/core/src/cloudflare/explorer.rs` (1111) -> `crates/core/src/cloudflare/explorer/`: 5 files, largest 396
- `crates/core/src/instances.rs` (467)
- `crates/core/src/control.rs` (323)
- `crates/core/src/pgctl.rs` (75)
- `crates/core/src/binaries.rs` (290)
- `crates/core/src/download.rs` (143)
- `crates/core/tests/db_it.rs` (1197) -> `crates/core/tests/db_it/`: 6 files, largest 288
- `crates/core/tests/control_it.rs` (186)

New files: 54, largest 411 lines.


## mcp

### mcp-app

- `crates/mcp/src/app.rs` (4144) -> `crates/mcp/src/app/`: 21 files, largest 450
- `crates/mcp/src/agent.rs` (106)
- `crates/core/src/app_control.rs` (922) -> `crates/core/src/app_control/`: 4 files, largest 494
- `crates/core/src/agents.rs` (791) -> `crates/core/src/agents/`: 5 files, largest 256
- `crates/app/src/agents.rs` (306 -> 307)

New files: 36, largest 494 lines.

### mcp-tools

- `crates/mcp/src/lib.rs` (1564 -> 284)
- `crates/mcp/src/main.rs` (8)
- `crates/mcp/src/browser.rs` (1045) -> `crates/mcp/src/browser/`: 4 files, largest 362
- `crates/mcp/src/browser_design.rs` (272)
- `crates/mcp/src/db.rs` (1315) -> `crates/mcp/src/db/`: 7 files, largest 292
- `crates/mcp/src/db_admin.rs` (543) -> `crates/mcp/src/db_admin/`: 2 files, largest 417
- `crates/mcp/src/db_d1_tests.rs` (351)
- `crates/mcp/src/ssh.rs` (684) -> `crates/mcp/src/ssh/`: 4 files, largest 244
- `crates/mcp/src/dev_tools.rs` (669) -> `crates/mcp/src/dev_tools/`: 4 files, largest 283
- `crates/mcp/src/instances.rs` (567) -> `crates/mcp/src/instances/`: 2 files, largest 475
- `crates/mcp/src/local_explorer.rs` (541) -> `crates/mcp/src/local_explorer/`: 2 files, largest 433
- `crates/mcp/src/config_files.rs` (326)
- `crates/mcp/src/diagrams.rs` (335)
- `crates/mcp/src/files.rs` (58)
- `crates/mcp/src/git.rs` (431 -> 494)
- `crates/mcp/src/keymap.rs` (470)
- `crates/mcp/src/mcp_servers.rs` (402 -> 476)
- `crates/mcp/src/network.rs` (193 -> 199)
- `crates/mcp/src/new_project.rs` (369)
- `crates/mcp/src/ports.rs` (336)
- `crates/mcp/src/prompts.rs` (316)
- `crates/mcp/src/scripts.rs` (313)
- `crates/mcp/src/skills.rs` (471)
- `crates/mcp/tests/tools.rs` (634) -> `crates/mcp/tests/tools/`: 3 files, largest 422

New files: 28, largest 475 lines.

### mcp-page

- `crates/app/src/mcp_page.rs` (3032) -> `crates/app/src/mcp_page/`: 15 files, largest 460
- `crates/core/src/mcp_clients.rs` (2831) -> `crates/core/src/mcp_clients/`: 17 files, largest 365
- `crates/core/src/mcp_catalog.rs` (707) -> `crates/core/src/mcp_catalog/`: 3 files, largest 302
- `crates/core/src/mcp_auth.rs` (467)

New files: 35, largest 460 lines.


## workspace/shell

### ws-shell

- `crates/app/src/workspace.rs` (4313) -> `crates/app/src/workspace/`: 19 files, largest 436
- `crates/app/src/workspace/control.rs` (1042) -> `crates/app/src/workspace/control/`: 4 files, largest 386
- `crates/app/src/workspace/explorer.rs` (92)
- `crates/app/src/workspace/new_project.rs` (249)
- `crates/app/src/workspace/project.rs` (616) -> `crates/app/src/workspace/project/`: 2 files, largest 433
- `crates/app/src/cli.rs` (1388) -> `crates/app/src/cli/`: 6 files, largest 375
- `crates/app/src/main.rs` (175)
- `crates/app/src/tabs.rs` (376)
- `crates/app/src/tab_groups.rs` (73)
- `crates/app/src/layout.rs` (85)
- `crates/app/src/states.rs` (272)
- `crates/app/src/welcome.rs` (456)
- `crates/app/src/theme.rs` (19)
- `crates/app/src/rt.rs` (29)
- `crates/app/src/testing.rs` (61)
- `crates/app/src/assets.rs` (386 -> 387)
- `crates/app/src/update.rs` (195)
- `crates/app/src/changelog_page.rs` (134)

New files: 31, largest 436 lines.

### ws-editor

- `crates/app/src/files/mod.rs` (2478 -> 365)
- `crates/app/src/files/ops.rs` (160)
- `crates/app/src/files/search.rs` (302)
- `crates/app/src/editor/diagrams.rs` (325)
- `crates/app/src/editor/kept.rs` (505) -> `crates/app/src/editor/kept/`: 3 files, largest 365
- `crates/app/src/editor/mod.rs` (1165 -> 441)
- `crates/app/src/editor/testing.rs` (94)
- `crates/app/src/highlight.rs` (1063) -> `crates/app/src/highlight/`: 4 files, largest 430
- `crates/app/src/highlight/grammars.rs` (1328) -> `crates/app/src/highlight/grammars/`: 4 files, largest 485
- `crates/app/src/file_icons.rs` (328 -> 347)
- `crates/app/src/code_actions.rs` (63)
- `crates/app/src/viewers/image.rs` (166)
- `crates/app/src/viewers/mod.rs` (57)
- `crates/app/src/viewers/pdf.rs` (230)
- `crates/app/src/viewers/video.rs` (359)

New files: 25, largest 485 lines.

### ws-pages

- `crates/app/src/tools_page.rs` (2311) -> `crates/app/src/tools_page/`: 11 files, largest 445
- `crates/app/src/skills_page.rs` (1920) -> `crates/app/src/skills_page/`: 8 files, largest 401
- `crates/app/src/new_project_page.rs` (1424) -> `crates/app/src/new_project_page/`: 6 files, largest 308
- `crates/app/src/new_project_page/tests.rs` (363 -> 364)
- `crates/app/src/catalog.rs` (966) -> `crates/app/src/catalog/`: 6 files, largest 321
- `crates/app/src/package_managers.rs` (104)

New files: 31, largest 445 lines.

### ws-keymap

- `crates/app/src/keymap/control.rs` (807) -> `crates/app/src/keymap/control/`: 2 files, largest 424
- `crates/app/src/keymap/editor.rs` (1871) -> `crates/app/src/keymap/editor/`: 10 files, largest 425
- `crates/app/src/keymap/file.rs` (624) -> `crates/app/src/keymap/file/`: 2 files, largest 384
- `crates/app/src/keymap/mod.rs` (1308 -> 225)
- `crates/app/src/keymap/names.rs` (287)
- `crates/app/src/keymap/recorder.rs` (240)
- `crates/app/src/shell_config.rs` (1104) -> `crates/app/src/shell_config/`: 5 files, largest 350
- `crates/app/src/network.rs` (599) -> `crates/app/src/network/`: 2 files, largest 306

New files: 27, largest 425 lines.

### ws-sidebars

- `crates/app/src/git/account.rs` (812) -> `crates/app/src/git/account/`: 3 files, largest 446
- `crates/app/src/git/commit.rs` (201 -> 213)
- `crates/app/src/git/diff.rs` (208)
- `crates/app/src/git/github.rs` (1214) -> `crates/app/src/git/github/`: 5 files, largest 317
- `crates/app/src/git/graph.rs` (382 -> 387)
- `crates/app/src/git/mod.rs` (103)
- `crates/app/src/git/panel.rs` (1018) -> `crates/app/src/git/panel/`: 4 files, largest 337
- `crates/app/src/git/store.rs` (320 -> 324)
- `crates/app/src/git/thread.rs` (1236) -> `crates/app/src/git/thread/`: 7 files, largest 361
- `crates/app/src/run/mod.rs` (8)
- `crates/app/src/run/panel.rs` (1368) -> `crates/app/src/run/panel/`: 6 files, largest 325
- `crates/app/src/run/panel/explorer.rs` (371)
- `crates/app/src/run/store.rs` (103)
- `crates/app/src/prompts/mod.rs` (9)
- `crates/app/src/prompts/panel.rs` (276)
- `crates/app/src/prompts/store.rs` (151)
- `crates/app/src/prompts/view.rs` (334)
- `crates/app/src/ports.rs` (646) -> `crates/app/src/ports/`: 3 files, largest 252
- `crates/app/src/ssh/mod.rs` (10)
- `crates/app/src/ssh/panel.rs` (502) -> `crates/app/src/ssh/panel/`: 2 files, largest 293
- `crates/app/src/ssh/store.rs` (348)
- `crates/app/src/ssh/view.rs` (495)

New files: 30, largest 446 lines.


## core services

### core-cli

- `crates/core/src/kebun.rs` (3356) -> `crates/core/src/kebun/`: 22 files, largest 382
- `crates/core/src/cli.rs` (1026) -> `crates/core/src/cli/`: 10 files, largest 281
- `crates/core/src/dev_tools.rs` (2587) -> `crates/core/src/dev_tools/`: 14 files, largest 472
- `crates/core/src/update.rs` (211)
- `crates/core/src/semai.rs` (348)
- `crates/core/src/package_manager.rs` (423)
- `crates/core/src/keymap.rs` (251)
- `crates/core/tests/release_it.rs` (1212) -> `crates/core/tests/release_it/`: 6 files, largest 406

New files: 53, largest 472 lines.

### core-agents

- `crates/core/src/skills.rs` (1779) -> `crates/core/src/skills/`: 9 files, largest 323
- `crates/core/src/stack_builder/mod.rs` (1367 -> 360)
- `crates/core/src/stack_builder/rules.rs` (1156) -> `crates/core/src/stack_builder/rules/`: 4 files, largest 415
- `crates/core/src/stack_builder/tests.rs` (486)
- `crates/core/src/agent_config.rs` (820) -> `crates/core/src/agent_config/`: 3 files, largest 453
- `crates/core/src/scripts.rs` (752) -> `crates/core/src/scripts/`: 3 files, largest 447
- `crates/core/src/new_project.rs` (246)
- `crates/core/src/nyawit.rs` (231)
- `crates/core/src/prompts.rs` (384)

New files: 25, largest 453 lines.

### core-system

- `crates/core/src/github.rs` (1462) -> `crates/core/src/github/`: 10 files, largest 306
- `crates/core/src/git.rs` (1196) -> `crates/core/src/git/`: 9 files, largest 396
- `crates/core/src/files.rs` (1378) -> `crates/core/src/files/`: 13 files, largest 224
- `crates/core/src/ssh.rs` (1119) -> `crates/core/src/ssh/`: 8 files, largest 266
- `crates/core/src/ports.rs` (869) -> `crates/core/src/ports/`: 6 files, largest 307
- `crates/core/src/probe.rs` (524) -> `crates/core/src/probe/`: 5 files, largest 178
- `crates/core/src/shell_config.rs` (571) -> `crates/core/src/shell_config/`: 4 files, largest 203
- `crates/core/src/owners.rs` (247)
- `crates/core/src/network.rs` (446)
- `crates/core/src/search.rs` (210)
- `crates/core/src/format.rs` (368)
- `crates/core/src/keychain.rs` (210)
- `crates/core/src/recent.rs` (150)
- `crates/core/src/media.rs` (172)
- `crates/core/src/pdf.rs` (188)
- `crates/core/src/cmd.rs` (48)
- `crates/core/src/date.rs` (47)
- `crates/core/src/rt.rs` (109)
- `crates/core/src/error.rs` (44)
- `crates/core/src/lib.rs` (55 -> 56)
- `crates/core/tests/ports_it.rs` (198)

New files: 55, largest 396 lines.


## site

### site

- `site/cloudflare.config.ts` (13)
- `site/public/install.sh` (0)
- `site/src/changelog.ts` (65)
- `site/src/components/chrome.tsx` (134)
- `site/src/components/interactive.tsx` (129)
- `site/src/content.ts` (140 -> 9)
- `site/src/router.tsx` (6)
- `site/src/routes/__root.tsx` (77 -> 78)
- `site/src/routes/changelog.tsx` (111)
- `site/src/routes/index.tsx` (531 -> 39)
- `site/src/styles.css` (1506 -> 23)
- `site/src/vite-env.d.ts` (1)
- `site/vite.config.ts` (10)

New files: 29, largest 258 lines.


## New files outside the lanes (the line-limit check)

- `crates/core/tests/line_limit.rs` (21)
- `scripts/check-line-limit.sh` (33)
