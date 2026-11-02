# Speedups of the 500-line split (2026-11-02)

Every `perf(...)` commit since 6935d18, by feature: what got faster and its lane's before -> after numbers (each commit's subject and body say how it was measured). Lanes measured with thread CPU time or wall time, median of 7 or more runs, old and new binaries in turns. Where a lane could only measure the debug build (its shared release-bench slot had an hour's queue), the speedup was measured again afterwards in an optimized build (`--release`, LTO off, 16 codegen units), on the final code with the commit reverted against the final code itself: the last column. Gains that come from doing the work off the UI thread, in parallel, or with fewer processes don't depend on the build and weren't measured again.

Every code change was reviewed for a change anyone could see; the six dropped and the eight fixed after review are in `2026-11-02-split-open-items.md`.

## board/canvas

| Commit | What got faster | Before -> after (lane) | Build | Optimized build, before -> after |
|---|---|---|---|---|
| `819b165` | menu shortcut labels from a table built once | context menu 1.37 ms -> 8.7 µs; shortcut_label 65 µs -> 74 ns (debug: label 65 -> 0.44 µs, menu 1.22 ms -> 30 µs) | release + debug | measured by the lane (previous column) |
| `d3c749d` | main menu laid out once, state filled per canvas | 330 -> 45 µs; with 819b165 6.2 ms -> 45 µs (debug 3.4 -> 0.23 ms; 11.8 -> 0.14 ms) | release + debug | measured by the lane (previous column) |
| `b9b37ba` | Design panel reads a layer's numbers as floats in place | 1 rect 20.3 -> 13.9 µs; 130 layers 2.59 -> 2.08 ms (debug 118 -> 70 µs; 16.5 -> 9.6 ms) | release + debug | measured by the lane (previous column) |
| `7368a0f` | list rows' whole numbers made integers in place | effects rows 2.31 -> 1.19 µs; fills 665 -> 389 ns (debug 13.1 -> 6.1 µs) | release + debug | measured by the lane (previous column) |
| `ec63b22` | presence: where an agent op landed, without walking every layer | add on 6000 layers 27.1 -> 2.53 ms; change 1 of 6000 34.0 -> 2.72 ms; 100 of 600 2.69 -> 0.43 ms | debug | 11.25 ms -> 275 µs ([how](#ec63b22)) |
| `552fb95` | palette searches once per query, not per frame | frame for "dup" 147 -> 42 µs ("z" 290 -> 202 µs) | debug | 6.40 µs -> 2.69 µs ([how](#552fb95)) |
| `246bd1d` | Design panel reads paint/effects list once per frame | rich selection frame 7.2 -> 2.6 ms | debug | 250.1 µs -> 176.9 µs ([how](#246bd1d)) |
| `3e80a22` | app toolbar tool groups built once per file kind | toolbar render 922 -> 469 µs; view render 1454 -> 987 µs; frame 19.14 -> 18.52 ms | debug (thread CPU) | reverted by 23447a1: since aed6254 and 895cf2d cache the same work it saves nothing (optimized 42.4 -> 45.6 µs, paired 1.004; debug no difference either) |
| `ffede7d` | agent `get` finds connector labels in one pass | summarize 2000 elements 103 -> 35 ms CPU | debug | 24.87 ms -> 3.19 ms ([how](#ffede7d)) |
| `906183b` | replies move payloads instead of copying via json! | get 2000 elements 54 -> 35 ms CPU; node.get reply 139 -> 38 ms; commands 2.6 -> 0.8 ms (with ffede7d: get ~122 -> 35 ms) | debug | 4.62 ms -> 3.03 ms ([how](#906183b)) |
| `aed6254` | canvas default keys table built once | toolbar::groups 398 -> 71 µs; sheet() 3030 -> 570 µs; primary up to 127 -> 9 µs (release wall: groups 1545 -> 5.5 µs, sheet 4236 -> 103 µs) | debug + release (wall) | measured by the lane (previous column) |
| `fcc008c` | keymap lookup only when its answer is used | unbound key 11.6 -> 0.65 µs; char in sticky 2.66 -> 0.46 µs (release wall 33.3 -> 0.06 µs) | debug + release (wall) | measured by the lane (previous column) |
| `c5663c5` | key bindings built once | agent key 1095 -> 40 µs; key_bindings() 1065 -> 20 µs | debug | 72.0 us -> 4.71 us ([how](#c5663c5)) |
| `895cf2d` | board toolbar groups built once per kind | groups 71 -> 4.9 µs; tools flyout 75 -> 2.4 µs | debug | 2.89 us -> 0.74 us ([how](#895cf2d)) |
| `b6c5a38` | overlay works out selection frame once per frame | 1000 selected 4.04 -> 3.51 ms; 300 frames 1.35 -> 1.23 ms | debug | 1.322 ms -> 1.079 ms ([how](#b6c5a38)) |
| `51d3a1d` | outline without a throwaway list | 1000 selected 3.51 -> 3.06 ms | debug | 1.079 ms -> 1.031 ms ([how](#51d3a1d)) |
| `641e999` | skeleton apply sets each kept paint on all layers in one node.set | 799 skeletons 178.8 -> 151.4 ms CPU (199: 14.9 -> 12.9; 399: 48.8 -> 41.5); re-measured at merge after the repeat fix | debug | 145.96 ms -> 113.45 ms ([how](#641e999)) |

## terminal

| Commit | What got faster | Before -> after (lane) | Build | Optimized build, before -> after |
|---|---|---|---|---|
| `89781fd` | snapshot row by row, colours once | 200x50 snapshot 258 -> 107 µs | release | measured by the lane (previous column) |
| `b480cbd` | text_tail (terminal_read) walks up from the bottom | last 200 of 10k lines 70.9 -> 0.33 ms; last 50: 67.5 ms -> 85 µs | release | measured by the lane (previous column) |
| `34986de` | frame cache, only changed rows rebuilt | unchanged 203x50 frame 1181 -> 739 µs; selected 1189 -> 1093; scrolled 1214 -> 1098 (GPUI test platform, no glyphs) | release | measured by the lane (previous column) |
| `52da994` | tab strip reads script state once, copies less | dock render 584 -> 496 µs; changed parts 85 -> 44 µs/frame | debug | 49.3 µs -> 43.0 µs ([how](#52da994)) |

## browser

| Commit | What got faster | Before -> after (lane) | Build | Optimized build, before -> after |
|---|---|---|---|---|
| `11993a6` | page_script spliced once | 101 -> 4.3 µs (O0); 33 -> 4.4 µs (O3) | O0 + O3 (rustc harness) | measured by the lane (previous column) |
| `4e50bec` | redact_json asks once per JSON name | 4.2 KB list 383 -> 152 µs (O0); 44 -> 19 µs (O3); identical on 474,142 inputs | O0 + O3 (rustc harness) | measured by the lane (previous column) |
| `f2c3c1d` | page logs/requests/hovers don't redraw the view (+ caption, toolbar-address fixes) | view notifications per 300 msgs 300 -> 0 | count (test) | a count, the same in any build |
| `93a97c3` | Design panel font list cached, refreshed on CoreText notification | 46.6 ms CoreText per Fonts/Inspect render -> 0 | release (scratch harness of the call) | measured by the lane (previous column) |
| `de9f6b5` | Colors tab built once per scan | render 732.7 -> 23.1 ms | debug | 22.30 ms -> 2.42 ms ([how](#de9f6b5)) |
| `f74ab98` | Annotate groups notes without cloning | render 2.09 -> 1.33 ms | debug | 283.3 us -> 163.8 us ([how](#f74ab98)) |
| `ca3ddb1` | screenshot scaling off the main thread | main thread per full-view shot 8.9-10.0 s -> 0; element 0.14-0.31 s -> 0 | debug | not measured again: the main thread no longer does the scaling at all, in any build |
| `4bd9b64` | decode_data_url without copying payload | 60 data URLs 38.2 -> 28.2 ms | release | measured by the lane (previous column) |
| `0b94647` | inline asset decoded once when saving | 100 inline assets 95.9 -> 60.0 ms | release | measured by the lane (previous column) |
| `b191ef3` | CSS color parse without allocating | classes 3.96 -> 3.56 µs | release | measured by the lane (previous column) |
| `18775d8` | scan colors merged via map, OKLab once | derive 18.6 -> 8.9 ms (color step 15.5 -> 6.0 ms) | debug | 1.87 ms -> 0.378 ms ([how](#18775d8)) |
| `79f4b15` | variables sorted by reference, only kept 200 cloned | derive 8.99 -> 8.66 ms | debug | 414 us -> 375 us ([how](#79f4b15)) |
| `0a56dd6` | merge_rules checks names against a set | 4.15 -> 1.18 ms at caps; small page 73 -> 118 µs (+45 µs) | debug | 327 us -> 84.1 us ([how](#0a56dd6)) |

## db

| Commit | What got faster | Before -> after (lane) | Build | Optimized build, before -> after |
|---|---|---|---|---|
| `950ae9a` | connection tree flattened only on change | render 1.18 ms -> 0.45 µs; filtered 226 µs -> 0.45 µs (debug 4.00 ms -> 3.9 µs) | release + debug | measured by the lane (previous column) |
| `c8b149f` | grid cell text kept with its page | 40x12 frame 117 -> 4.3 µs (debug 2.62 ms -> 37 µs) | release + debug | measured by the lane (previous column) |
| `3301f48` | header tick reads page keys in place | 1000 rows 78 µs -> 4 ns; all ticked 269 -> 158 µs (debug 230 µs -> 35 ns) | release + debug | measured by the lane (previous column) |
| `41c03a1` | diagram card text once per load, binary-search matches | 40 cards 152 -> 2.4 µs; search 199 -> 20 µs (debug 245 -> 14 µs) | release + debug | measured by the lane (previous column) |
| `c2cf87b` | diagram built from schema tree in place | reload 5.6 -> 3.2 ms (debug 12.0 -> 8.2 ms) | release + debug | measured by the lane (previous column) |
| `e5cc463` | no paths for off-canvas edges | 67 of 299 in view 24.8 -> 5.9 ms/frame (debug 405 -> 92 ms) | release + debug | measured by the lane (previous column) |
| `dcb5b3c` | PG load_schema finds relations in a map | 1500 tables client CPU 222.1 -> 101.7 ms | debug | 17.24 ms -> 5.52 ms ([how](#dcb5b3c)) |
| `0eb41e6` | SQLite blob hex into one String | 100 x 4 KiB 65.1 -> 15.4 ms | debug | 12.9 ms -> 0.332 ms ([how](#0eb41e6)) |
| `8c13059` | CSV fields checked/quoted without copying | 20k rows x 8 244.4 -> 45.7 ms | debug | 2.83 ms -> 1.48 ms ([how](#8c13059)) |
| `ce3edb3` | one percent-encoder; D1 location borrowed | location() x999 7.29 -> 3.20 ms | debug | 805 us -> 397 us ([how](#ce3edb3)) |
| `fabed17` | ERD layout one slot buffer | 2000 tables 7.33 -> 6.10 ms | debug | 484 us -> 273 us ([how](#fabed17)) |
| `90103eb` | SQLite load_schema without sqlite_master scan per index | 300 tables 349.1 -> 57.6 ms (lane number, before the merge fix; not re-measured) | debug | 42.3 ms -> 9.39 ms ([how](#90103eb)) |

## mcp

| Commit | What got faster | Before -> after (lane) | Build | Optimized build, before -> after |
|---|---|---|---|---|
| `2654bbc` | diagnose_network pings router beside other checks | 1.744 -> 0.887 s | debug (concurrency) | not measured again (doesn't depend on the build) |
| `a6bb21c` | git_diff up to 8 files at once, order kept | 24 files 1.119 -> 0.471 s | debug | not measured again (doesn't depend on the build) |
| `9633665` | list_instances pg_ctl status at once | 4 instances 41.6 -> 14.5 ms | debug | not measured again (doesn't depend on the build) |
| `47e088f` | describe_table 4 catalog queries pipelined | 28.7 -> 23.9 ms | debug | not measured again (doesn't depend on the build) |
| `c60022f` | canvas op sent on the connection that found the app | connections/call add 2 -> 1, add+file 3 -> 2, get 3 -> 1; instructions/6000 calls 11.154 G -> 10.471 G (-6.1%); get 148 -> 90 µs | debug (dev build) | not measured again (doesn't depend on the build) |
| `8c9ae0b` | app version asked once per app (socket identity key) | app_state per canvas-file op 1 -> 0; connections 2 -> 1; 500 -> 143 µs/call; instructions 10.436 G -> 5.583 G (-46.5%) | debug (dev build) | not measured again (doesn't depend on the build) |

## workspace/shell

| Commit | What got faster | Before -> after (lane) | Build | Optimized build, before -> after |
|---|---|---|---|---|
| `82e6beb` | commit/PR diff parsed off the UI thread | UI thread 960 ms -> 0 (1.6 MB patch); 283 KB 39.7 ms -> 0 | debug | not measured again (doesn't depend on the build) |
| `c5e7bcc` | PR timeline grouped on change, not 2x per render | 143 µs -> 0 per render at 200 events | debug | 1321.0 us -> 1272.8 us ([how](#c5e7bcc)) |
| `acf5a00` | commit graph lanes off the UI thread | UI thread 9.9 ms -> 0 (2,587 commits) | debug | not measured again (doesn't depend on the build) |
| `c19fad7` | status, branches, remote read at once | refresh 160 -> 81 ms | debug (per lane; body silent) | not measured again (doesn't depend on the build) |
| `b663cd0` | prompt search / repo filter once per render | saves 211-265 µs (200 prompts), 107-159 µs (300 repos) per render | debug | 1216.7 us -> 1069.2 us; 895.8 us -> 796.3 us ([how](#b663cd0)) |
| `02c0611` | Git sidebar rows built on store change, no status copy | 5,000 files 4.50 -> 0.71 ms/render; 1,000: 1.01 -> 0.52 ms | debug | 463.3 us -> 38.0 us ([how](#02c0611)) |
| `b36c806` | sawit-mcp answers written off the UI thread (+ quit-wait fix) | 4 MiB: UI held 11.7 ms -> 11.3 µs; 1 MiB 3.32 ms -> 8.1 µs (receive time ~same) | release | measured by the lane (previous column) |
| `8257a34` | New Project nav picks once per frame | 81 -> 6 µs | debug | 57.1 us -> 47.6 us ([how](#8257a34)) |
| `af369b1` | keymap editor/form rows kept until store changes | filter key + frame 36.1 -> 21.3 ms; form frame 101.6 -> 81.4 ms; recorded key 97.6 -> 83.0 ms | debug | 1188.8 us -> 768.2 us ([how](#af369b1)) |
| `cebb5a7` | Files tree rows flattened only on change | 20,734 rows 3.44 ms -> 83 ns/render (debug 105 ms -> 208 ns) | release + debug | measured by the lane (previous column) |
| `6abcb00` | dirty check compares rope in place | 1 MB: 160 µs -> 42 ns/keystroke (debug 501 µs -> 42 ns) | release + debug | measured by the lane (previous column) |
| `781e613` | preview copy only in files that preview | 1 MB 74.1 -> 35.6 µs, 0 in code files (debug 67 -> 32 µs) | release + debug | measured by the lane (previous column) |
| `6b1416d` | file icons without allocating | 40 rows 56.4 -> 39.4 µs (debug 223 -> 197 µs) | release + debug | measured by the lane (previous column) |

## core services

| Commit | What got faster | Before -> after (lane) | Build | Optimized build, before -> after |
|---|---|---|---|---|
| `899c7db` | sawit list / sawit <db> read only passwords they use | list 1.41 -> 1.03 s; list connections 201 -> 0.5 ms; find .env db 258 -> 66 ms | debug | not measured again: the time is Keychain reads through `security`, which don't depend on the build |
| `2ea55a9` | sawit list runs lsof/ps once | 1.03 -> 0.79 s | debug | not measured again (doesn't depend on the build) |
| `4d6e290` | pg_ctl status for all instances at once | list instances 42 -> 14 ms; bakar instances 43 -> 13 ms | debug | not measured again (doesn't depend on the build) |
| `bbcac60` | strip_jsonc no longer copies the rest per comma | 9 KB 42.2 -> 0.95 ms; 38 KB 767 -> 5.0 ms; identical on 168,420 inputs | debug | 1.01 ms -> 56.6 us ([how](#bbcac60)) |
| `01e2a6b` | lockfile lookup once per folder per scan | 40-package monorepo 5.55 -> 4.81 ms | debug | 1.52 ms -> 1.13 ms ([how](#01e2a6b)) |
| `4da861a` | listening_jobs lsof + ps at once | median 140 -> 98 ms | debug | not measured again (doesn't depend on the build) |
| `88491d3` | git status + rev-parse MERGE_HEAD at once | median 70 -> 44 ms (930 files) | debug | not measured again (doesn't depend on the build) |
| `76c09d2` | list_dir no stat per file, lowercase once | 2220 entries 8.7 -> 1.7 ms; 55: 109 -> 52 µs | release | measured by the lane (previous column) |
| `4a3bf3d` | check_port asks docker ps + every listener at once | 2 listeners median 97 -> 93 ms | debug | not measured again (doesn't depend on the build) |

## site

| Commit | What got faster | Before -> after (lane) | Build | Optimized build, before -> after |
|---|---|---|---|---|
| `d1fd28b` | home copy out of the shared entry chunk | entry 114366 -> 112426 B gz (/changelog 115220 -> 113277) | - | bytes of the production build (previous column) |
| `0fa9a65` | closing app icon 1024 -> 512 px | 217217 -> 64866 B (SSIM 0.9993 at 3x) | - | bytes of the production build (previous column) |
| `068fe34` | icon lazy-loaded (was preloaded in head) | first load 1848525 -> 1783341 B | - | bytes of the production build (previous column) |
| `e9dda50` | 8 screenshots progressive JPEG q0.6 | 1331848 -> 900730 B (SSIM >= 0.993) | - | bytes of the production build (previous column) |
| `7f4c0ee` | stop requesting Plex Serif 300 upright | fonts CSS 11886 -> 9895 B | - | bytes of the production build (previous column) |
| `b7a41df` | dead CSS removed | 24731 -> 24697 B (5770 -> 5757 gz) | - | bytes of the production build (previous column) |
| `a2bae58` | /assets/* immutable (public/_headers) | repeat visit 3 revalidations -> 0 | - | bytes of the production build (previous column) |

## How the optimized-build numbers were taken

Four agents, one per crate group, each built the crate's tests optimized (`cargo test --release`, LTO off, 16 codegen units) twice: the final code, and the final code with the group's speedups reverted (or a commit's own parent where two commits share a path). The same throwaway harness ran in both, timing thread CPU time, median of 7 runs, the two binaries in turns for 3 to 25 rounds; outputs were checked identical between the two trees. The scenarios, per commit:

- <a id="de9f6b5"></a>`de9f6b5`: Design panel, Colors tab: one DesignPanel::render over a scan at the page script's caps (500 colours, 400 text samples, 400 assets, 500 variables). Test: browser::bench_release::colors_tab_render Optimized: 22.30 ms -> 2.42 ms (faster). Debug, as the lane measured: Colors median 732.7 ms -> 23.1 ms per render; the first render (builds the palette) 32.0 ms
- <a id="f74ab98"></a>`f74ab98`: Annotate panel: one AnnotatePanel::render with 30 notes like real picks (60 computed styles, ~1.7 KB of markup, 12 attributes, DOM path, neighbours and source each; 10 notes on each of 3 pages). Test: browser::bench_release::annotate_panel_render Optimized: 283.3 us -> 163.8 us (faster). Debug, as the lane measured: median 2.086 ms -> 1.334 ms per render (min 1.478 -> 1.279 ms)
- <a id="c5e7bcc"></a>`c5e7bcc`: Pull request thread, one ThreadView::render on the Timeline tab with a synthetic 200-event timeline (runs of commits, comments and reviews with ~1 KB bodies). Test: git::thread::bench_release::thread_timeline_render Optimized: 1321.0 us -> 1272.8 us (faster). Debug, as the lane measured: 143 us -> 0 per render at 200 events: one grouping 71 us, done twice per render before and not at all after (100 events 36 us, 500 events 150 us per grouping)
- <a id="b663cd0"></a>`b663cd0`: Prompts sidebar, one whole frame (window.refresh() in an update, drawn as effects flush, so the render and the virtual list's row callback both run). 200 prompts of ~2 KB, search "review", 280x900 window. Test: prompts::panel::bench_release::prompts_sidebar_frame Optimized: 1216.7 us -> 1069.2 us (faster). Debug, as the lane measured: prompt search, 200 prompts of ~2 KB: "review" 211 us, "unclear tests" 265 us, no query 23 us (the second search per render removed)
- `b663cd0`: GitHub tab, Your Repositories: one whole frame with 300 repositories, search "project", 1000x800 window. Test: git::github::bench_release::repos_page_frame Optimized: 895.8 us -> 796.3 us (faster). Debug, as the lane measured: 300 repositories: "project" 107 us, "whom it" 159 us, no query 64 us per filter (the second filter per render removed)
- <a id="02c0611"></a>`02c0611`: Git sidebar: one GitPanel::render with a synthetic status of 5,000 changed files and 60 branches. Test: git::bench_release::git_sidebar_render Optimized: 463.3 us -> 38.0 us (faster). Debug, as the lane measured: render 4.50 -> 0.71 ms with 5,000 changed files (1,000 files 1.01 -> 0.52 ms; 20 files 0.55 -> 0.48 ms)
- <a id="8257a34"></a>`8257a34`: New Project page, Jump To nav: one render_nav (17 chips) over the page's default stack. Test: new_project_page::left::bench_release::nav_render Optimized: 57.1 us -> 47.6 us (faster). Debug, as the lane measured: the nav's picks 81 us -> 6 us a frame (default stack, medians of 200 runs)
- <a id="af369b1"></a>`af369b1`: Keymap editor over the app's own bindings (456 rows, bound as the lane's debug harness bound them): a key typed in the filter after "clos" ("e" and backspace by turns), with its frame. Test: keymap::editor::bench_release::keymap_editor Optimized: 1188.8 us -> 768.2 us (faster). Debug, as the lane measured: a key typed in the filter, with its frame 36.1 -> 21.3 ms (frame with the form open 101.6 -> 81.4 ms; key recorded in the form + frame 97.6 -> 83.0 ms; controls: KeymapStore::rows 13.3/13.2 ms, editor frame 72.8/70.5 ms)
- <a id="18775d8"></a>`18775d8`: tokens::derive on a scan at the page script's caps (500 colors in 60 hues of near shades, a fifth translucent; 400 text styles; 1000 variables), using the site-design lane's generator verbatim; also derive's color step (derive_colors) on its own Optimized: 1.87 ms -> 0.378 ms (faster). Debug, as the lane measured: derive 18.6 -> 8.9 ms (-52%), its color step 15.5 -> 6.0 ms (-61%); type step and a small scan unchanged
- <a id="79f4b15"></a>`79f4b15`: tokens::derive on the same big scan (1000 variables sorted by name, 200 kept) Optimized: 414 us -> 375 us (faster). Debug, as the lane measured: derive 8.99 -> 8.66 ms (-3.6%, about 320 us); color and type steps within 1.4%
- <a id="0a56dd6"></a>`0a56dd6`: Scan::merge_rules with 500 page + 500 sheet variables (the page script's caps; every tenth sheet variable also on the page; 20 + 30 font faces), each call on a fresh copy of the scan and the rules; and a small page (40 + 40); the lane's scenario verbatim Optimized: 327 us -> 84.1 us (faster). Debug, as the lane measured: caps 4.15 -> 1.18 ms (-72%, copy included: 0.16 ms); small page 73 -> 118 us (+45 us)
- <a id="0eb41e6"></a>`0eb41e6`: sqlite::value_text on 100 blobs of 4 KiB (the db lane's blob) Optimized: 12.9 ms -> 0.332 ms (faster). Debug, as the lane measured: 100 x 4 KiB blobs: 65.1 -> 15.4 ms
- <a id="8c13059"></a>`8c13059`: RowWriter CSV to io::sink(), 20,000 rows x 8 columns (the db lane's rows: id, name, email, price, flag, a note with quotes and a comma in 4 of 5 rows, timestamp, qty) Optimized: 2.83 ms -> 1.48 ms (faster). Debug, as the lane measured: 20k rows x 8: 244.4 -> 45.7 ms
- <a id="ce3edb3"></a>`ce3edb3`: ConnProfile::location() x 999, cycling a pasted Neon PostgreSQL URL (sslmode, channel_binding, options), a local PostgreSQL profile and a local D1 profile (the lane's profiles verbatim) Optimized: 805 us -> 397 us (faster). Debug, as the lane measured: location() x 999: 7.29 -> 3.20 ms
- <a id="fabed17"></a>`fabed17`: db::diagram::layout on 2000 tables, 400 of them in chains of 4 and the rest alone (the lane's sizes and edges) Optimized: 484 us -> 273 us (faster). Debug, as the lane measured: 2000 tables: 7.33 -> 6.10 ms
- <a id="90103eb"></a>`90103eb`: sqlite::with_read(path, sqlite::load_schema) on a 300-table file, each table with a foreign key, a UNIQUE column and an index (the lane's file) Optimized: 42.3 ms -> 9.39 ms (faster). Debug, as the lane measured: 300 tables: 349.1 -> 57.6 ms
- <a id="bbcac60"></a>`bbcac60`: scripts::read_json on C3's wrangler.jsonc grown by D1 bindings, each with a comment and trailing commas: 5 / 50 / 200 bindings, i.e. 1.3 / 9.7 / 38.0 KB (the commit's 1 / 9 / 38 KB, same generator). Headline: the 9.7 KB file Optimized: 1.01 ms -> 56.6 us (faster). Debug, as the lane measured: 9 KB wrangler.jsonc 42.2 -> 0.95 ms (also 1 KB 1.02 -> 0.14 ms, 38 KB 767 -> 5.0 ms)
- <a id="01e2a6b"></a>`01e2a6b`: scripts::scan on a monorepo: root package.json with workspaces apps/* and packages/*, 25 apps + 15 packages (six scripts each, no lockfile or packageManager of their own), the root's pnpm-lock.yaml, a Makefile and a Cargo.toml (the lane's layout) Optimized: 1.52 ms -> 1.13 ms (faster). Debug, as the lane measured: 40-package monorepo 5.55 -> 4.81 ms (also 5.67 -> 5.15 and 5.06 -> 4.61; wall time, old and new code in one build)
- <a id="ffede7d"></a>`ffede7d`: summary::summarize on a page of 2000 elements added by one `add` op: 400 each of stickies with text, rectangles with a label, labelled arrows joining them, text layers and ellipses (2400 layers including the arrows' labels). The summaries are dropped inside the timing. Optimized: 24.87 ms -> 3.19 ms (faster). Debug, as the lane measured: summarize 103.4/105.4/102.6 -> 34.9/34.8/35.0 ms CPU (subject: 103 -> 35 ms). The body also gives the optimized whole `get` before the change: 22.2 ms.
- <a id="906183b"></a>`906183b`: The `get` op through ops::run on the same 2000-element board (reply dropped inside the timing), with labels found in one pass in both trees. Also: the `command` op running node.get on the page (1,366,102-byte result), and the `commands` op (558 commands). Optimized: 4.62 ms -> 3.03 ms (faster). Debug, as the lane measured: get 50.9/54.8/53.9 -> 25.6/34.7/35.6 ms CPU (subject: 54 -> 35 ms). command node.get reply built with a copy 139.0/116.5/143.7 -> moved in 40.4/34.1/37.5 ms (whole op after: 53.5 ms). commands reply 1.9/2.6/3.3 -> 0.6/0.8/1.1 ms (whole op after: 2.6 ms).
- <a id="c5663c5"></a>`c5663c5`: input::route_default_key on a new board for `g` (bound to nothing), the commit's headline 'agent key'. Also route_default_key `v` (tool.move) and shortcuts::key_bindings() (211 bindings). Optimized: 72.0 us -> 4.71 us (faster). Debug, as the lane measured: route_default_key g 1095 -> 40 us, v 1069 -> 8.8 us; key_bindings() 1065 -> 20 us (debug, per call)
- <a id="895cf2d"></a>`895cf2d`: toolbar::groups(DocKind::Board) per call. Also groups(Design), and toolbar::flyout(Tools("geometry")) on a new board. Optimized: 2.89 us -> 0.74 us (faster). Debug, as the lane measured: groups(Board) 71 -> 4.9 us; groups(Design) 53 -> 3.0 us; flyout(Tools(geometry)) 75 -> 2.4 us (debug, per call)
- <a id="b6c5a38"></a>`b6c5a38`: overlay::overlay of a board of 1000 stickies in rows of 40, all selected, viewed at 25% on a 1600x1000 canvas (the lane's own setup). Also a design file of 300 frames, all selected, and 300 ink strokes of 40 points, all selected. Optimized: 1.322 ms -> 1.079 ms (faster). Debug, as the lane measured: 1000 stickies 4.04 -> 3.51 ms; 300 frames 1.35 -> 1.23 ms; 300 ink 801 -> 664 us (debug, per overlay)
- <a id="51d3a1d"></a>`51d3a1d`: The same overlay scenarios (1000 stickies, 300 frames, 300 ink strokes, all selected). Optimized: 1.079 ms -> 1.031 ms (faster). Debug, as the lane measured: 1000 stickies 3.51 -> 3.06 ms; 300 frames 1.23 -> 1.15 ms; 300 ink 664 -> 612 us (debug, per overlay)
- <a id="641e999"></a>`641e999`: skeleton::apply onto a new board of 799 skeletons: 400 labelled rectangles in a row with strokeColor #1971c2, each joined to the one before by an arrow with strokeColor #e03131. Also 199 skeletons (100 rectangles) and 399 skeletons (200 rectangles). Optimized: 145.96 ms -> 113.45 ms (faster). Debug, as the lane measured: 799 skeletons 178.8 -> 151.4 ms; 399: 48.8 -> 41.5 ms; 199: 14.9 -> 12.9 ms (thread CPU, debug with FigCraft at opt-level 3)
- <a id="ec63b22"></a>`ec63b22`: presence::target (runs on the UI thread after an agent's op): one rectangle added to a page of 6000 layers. The board holds 6000 rectangles (6000 top-level layers, 18,000 document nodes, since a board rectangle is 3 nodes); target(before, after, page) is timed on the documents before and after the add op. Optimized: 11.25 ms -> 275 µs (faster). Debug, as the lane measured: 27.1 ms -> 2.53 ms (add one layer to a page of 6000 layers; also change one of 6000: 34.0 -> 2.72 ms, change 100 of 600: 2.69 -> 0.43 ms, a diagram of 400 layers on a page of 600: 1.05 -> 0.74 ms)
- <a id="552fb95"></a>`552fb95`: Actions palette open in an 800x600 canvas window (the crate's chrome_canvas helper), query typed as keystrokes (cmd-k d u p). BoardView::render_menus is called 50 times inside each of 6 draws per run (300 calls per run); only the calls are timed. Headline query: "dup". Optimized: 6.40 µs -> 2.69 µs (faster). Debug, as the lane measured: 147 µs -> 42 µs (render_menus with the palette open on "dup"; per-round medians 150/144 -> 42/43 µs; query "": 273/265 -> 251/256 µs; query "z": 293/287 -> 196/207 µs)
- <a id="246bd1d"></a>`246bd1d`: Design panel (render_design_panel) rebuilt with nothing changed, as while the canvas pans or an agent's pointer moves. 900x1000 window; a design file with one rectangle holding 4 fills, 2 strokes and 3 drop shadows, all three shadow editors open (38 fields). 50 calls per draw, 6 draws per run. Optimized: 250.1 µs -> 176.9 µs (faster). Debug, as the lane measured: 7.2 ms -> 2.6 ms (rich selection; per-round medians 7.67/6.75 -> 2.46/2.72 ms); plain rectangle 869/918 -> 876/973 µs (no change)
- <a id="3e80a22"></a>`3e80a22`: 800x600 window with the chrome showing, a board with five stickies, all selected. toolbar::render_toolbar is called 50 times in one draw per run. Also timed: BoardView::render (50 calls per draw), and whole frames (notify, then the test app's draw; 50 frames per run) with all five selected and with none selected. Optimized: 42.4 µs -> 45.6 µs (same). Debug, as the lane measured: 922 µs -> 469 µs (toolbar render; view render 1454 -> 987 µs; whole frame, five selected 19.14 -> 18.52 ms; none selected 18.26 -> 18.11 ms)
- <a id="52da994"></a>`52da994`: TerminalPanel::render (the dock: tab strip plus the tab in front) in a 1200x800 window of three projects and nine tabs: one tab split in two panes running Claude Code and Codex (set with the crate's test-only fake_agent), a Run-panel script running sleep, and an SSH session to a stand-in server (/bin/sh -c 'read line'). Every shell waits at its prompt. 50 renders per draw, 10 draws (500 renders) per run. Optimized: 49.3 µs -> 43.0 µs (faster). Debug, as the lane measured: 584 µs -> 496 µs (dock render)
- <a id="dcb5b3c"></a>`dcb5b3c`: Loading a 1,500-table PostgreSQL schema (5,998 columns and constraints, 4,499 indexes, 1,499 foreign keys) from a local PostgreSQL 16 through the function the app uses; the loaded schema hashed the same in every run of both trees. Optimized: 17.24 ms -> 5.52 ms (faster). Debug, as the lane measured: 1500 tables client CPU 222.1 -> 101.7 ms (debug, as the lane measured)

## Debug builds

`22fd986` perf(build): optimize QuickJS, PNG, SQLite and the terminal in debug builds (Mermaid 4.3x, screenshot 14x, SQLite 1.5-2.9x, terminal 1.6x). Release builds are unchanged; the measured paths are in its body.

82 commits; 3e80a22 was later reverted (23447a1), so 81 remain.
