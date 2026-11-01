# Native canvas: hand checks and questions for you

Collected on 2026-10-31 from main `8a2ace4`. The checks come from the lists headed "Hand checks (pending — needs
the user)" in `docs/superpowers/plans/2026-10-29-native-canvas-1-…8-*.md`, read from main. The questions come
from the SDD ledgers in `.superpowers/sdd/2026-10-29-native-canvas-*/progress.md`.

- There are **66 hand checks**, grouped by plan. `P5-2` means Plan 5's hand check 2, so the numbers match the
  plan files, their notes and the ledgers.
- The plans list 73 entries. Seven of them repeat another check or only point at other plans' checks. Each such
  check is kept once, and the duplicate number names where it went.
- There are **26 questions** for you, under "Questions for you" at the end.
- Nobody has run these checks yet. Synthetic input can't reach the window, so the run drove the app over its
  control socket. Everything that needs real hands was left for you (rule R-LIVE).
- Run them before `/consolidate`, because a push to main makes a release. Two of them matter most:
  - P7-2 (which import losses matter): a release migrates each real board once.
  - P8-2 (whether the slow settle is acceptable): this is a release judgement.
- Write each answer on its `Answer:` line. Rule R5 keeps the answers in the plan files, under each plan's own
  list, as `docs:` commits. Copy them there when you are done.

## Before you start

**Build.** Use a checkout of main, after the consolidation merges.
- A release build, `cargo build --release -p sawit`, is needed for the checks that judge smoothness: P1-2, P1-3
  and P8-2.
- A debug build, `cargo build -p sawit`, is enough for the others and builds faster. Below, the commands use
  `target/release/sawit`. With a debug build, use `target/debug/sawit` instead.

**Launch.** This is Plan 1 Task 7's launch recipe. The plans wrote their lists for a debug build launched with
`SAWIT_NATIVE_CANVAS=1`. Since the cutover (`763326c`), main has no such flag and every build shows the native
canvas, so leave the flag out.

```bash
S=<a scratch folder>; mkdir -p "$S/home" "$S/proj"
# Start state B only (the example design file). Skip this line for start state A (an empty board):
mkdir -p "$S/proj/.nyawit" && cp ~/.cargo/git/checkouts/figcraft-bc55fa7002b5fc9c/0971d09/examples/ridgeline-app.figcraft "$S/proj/.nyawit/canvas.figcraft"
SAWIT_HOME=$S/home RUST_LOG=sawit=debug target/release/sawit --foreground "$S/proj" 2>"$S/sawit.log" &
PID=$!
# Once the app is up: $S/home/sawit.sock, or /tmp/sawit-$USER-<hash>.sock when that path is 100 bytes or more.
SOCK=$(lsof -p $PID -a -U | grep -o '/[^ ]*\.sock$')
ctl() { python3 -I -c 'import socket,sys; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); s.sendall(b"sawit-control/1\n"+sys.argv[2].encode()); s.shutdown(socket.SHUT_WR); print(s.makefile().read(), end="")' "$SOCK" "$1"; }
ctl "{\"cmd\":\"open_page\",\"page\":\"canvas\",\"project\":\"$S/proj\"}"
```

- `--foreground` keeps the app in this shell. `$PID` is the window's process, and the log gets the
  `canvas render … ms` lines.
- The scratch `SAWIT_HOME` keeps your own Sawit data, and the Sawit you have running, out of the checks.
- Stop the app with `/bin/kill $PID`, which stops this instance only. `kill` is an alias in your shell, so use
  the full path.
- Use a fresh scratch folder for each plan's list, because the checks edit the board. To relaunch on the same
  board, run the same lines again without the copy line.

**Start states.**
- A: an empty board, from the recipe without the copy line. The first edit creates `.nyawit/`.
- B: the example design file, from the recipe with the copy line.
- In both, after `open_page`, click the canvas once so that it has the keyboard.

## Plan 1: foundation (opening, saving, panning, zooming): 5 checks

Start: B, with the recipe's `open_page` line run.

**P1-1. Autosave while the window is covered or minimised.** This comes from Task 6's review: an op sent while
the window's display link is stopped.
- Minimise the Sawit window (⌘M), or cover it completely with another app's window.
- Run `ctl '{"cmd":"canvas","op":{"op":"command","command":"text.create","params":{"x":0,"y":0,"text":"covered-save"}}}'`.
  Expect `ok:true`.
- Wait 2 s with the window still minimised or covered. Then run
  `unzip -p "$S/proj/.nyawit/canvas.figcraft" document.json | grep -c covered-save`. Expect 1 or more.
- Bring the window back. Expect no crash and no blank canvas: the canvas redraws at once.
- Run `ctl '{"cmd":"canvas","op":{"op":"command","command":"view.zoomSelection"}}'`. Expect the new text to
  show.

Answer: ______

**P1-2. Panning with the trackpad.** Use a release build. A debug render takes 0.1–0.2 s, so a debug build
can't show whether panning is smooth. Scroll with two fingers over the canvas.
- The content follows the fingers and moves at once. The last frame moves with them until the new render
  arrives.
- A fast pan of more than about 256 pt shows the page colour at the leading edge for a moment, then the content
  fills in.
- Nothing jumps or flickers when a new frame replaces the moved one.

Answer: ______

**P1-3. Zooming with gestures.** Pinch, and scroll with two fingers while holding ⌘.
- Both zoom about the pointer: the spot under the pointer stays under it.
- During the gesture the image is a little soft, because it renders at half resolution. About 0.15 s after the
  fingers stop, it is sharp again.
- At rest, text and 1 px lines are crisp, with no half-pixel blur.

Answer: ______

**P1-4. Opening the canvas by hand.** This is also the first part of Plan 8's hand check 1. Each way in must
bring the open canvas tab forward, with no second canvas tab. Put the tab in the background before each way in:
otherwise a way in that brings nothing forward would still pass.
- Run `ctl '{"cmd":"open_page","page":"changelog"}'`. The Changelog tab comes forward, with the canvas tab behind
  it. Click the Sawit window's title bar, and press ⌃⌘C. Expect the canvas tab to come forward.
- Run the same `open_page changelog` line again. Then click the title bar's Canvas button. Expect the canvas tab
  to come forward again.
- Run `ctl '{"cmd":"state"}'`. Expect exactly one tab of kind `canvas`.

Answer: ______

**P1-5. Quitting before an edit is on disk.** This is also Plan 2's hand check 13. The quit hook writes what
isn't saved yet. This check runs it through a failed save, which leaves an edit unsaved until the next edit or
flush (closing the tab, quitting): the autosave doesn't try again by itself.
- Run `chmod 555 "$S/proj/.nyawit"`, then
  `ctl '{"cmd":"canvas","op":{"op":"command","command":"text.create","params":{"x":0,"y":0,"text":"quit-save"}}}'`.
  Expect `ok:true`, and then the toast "The canvas couldn't be saved to …/canvas.figcraft: Permission denied
  (os error 13)".
- Run `chmod 755 "$S/proj/.nyawit"`, then
  `unzip -p "$S/proj/.nyawit/canvas.figcraft" document.json | grep -c quit-save`. Expect 0: nothing has saved it
  since.
- Click the Sawit window's title bar, and press ⌘Q. Don't click the canvas: a drag there is an edit, which
  saves. Expect the app to quit, and `$PID` to end.
- Run the `unzip … | grep -c quit-save` line again. Expect 1 or more: the quit hook saved it.
- By hand (Plan 2's hand check 13): relaunch without the copy line and open the canvas. Move a layer with the
  mouse, and press ⌘Q at once. Relaunch and open the canvas again. Expect the move to be kept.
- Closing the tab uses the same save. The run checked that live.

Answer: ______

## Plan 2: editing: 13 checks (its hand check 13 is the last step of P1-5)

Start: A. Click the canvas once, so that it has the keyboard. Then make three rectangles in a row, a little
apart: press R before each one, and drag it out. After each rectangle the tool goes back to Move, so a second
drag without R would draw a selection box instead.
- P2-3, P2-4 and P2-5 also need a sticky with text. To make one, press Q, click, type a word, press Escape,
  then press V.
- Where a check already passed over the socket, it says so.

**P2-1. Drag a shape** (Step 4, and Task 3's review).
- Press V. Drag a rectangle by its middle, and let go. It follows the pointer all the way, then stays there,
  selected: a blue outline, 4 corner handles and its size under it.
- Drag it level with another rectangle. While the button is held, red-orange guides join the edges that line
  up, and the rectangle snaps into line.
- Drag an end rectangle until the gaps on both sides of the middle one are equal. Each gap shows a red-orange
  line with a pill that gives its width in px.
- The guides go when the button comes up.
- Over the socket, the snap, the guide and the `26` pills showed as described.

Answer: ______

**P2-2. A drag released outside the window** (Review Focus 1, and Task 2's review).
- Press R. Press on the canvas, drag out past the window's edge, and release outside the window.
- Expect one rectangle, selected, with the Move tool back. Moving back over the canvas draws nothing.
- One ⌘Z removes the whole rectangle.

Answer: ______

**P2-3. ⌘Z** (Step 4).
- Right after a drag, ⌘Z puts the rectangle back where it was, in one step. ⇧⌘Z moves it again.
- Double-click a sticky that has text, and type x: the x replaces the text. Press ⌘Z while still typing. Expect
  the edit to end, and the sticky to read its old text again, in one step.
- Both passed through the key op.

Answer: ______

**P2-4. ⌘B while editing versus not** (Step 4).
- Click empty canvas, so that no text is edited, and press ⌘B. Sawit's left sidebar hides, and the canvas
  doesn't change. ⌘B again shows the sidebar.
- Double-click a sticky, which selects all of its text, and press ⌘B. The sidebar stays where it is, and the
  text turns bold.
- The plan's workaround (a text in Helvetica Neue, because this Mac has no Inter) is gone: Sawit embeds Inter
  since the cutover. P8-3 checks Inter's bold and weights.

Answer: ______

**P2-5. ⌘W closes the tab** (Step 4).
- Click the canvas, and press ⌘W. The Canvas tab closes.
- Reopen it with ⌃⌘C, or the title bar's Canvas button. The board is as it was.
- Double-click a sticky, type a word, and press ⌘W while still typing. The tab closes here too: ⌘W is Sawit's
  even while text is edited. Reopen it. The sticky reads what was typed.

Answer: ______

**P2-6. Cursors** (Task 2's review; ruling P2-C3 can only be seen by eye).
- R: a crosshair.
- T: a crosshair over empty canvas, and an I-beam over a text layer. To make a text layer: T, a click, a word,
  then Escape.
- V, with a rectangle selected:
  - ↔ on its left and right edges, and ↕ on its top and bottom edges;
  - the ↖↘ diagonal on its top-left and bottom-right corners, and the ↗↙ diagonal on its top-right and
    bottom-left corners;
  - a crosshair just outside a corner (rotate).
- H: an open hand, and a closed hand while the button is held.

Answer: ______

**P2-7. Double-click into a sticky** (Task 2's review).
- Press Q, and click to place a sticky. Type a, press Escape, then V.
- Double-click the sticky. Its text shows selected, and typing x replaces it. Without the edit, x would pick the
  Connector tool.
- Over the socket, the double-click selected all of the text, and x replaced it.
- Triple click (the final review): place a new sticky (Q, a click), type `one`, press Return, type `two`, and
  press Escape. Click three times quickly on `two`. The sticky's text is being edited, and `two` alone shows
  selected, not a caret. Typing y replaces just that line.

Answer: ______

**P2-8. Panning by hand** (the reviews of Tasks 2 and 4).
- Hold Space over the canvas: an open hand. Drag: the canvas follows the pointer, under a closed hand. Release
  Space: the tool's cursor is back.
- With a mouse, press R, then drag with the middle button. The canvas pans, and nothing is drawn.
- Drag with the middle button again, on past the window's edge, and release outside the window. The pan ends
  there: back over the canvas, the crosshair shows, not the closed hand.
- While typing in a sticky, Space types a space.

Answer: ______

**P2-9. Shifted digits on a real keyboard** (Task 1's review; a US layout).
- Select a shape, and zoom out. ⇧2 zooms to the selection, ⇧1 zooms to fit, and ⇧0 sets 100 %.
- While typing in a sticky, ⇧⌘8 makes the paragraph a bulleted list, and ⇧⌘8 again ends it. ⇧⌘7 makes a
  numbered list.

Answer: ______

**P2-10. Typing with a Chinese input method** (Step 4, Task 5's review, and Review Focus 3).
- Set-up: add Pinyin – Simplified (System Settings › Keyboard › Text Input › Edit › +). Press Q, and click to
  place a sticky. You are now typing in it.
- (1) With Pinyin, type ni.
  - ni shows underlined in the sticky. The candidate window opens just below it, from its left end, and not at a
    corner of the window or of the screen. Check this again after a pinch zoom-in.
  - Press Space (or 1). The first candidate (你) replaces the underlined ni, and no space is typed.
  - Type ni, then press Return. Pinyin commits the letters ni, and no new line starts.
- (2) Type ni, then press Escape. The letters vanish, and nothing goes in. The caret stays, and typing composes
  again. A second Escape ends the edit.
- (3) Click away while composing:
  - Type ni, and click empty canvas. The edit ends with no underline, caret or candidate window left.
  - Write down what the sticky then reads: ni, 你, or nothing added. Each passes Review Focus 3. For comparison,
    write down what TextEdit keeps on the same click. Q16 asks which behaviour you want.
  - Double-click the sticky, and type a. The a composes alone, never nia.
- (4) An edit that ends under the input method:
  - In another app's terminal (not Sawit's own), with the recipe's `ctl` and `SOCK`, run
    `sleep 5; ctl '{"cmd":"canvas","op":{"op":"key","key":"Escape"}}'`.
  - Switch back with ⌘Tab (a click would end the edit), and type ni in the sticky within the 5 s. The edit ends,
    and the underline goes.
  - Double-click the sticky, and type a. Write down whether the a composes alone, or nia shows.
- (5) Switch to ABC or U.S., with press-and-hold on (the macOS default). Type caf, then hold e. The accents menu
  opens by the e. Choose é: the sticky reads café.
- (6) ⌥E: ´ shows underlined at the caret. Then E: é, with no ´ left.
- (7) With Pinyin on and nothing composed, Space types a space. (A plain Space in a sticky is P2-8's last step.)
- (8) The caret holds steady, where FigCraft's blinks (Task 5, Decision 6). Say whether that is fine. Live, it
  cost 0–1.8 % CPU at rest. Since `84053c2` it is a 2 px blue bar, so that it shows at the start of a line too.
  Check that it shows in a new sticky, and after ⌘← (the start of the line).
- (9) While ni is composing, pan with two fingers. The candidate window stays where it was until the next key
  (Task 5's review, a deferred minor). Say whether that bothers you.

Answer: ______

**P2-11. Pasting while composing** (Task 6's review).
- Copy hello in TextEdit. In a sticky being edited, with Pinyin, type ni: it is underlined, and the candidates
  are open. Then press ⌘V.
- hello is in the sticky. No underlined ni stays once nothing is being composed. Typing on composes and commits
  as usual.
- Write down where the composition's own text went, if anywhere.

Answer: ______

**P2-12. The system clipboard** (Task 6's review). The socket can't reach the clipboard: the key op answers
`handled: true` for ⌘C without copying (ruling C7).
- (a) Select a sticky, ⌘C, then ⌘V in TextEdit. It types `sawit-canvas:N`, where N counts the copies since
  launch. Q17 asks whether to keep this marker.
- (b) Copy a word in TextEdit, click empty canvas, and press ⌘V. A text layer reads that word, its top-left in
  the middle of the view, and no toast shows.
- (c) Press ⌃⇧⌘4, and drag over part of the screen (the capture goes to the clipboard). Then ⌘V on the canvas:
  an image layer shows it, centred in the view (the final review), at the size it had on screen.
- (d) In a text editor, copy the source of an SVG file that starts with `<?xml`. Then ⌘V on the canvas: its
  shapes come in, in a frame centred in the view, not as a text layer.
- (e) Between two project canvases. This is also Plan 8's hand check 8, whose steps these are.
  - Make the second folder first: `mkdir -p "$S/proj2"`. Sawit opens a path that doesn't exist as a file, in a
    window with no project.
  - Open it in a window of its own: `SAWIT_HOME=$S/home target/release/sawit -n "$S/proj2"`, and open its
    Canvas page (⌃⌘C). Its canvas is `$S/proj2/.nyawit/canvas.figcraft`, and the first window's is
    `$S/proj/.nyawit/canvas.figcraft`.
  - In the first window's canvas, select a sticky and press ⌘C. Click the second window's canvas and press ⌘V:
    the sticky pastes there, and ⇧⌘V pastes it again.
  - A second canvas tab, from a `.figcraft` file opened in the Files tab, can be the other canvas too.
- (f) While typing in a sticky, select a word and press ⌘C. Then ⌘V in TextEdit types that word, not a marker.
- (g) Copy text in TextEdit, then press ⌘V in a sticky being edited. The text is typed at the caret, and no
  layer appears.
- (h) ⌘C a sticky, quit Sawit with ⌘Q, and relaunch it. ⌘V on the canvas pastes nothing: no toast, and no
  `sawit-canvas:N` text layer (Decision 15).

Answer: ______

**P2-13.** Moved to P1-5's last step, which has the same steps.

**P2-14. Corner radius and arc handles** (the final review's Important 2).
- Press Q, click to place a sticky, and press Escape: the sticky stays selected. Just inside each corner, a
  little in from the square handle, a small round handle shows.
- Hover one: it grows a little. Drag it toward the middle: all four corners round as you drag, and a blue pill
  beside the handle reads `Radius` and the value. Let go. ⌘Z puts the corners back.
- Drag the square corner handle itself: the sticky resizes, and its corners stay as they were.
- Make an ellipse:
  `ctl '{"cmd":"canvas","op":{"op":"command","command":"node.create","params":{"type":"ellipse","x":200,"y":-100,"width":240,"height":160}}}'`.
  Click it: one round handle shows just inside its right edge. Drag it round the ellipse: the ellipse opens into
  a pie, and a pill reads `Sweep` with a share in %. A second round handle, with a dot inside, marks where the
  arc starts. ⌘Z closes the ellipse again.
- Say whether the pills and the handles read clearly. Over the socket, they showed as the plan's "Final review
  fixes" describe.

Answer: ______

## Plan 3: toolbar, top bar and context bar: 7 checks

Start: A, with the recipe's `open_page` line run. Click the canvas once, so that it has the keyboard.
- P3-5 and P3-6 need a sticky, a shape and a connector. Place them with the toolbar.
- Where a check already passed over the socket, it says so.

**P3-1. Tools and flyouts** (Step 4, and Task 3's review).
- Click each tool button. It shows marked (a lighter square), and its tool is on: after Sticky note, a click on
  the canvas places a sticky. Hover a button: its tooltip reads its name and key ("Sticky note  Q").
- Press Q, B, X and V: the marked button follows at once. This passed over the socket.
- Click the chevron beside Sticky note. The flyout opens above the toolbar, centred on the chevron: 8 swatches,
  the current colour marked. Pick blue: the flyout closes, the Sticky note tool is on, and the next sticky is
  blue.
- The chevron beside Shapes lists the 7 shapes, the current one checked. Pick Ellipse: the Shapes tool now
  places ellipses. The Connector chevron lists Straight, Elbow and Curved, and the Stamp chevron the 6 stamps.
- If a sticky's text has a caret, press Escape first: ⌘\ does nothing while text is being edited. Open a flyout
  and press ⌘\: nothing happens, because the open flyout has the keyboard. The canvas takes only the keys
  pressed while it has the keyboard itself (ruling P5-C3). Press Escape: the flyout closes, and the canvas has
  the keyboard again. Then press ⌘\ twice: the chrome goes, then comes back with no flyout open.
- Say whether the marked look of a tool button is clear enough.

Answer: ______

**P3-2. Placing a template** (Step 4, and Review Focus 5).
- Click the last button, Templates & table. The picker opens above the toolbar: Retrospective, Brainstorm,
  Weekly planner, Flow chart and Kanban board, each with a line of description, then "Table" and a 6 × 6 grid.
- Zoom far out (the zoom menu's 50 %, then ⌘− a few times), open the picker, and click Flow chart. It lands in
  the middle of the view, selected, and the picker closes. One ⌘Z removes all of it. Over the socket, the same
  command at 5 % did this.
- Open the picker, and leave it open for 10 s. Activity Monitor shows Sawit at about 0 % CPU: the list isn't
  asked for on every frame (ruling P3-C14).

Answer: ______

**P3-3. A 3 × 3 table** (Step 4).
- Open the picker, and move the pointer over the grid. The cells from the top left to the one under the pointer
  light up, and over the third cell of the third row the heading reads "Table  3 × 3".
- Click it. A 3 × 3 table lands in the middle of the view, selected, and the picker closes. ⌘Z removes it. Over
  the socket, `board.table` placed the same table.

Answer: ______

**P3-4. The zoom menu** (Step 4).
- Click "100%" in the top bar. The menu lists Zoom in and Zoom out, then Zoom to fit and Zoom to selection, then
  50 %, 100 % and 200 % (the engine's labels), each with its keys on the right.
- Each item does what it says, and the button then shows the new zoom. Over the socket, the same commands
  changed the label.

Answer: ______

**P3-5. The context bar** (Task 3's review, and Review Focus 2).
- Select a sticky, and click the blue swatch. The sticky turns blue, and that swatch is marked.
- Select a shape, and open its shape menu: the 7 shapes, the current one checked. Pick the checked one: nothing
  happens, and no toast shows (ruling P3-C2). Pick Diamond: the shape becomes a diamond. The size menu offers
  S, M, L and XL.
- Select a connector. Its style and cap menus change it.
- Each menu opens by its button, and the bar's buttons look right when hovered and when marked.

Answer: ______

**P3-6. A narrow canvas** (Task 3's review and Plan 3's final review). The toolbar scrolls, as FigCraft's does,
and so does the context bar.
- With Sawit's sidebar shown, drag the window's edge in until it stops (720 pt wide). The canvas is then about
  480 pt wide.
- Expect the board toolbar 8 pt in from both sides of the canvas, with its buttons at their usual size and the
  Move button whole at its left end. The Templates & table button is out of sight past its right end.
- Swipe sideways over the toolbar with two fingers, or turn a mouse wheel over it. The buttons slide inside it
  until the Templates & table button shows whole, and the canvas under it doesn't move. Click that button: the
  picker opens above it. Swipe back: the Move button shows again.
- Select a connector (draw one with the Connector tool if the board has none). Its context bar is 8 pt in from
  both sides of the canvas, with its colours whole at its left end and its End cap menu out of sight past its
  right end. Swipe sideways over the bar, or turn a mouse wheel over it: its controls slide until the End cap
  menu shows whole, and the canvas under it doesn't move. Open that menu and pick Circle: the connector's end
  becomes a filled circle.
- Widen the window. From a canvas of about 524 pt, the toolbar has its full width again, every button in sight.
  From about 667 pt, so does the connector's context bar, every control in sight.
- Say whether the scrolling toolbar and context bar work well enough, or whether they should wrap or shrink
  instead.

Answer: ______

**P3-7. Hide UI and Maximize** (Task 3).
- Hover the eye-off button at the top bar's right: "Hide UI  ⌘\". Click it: all the chrome goes. ⌘\ brings it
  back. The button only hides (ruling P3-C12).
- Hover the Maximize button: "Maximize  ⇧⌘↩". Click it: Sawit's sidebar and terminal go, and the canvas takes
  the window. Click it again: they come back. P8-1 checks the tab bar with ⇧⌘↩.

Answer: ______

## Plan 4: layers panel: 11 checks

Start: B, with a fresh copy of the example, because these checks edit the file. Run the recipe's `open_page`
line.
- A design file opens with the layers panel on the left and the Design panel on the right. Click the canvas
  once, so that it has the keyboard.
- What the file holds:
  - "Home — Light" holds the frames "Tab bar", "Content" and "Status bar".
  - "Content" holds instances ("Trail row", "Featured") and frames.
  - "Trail — Light" holds "Home indicator" (a rectangle), "Hero" (an instance) and others.
  - "Components" is a section of components ("Trail row", "Trail card", "Button", "Chip", "Landscape").
- Where a check already passed over the socket, it says so.

**P4-1. Drag to reorder** (Step 4).
- Open "Home — Light" with its arrow. Press on "Status bar" and drag a few pixels: a small label with its name
  follows the pointer.
- Move over the top half of the "Tab bar" row: a 2 px line shows in the gap above it, from the row's indent to
  the panel's right edge. Over its bottom half the line moves below it. Over the gap where the layer already
  is, no line shows: a drop that moves nothing shows no mark.
- Drop above "Tab bar": "Status bar" is now the first of the three, and the canvas shows the new stacking. One
  ⌘Z puts the order back, and ⇧⌘Z does it again.
- ⌘-click two rows and drag one of them: the label reads "2 layers", both move, and one ⌘Z puts both back.

Answer: ______

**P4-2. Drag into a frame** (Step 4).
- Drag "Status bar" onto the middle half of the "Content" row. A frame (not a line) goes round the row. Drop:
  "Status bar" is now one of Content's children, and one ⌘Z puts it back.
- Over the middle of a row that takes no layers, no frame shows, only the lines above and below it. Try an
  instance such as "Featured", or the rectangle "Home indicator".

Answer: ______

**P4-3. Drops that must be refused** (Review Focus 1).
- Drag "Home — Light" over its own child "Content" and over the rows under it. No mark shows on the dragged row
  or under it, and a release there changes nothing: the order, the selection and the undo history stay the same
  (⌘Z undoes the step before the drag).
- Drag the section "Components" over the middle of "Home — Dark": no frame shows, since sections go only on the
  page or into a section.
- Open the instance "Hero" in Trail — Light, and drag one of its children onto the middle of "Home — Dark". The
  frame shows, because the panel leaves what lies inside an instance to the engine. Release: a toast says why
  the engine refuses, and the children, the selection and the undo history are unchanged.

Answer: ______

**P4-4. Rename** (Step 4, and Review Focus 2). The field's start passed over the socket. Typing and keys need
you.
- Double-click "Home — Dark" in the panel: a field replaces the name, with the whole name selected. Type
  `Dark home` and press Enter: the row, the label on the canvas and the Design panel's header read
  "Dark home". One ⌘Z puts "Home — Dark" back.
- Double-click it again and press Escape: the old name stays, and ⌘Z undoes the step before. Double-click,
  clear the text and press Enter: the old name stays and no undo step is added. The same holds for a name of
  only spaces.
- Double-click, type a few letters and click another row or the canvas: the typed name is kept, because the
  field keeps what it holds when it loses the keyboard.
- Click a layer on the canvas (it is selected and the canvas has the keyboard) and press F2, or ⌘R: the field
  opens in that layer's row. Double-click a frame's name on the canvas, the label above the frame: the same.
  Both started over the socket, with ops and not the real keys or mouse. With a long list, scroll that layer's
  row out of view first: the list scrolls back to it (Task 2's review found no test of this on a list that
  scrolls).
- Press ⌘\ to hide the chrome, then F2 on a selected layer: nothing shows. Press ⌘\ again. Task 2's review
  expects the field to appear then, with its name selected and holding the keyboard, so that a letter typed
  replaces the name (a deferred minor). Say whether that matters.

Answer: ______

**P4-5. The filter** (Step 4, and Review Focus 3).
- Click "Find layers" and type `card`. The tree lists the layers whose names hold "card", under the layers that
  hold them, open (at least Components, then Trail card), and hides the rest. The arrows do nothing while it
  filters.
- With the field still holding the keyboard, type `v`, `r` and `t`, the keys of the Move, Rectangle and Text
  tools. They appear in the field and the tool doesn't change. Then click the canvas and press `r`: the
  Rectangle tool comes on, so the same key acts there.
- Type `zzzz`: the tree says "No matching layers" and shows no rows, never the old tree.
- Clear the field (the ✕ in it, or delete the text): the tree is as it was before the filter. The rows you had
  opened are open, the ones you had closed are closed, and nothing that the filter showed stays open.

Answer: ______

**P4-6. Hover highlight** (Step 4, and the P2.T3 ruling).
- With the Move tool (V), hover the row of a layer that isn't selected. The canvas outlines that layer 1.5 px in
  blue (purple in a component), and the row takes a light tint. A selected layer gets no extra outline.
- Move the pointer off the rows, onto the canvas: the outline goes (ruling P4-C12).
- With another tool (say Rectangle) no outline shows, because the engine sets the hover only for Move and
  Scale. This is known.
- Rest the pointer on a row and press ⌥1: the panel hides. Task 2's review expects the layer's outline on the
  canvas to stay until the pointer moves, and the row to be drawn hovered when the panel comes back (a deferred
  minor). Say whether that matters.

Answer: ______

**P4-7. Pages** (Step 4, and Review Focus 4).
- Click "+" in the Pages header: "Page 2" is added and opens, and the tree says "No layers on this page". Click
  "App": it opens again, as you left it. Click the "Pages" title: the list folds, and opens again with another
  click.
- Double-click a page's name: a field opens with the name selected. Enter keeps what you typed, Escape drops it,
  and a blank name keeps the old one.
- Right-click a page: Rename page, Duplicate page and Delete page. Duplicate makes "Copy of …" right after it,
  with the same layers, and opens it. Delete the open page: the first page opens. Delete the only page: a toast
  says a file needs at least one page, and the page stays.
- Rename an empty page to `-`: it becomes a divider line, which a click doesn't open. The line passed over the
  socket.

Answer: ______

**P4-8. Selecting** (Plan 4's goal).
- Click a row: that layer is selected on the canvas and in the Design panel. Press `r`: the Rectangle tool
  comes on, since the canvas has the keyboard and not the panel.
- ⇧-click another row: the rows between the two are selected. ⌘-click a row: it joins the selection, or leaves
  it when it was in.

Answer: ______

**P4-9. Eye and lock** (the spec's "eye and lock appear on hover or when off"). The icons and their effect
passed over the socket. The mouse needs you.
- Hover a row: a padlock and an eye appear at its right end, with the tooltips "Lock" and "Hide". Click the
  eye: the layer hides, its name goes grey, and the eye-off icon stays when the pointer leaves. Click it again:
  the layer shows.
- Click the padlock: the layer locks, a click on it on the canvas selects nothing, and the padlock stays after
  the pointer leaves. Click it again: unlocked.

Answer: ______

**P4-10. Scrolling** (ruling P4-C14, and the spec's virtualized list).
- Press ⌘A and then Enter (select all, then select children): the 4 frames open, with 20 rows. With the
  terminal showing, the list is longer than the panel. With it hidden, about 20 rows fit, so open a few more
  rows with their arrows until the list scrolls.
- A two-finger scroll over the list scrolls the list and never pans the canvas. A two-finger scroll over the
  canvas pans the canvas, and the list stays where it is. A pinch over the list doesn't zoom the canvas. The
  list scrolls smoothly.

Answer: ______

**P4-11. The keys of the panel** (rulings P4-C8 to P4-C10: the socket ran the commands, not the keys).
- ⌥1 shows and hides the layers panel (on a board too, where it starts hidden). ⇧⌘\ does the same.
- ⌥L closes every row but the ones that lead to the selection.
- ⌘\ hides all the chrome, the panel with it, and brings it back.

Answer: ______

## Plan 5: Design panel: 13 checks

Start: B, with a fresh copy of the example, and the recipe's `open_page` line run. A design file opens with the
Design panel shown. Click the canvas once, so that it has the keyboard.

Two lines make the layers the checks use, below the three phone frames. Each layer is selected when it is made,
and ⇧2 zooms to the selection:

```bash
ctl '{"cmd":"canvas","op":{"op":"command","command":"node.create","params":{"type":"rectangle","x":500,"y":900,"width":220,"height":140,"name":"P5T6 probe","props":{"fills":["#3B82F6",{"type":"linearGradient","stops":["#FF7A59","#6C5CE7"]}],"strokes":["#111827"],"strokeWeight":2,"strokeAlign":"center","dashes":[4,4],"effects":[{"type":"dropShadow","color":{"r":0,"g":0,"b":0,"a":0.4},"offset":[2,4],"radius":8,"spread":0}],"exports":[{"format":"png","size":{"by":"scale","value":2}},{"format":"svg","size":{"by":"scale","value":1}}],"cornerRadius":12}}}}'
ctl '{"cmd":"canvas","op":{"op":"command","command":"node.create","params":{"type":"text","text":"Typography probe","x":500,"y":1100,"name":"P5T6 text","props":{"fontFamily":"Helvetica Neue","fontStyle":"Medium","fontSize":24,"lineHeight":"150%","letterSpacing":"2%"}}}}'
```

- The Trail card is the card at the lower left of the Components section. Zoom in with ⌘+ to hit it.
- Where a check already passed over the socket, it says so.
- Several checks press R to see whether the canvas has the keyboard: the Rectangle tool comes on only if it
  does. R can show that only from another tool, so each such step starts from the Move tool (V), which the
  toolbar marks.
- If R leaves the Move tool on, that step failed. Note it, click the canvas on the selected layer (with the Move
  tool a click selects, it doesn't draw), and go on.
- After each R that worked, press V before anything else, so that the next click on the canvas selects instead
  of drawing.

**P5-1. Scrub X** (Step 3, the P5.T4 ruling, and Review Focus 3).
- Select the probe rectangle. Drag the "X" label in the Position row sideways. Over the label the pointer is a
  left-right arrow. X moves 1 per px, and 10 per px with ⇧ held, and the rectangle follows on the canvas as you
  drag.
- Let go with the pointer over the canvas, and again outside the window: the drag ends where it is, either way.
- One ⌘Z puts X back where it was before the drag, because the whole drag was one step. ⇧⌘Z does it again.
- Drag again, and press Escape before letting go: X goes back to its start value and the drag is over. ⌘Z now
  undoes the step before this drag, not this one.
- Drag the opacity field's label: opacity moves 1 % per px.
- Select two layers whose X differ and drag the X label. The first pixel sets both to one X, counted from the
  first layer's, where Figma moves each layer from its own X. This is Task 4's deferred minor: say whether it is
  acceptable (Q8).

Answer: ______

**P5-2. Type W** (Step 3, the P5.T4 ruling, Review Focus 2, and the final review's I1).
Start: the probe rectangle selected, the Move tool on (click the canvas on the probe, press V).
- Click in W: its text shows selected. Type 40 and press Enter: the rectangle is 40 wide, and the field reads
  40. Click the rectangle on the canvas and press R: the Rectangle tool comes on, so the canvas has the keyboard
  again. Press V: the Move tool is back.
- Type 60 in W and click in another field, with no Enter: 60 is applied when W lets go of the keyboard.
- (I1) Type 80 in W and, with no Enter, click the rectangle on the canvas (press and let go without moving): the
  rectangle is 80 wide, and W reads 80. Then type 90 in W and, with no Enter, press on the rectangle and drag
  it: it moves, and stays 90 wide. ⌘Z undoes the move, a second ⌘Z puts W back to 80, and a third back to 60.
- Type `abc`, `-5`, `1e400` and nothing at all, each followed by Enter. Each time the field shows the old value
  again, the rectangle doesn't change, and no toast shows.
- Turn on the proportions lock (the link button beside H): typing 100 in W changes H in step.
- Type 50, with no %, in the opacity field. It reads 50 %, and the rectangle fades.
- With the Move tool on, click in X and press R: an "r" appears in the field, and the toolbar still marks the
  Move tool. Press Enter: X shows its value again.
- On its own, from the Move tool: turn the proportions lock off again, click in W, type 40 and press Enter, so
  W is 40. Then type 5, with no click in between: the field reads 405, because after Enter the field keeps the
  keyboard with the caret at its end. Press Enter (the rectangle is 405 wide), click the rectangle on the
  canvas, and press ⌘Z: W is 40 again. The same holds for a canvas key typed after Enter: a digit (opacity) or
  Backspace edits the field, and the next press on the canvas applies it. This is Task 4's deferred minor too:
  say whether Enter should select the text again, as a click into the field does, or give the keyboard back to
  the canvas (Q7).
- Added from the Plan 5 final review: with the caret in W, press Tab. The code reads as if nothing happens,
  because the spec gives Tab to the canvas, where it is FigCraft's select-next. Nobody has tried it. Say whether
  Tab and ⇧Tab should move between the fields (Q10).

Answer: ______

**P5-3. Mixed selections** (Review Focus 1, and ruling P5-C2).
- Select two layers whose X differ (click one, ⇧-click the other). X reads "Mixed". Type 100 in it and press
  Enter: both layers go to X 100, and one ⌘Z puts both back.
- Select the text and the rectangle together. Typography shows, with the text's values. Set the size to 18:
  only the text changes, and no toast shows.
- Select two layers with different fills. Fill reads "Mixed" with "Replace all". Click it: both get one new grey
  fill, and one ⌘Z restores both.

Answer: ______

**P5-4. Menus leave the canvas its keys** (the P5.T4 ruling on blend, and the P5.T5 one on the other menus).
Start: the probe rectangle selected, the Move tool on (click the canvas on the probe, press V). Each step below
begins from the Move tool and ends with V.
- Open the blend menu in Appearance: FigCraft's modes in groups, with the current one, "Normal", checked. Pick
  Multiply, then press R without clicking the canvas: the Rectangle tool comes on. Press V.
- Open the blend menu again and press Escape, then press R: the Rectangle tool comes on. Press V.
- Pick a stroke position (Inside, Center or Outside), then press R: the same. Press V.
- Pick an export format, then press R: the same. Press V.
- Click the canvas on the Trail card (with the Move tool), and do the same with its W and H menus (Fixed and
  Hug): pick one, press R, then V, for each menu.

Answer: ______

**P5-5. The colour picker** (Step 3, and the P5.T5 ruling).
- (a) Draw a rectangle (R, then drag): its fill is `D9D9D9`. Click the fill's swatch in the Design panel. The
  picker opens under the swatch, about 234 px wide, whole and inside the window. Drag its sliders: the canvas
  follows. Close it by clicking the swatch again, and press ⌘Z once: the fill has the colour it had before the
  pick. A second ⌘Z undoes the step before the pick, which is drawing the rectangle.
- (b) Open the swatch again, drag a slider, and press Escape: the picker closes and the colour goes back. ⌘Z
  then undoes no colour.
- (c) Open the picker, pick a colour, then press on the selected layer on the canvas and drag it: the picker
  closes, the layer moves, and no toast shows. ⌘Z twice undoes the move, then the colour. A click on empty
  canvas while a picker is open closes it, with no toast.
- Type `ff0000` in the hex field next to the swatch and press Enter: the fill is red. Type `zzz`: the field
  shows the colour again.
- (Review Focus 4) Press V, and click the probe on the canvas. Its Fill rows are "Linear gradient" on top, then
  the solid `3B82F6`. Type `22C55E` in the solid's hex field and press Enter. The gradient row still reads
  "Linear gradient", and the canvas still shows the orange-to-purple gradient over the rectangle. Hide the
  gradient with its eye: the rectangle under it is green. Show it again: the gradient is as it was. Each is one
  ⌘Z step.
- (C1, a text being edited) With the Move tool, double-click "P5T6 text" on the canvas, press ⌘→ to put the caret
  at the end of the line, and type " one". Click the Fill swatch in the Design panel: the caret goes, and the
  text keeps " one". Drag a slider, then press Escape: the colour goes back, and the text still ends in " one".
  Click the text on the canvas and press ⌘Z: " one" goes, as one step.
- (C1, the same with a label) Double-click the text again, press ⌘→, and type " two". Drag the X label, and press
  Escape before letting go: X goes back, the text still ends in " two", and ⌘Z (after a click on the text) takes
  back " two" alone.
- (C1, a pen path) Press P, and click two points on empty canvas: a path is being drawn. Click its stroke swatch
  and pick a colour: the path takes it, and keeps its two points. Press Escape: the picker closes, and the path
  and its colour stay. Without clicking the canvas (a click would add a point), press Enter: the path is done,
  and one ⌘Z takes it away, colour and all. Press V. (Q22 asks whether the path should end first.)

Answer: ______

**P5-6. A drop shadow** (Step 3, and the P5.T5 ruling).
- With the rectangle selected, click "+" in Effects. A "Drop shadow" row appears, and the canvas shows the
  shadow.
- Click the sliders button at the left of the row: x, y, blur, spread and the colour row show under it. Type 24
  in blur: the shadow softens. Type -3: the field shows 24 again, with no toast.
- Open the kind menu: Inner shadow, Layer blur (which has only a radius) and Background blur.
- The eye hides the effect, and "−" removes it. Each one is a ⌘Z step.
- Select the Trail card: two "Drop shadow" rows, the top one first. Change one's blur: the other doesn't change.
- Open the editor of one of them, then remove the other. The editor should stay with the effect it was opened
  for. Task 5's deferred minor: editors are kept by their place in the list, so the editor may jump to the
  effect that moves into that place.

Answer: ______

**P5-7. A font change** (Step 3, the P5.T5 ruling, and Review Focus 5).
Start: the Move tool on (click the canvas on "P5T6 text", press V), "P5T6 text" selected.
- Open the family menu. It has a search field over a list at most 320 px tall, and long names are cut with an
  ellipsis. The list fills once the font scan is done. Type part of a name, and pick a family with Enter or a
  click: the text changes to it. The style menu then lists that family's styles. Pick Bold: the text turns bold.
- Then press R without clicking the canvas: the Rectangle tool comes on. Press V.
- Open the family list again, press Escape, press R: the Rectangle tool comes on again. Press V.
- (Review Focus 5) With "P5T6 text" selected, make its first word bigger than the rest:
  `ctl '{"cmd":"canvas","op":{"op":"command","command":"text.style","params":{"start":0,"end":10,"fontSize":40}}}'`.
  The size field reads "Mixed". Type 30 in it and press Enter: the whole text is 30 on the canvas, both words,
  and the field reads 30. Click the text on the canvas and press ⌘Z: the two sizes come back, and the field
  reads "Mixed" again.
- Try size 30, line height `auto`, `120` and `150%`, and letter spacing `-1` and `2%`. Each applies. `abc` and a
  negative line height are refused, and the field shows the old value. The marked align and resize buttons
  follow the text.
- The plan expected the example's phone-frame texts to read "Inter" in the family menu while the canvas drew
  them in Helvetica Neue. Since the cutover Sawit embeds Inter, so they should now draw in Inter too.

Answer: ______

**P5-8. Export 2× PNG** (Step 3, and ruling P5-C12).
- Select the probe (rows PNG 2× and SVG 1×), and click "Export 1 layer". Finder opens in
  `<project>/.nyawit/exports/` with the first file selected. The files are `P5T6 probe@2x.png` and
  `P5T6 probe.svg`. The PNG is about 492 × 332 px: 440 × 280, plus room for the shadow.
- Click the button again: new files named `P5T6 probe@2x-2.png` and `P5T6 probe-2.svg`, and the first two are
  untouched.
- Select a layer with no export row: it shows "PNG 1×", and the button exports it so. Change that row's scale to
  2×: it reads "PNG 2×", and the layer now holds the row (⌘Z takes it out). Click "+": a second row appears.
  Select two layers: the button reads "Export 2 layers".

Answer: ______

**P5-9. ⌥8 on the real keyboard** (the P5.T4 ruling). Click the canvas, and press ⌥8: the Design panel hides,
and the top bar's Design toggle loses its mark. Press ⌥8 again: both come back. Click the toggle itself: the
same happens. Over the socket the key op did this both times.

Answer: ______

**P5-10. The panel's fold** (Step 1, and the P5.T4 ruling). Select the Trail card, with the window as tall as
the screen. Scroll the panel with two fingers or the wheel: the panel scrolls and the canvas doesn't. At the
bottom, the Export row and the whole "Export 1 layer" button show above the window's edge. No scrollbar is
drawn, as in Sawit's other side panels.

Answer: ______

**P5-11. Auto layout and align** (Step 1, and the P5.T3 and P5.T4 reviews).
- Select the Trail card. Click each direction button (vertical, horizontal, wrap): its children line up that
  way. Click a cell of the 3 × 3 grid: they align there. Scrub the gap and the padding labels. Click the button
  beside the padding field: four fields, one for each side. Type 12 in the top one: only the top changes.
- Draw a frame with the Frame tool and put two rectangles in it. Click "+" in the empty Auto layout header: the
  frame gets an auto layout, and the rectangles line up. "−" takes it away. Each is one ⌘Z.
- Select three rectangles in different places. Click align-left, then distribute horizontally. The last two
  buttons are off with fewer than three layers. Each is one ⌘Z step, and hovering a button shows its name and
  keys.

Answer: ______

**P5-12. A gradient row** (the P5.T5 ruling and its deferred minor). Select the Trail card's photo, which has a
linear gradient. ⌘-click it, or run
`ctl '{"cmd":"canvas","op":{"op":"command","command":"select.set","params":{"ids":[30]}}}'`. The row reads
"Linear gradient". Click its replace-with-solid button: it becomes a solid, and ⌘Z brings the gradient back. Its
swatch is a grey ramp, not the gradient's colours (the deferred minor): say whether that matters (Q9).

Answer: ______

**P5-13. How it looks and feels** (the P5.T4 and P5.T5 rulings).
- Hover each kind of control: buttons, fields, menus. Focus a field: its ring shows. Say whether the panel looks
  like part of Sawit (its colours are the theme's), and whether anything is hard to hit, cramped or cut.
- With Sawit's sidebar shown, drag the window's edge in to its 720 pt minimum. The panel keeps its 264 pt, and
  its fields stay whole. The canvas keeps the rest, and the toolbar scrolls inside it (P3-6).
- Added from the Plan 5 final review: this check predates the layers panel. On the example, both panels show
  (240 pt and 264 pt), and at the minimum window the canvas between them is only a strip (the final review put
  it at about 55 pt). In a tab narrower than about 504 pt, the Design panel is pushed past the tab's edge until
  one panel is hidden. Say whether a panel should give way (Q11).

Answer: ______

## Plan 6: menus, the palette and the shortcut sheet: 10 checks

Start: A, with the recipe's `open_page` line run. Click the canvas once, so that it has the keyboard, and put
two stickies on it: press Q, click, and press Escape, then do it again a little to one side. The plan ran a
debug build with `SAWIT_NATIVE_CANVAS=1`; that flag no longer does anything.

**P6-1. The ≡ menu and each submenu.** The keys sit flush right in one column and stay readable on the
highlighted row.

Answer: ______

**P6-2. Context menus.**
- A two-finger tap (or the mouse's right button) and a ⌃-click each open the menu at the pointer:
  - on an unselected sticky, which becomes the selection;
  - on empty canvas with something selected: the selection clears and the four empty-canvas items show.
- A ⌃-click does nothing else: no drag, marquee or tool stroke.
- With a sticky selected, the menu stays inside the window, also when you open it near the window's bottom
  edge. A two-finger swipe scrolls it to its last item, Select all.

Answer: ______

**P6-3. The shortcut sheet** (⌃⇧? or ≡ ▸ Help). Its groups scroll with a two-finger swipe. Scroll to the end and
confirm that there is no ⌘W row and no ⌘J row. In Text you will meet ⌥⌘J (Justify): that is FigCraft's key, not
a ⌘J row.

Answer: ______

**P6-4. Import from file… (⇧⌘I).** The open panel's button reads Import. ⌘-click two images: both come in at the
middle of the view, selected. Place image… (⇧⌘K): its button reads Place. Cancel in either changes nothing and
shows no toast.

Answer: ______

**P6-5. Save as… (⇧⌘S) on a project's canvas.** The save panel opens in the project folder with
canvas.figcraft in its name field. Save shows the toast "Saved a copy as canvas.figcraft".

Answer: ______

**P6-6. Export… (⇧⌘E)** with a sticky selected, then with nothing selected. Each time a Finder window opens on
`.nyawit/exports/` with the new file selected. Export selection as SVG… and Export frames to PDF… do the same.

Answer: ______

**P6-7. Export frames to PDF… on a board.**
- Set up the board: 3 stickies, one of them inside a section, and a connector between the other two. Select
  nothing, then export.
- Preview shows 3 pages, one sticky each, ordered by top edge, then by left edge. There is no page for the
  section or the connector.
- Then say whether boards keep this rule or get their own: the whole board on one page, or a page per section
  (Q21). Neither answer fails the task.

Answer: ______

**P6-8. Copy as ▸ Copy as PNG (⇧⌘C)** with two stickies selected, then Preview ▸ File ▸ New from Clipboard: one
picture of both, in the canvas's colours.

Answer: ______

**P6-9. Copy as text, CSS and SVG,** each pasted into TextEdit in plain-text mode:
- text: the stickies' text, one line each;
- CSS: the first sticky's CSS, with its width and height;
- SVG: an `<svg …>` document.

Answer: ______

**P6-10. The canvas gets the keyboard back** (V and H switch tools).
- After each dialog above closes.
- After the ≡ menu, the palette, the sheet and the context menu close: on Escape in each, and on a choice in the
  ≡ menu, the palette and the context menu that opens no dialog or sheet. Tests cover this for the palette, the
  sheet and the context menu, but not for the ≡ menu.
- Added from the Plan 6 final review: open the palette (⌘K), switch to another tab (⌃Tab) and back, then type.
  Say where the keys go. The code suggests the palette may need one click before it takes keys again.

Answer: ______

## Plan 7: agents and the old canvas: 2 checks

Both checks compare the native canvas with the old Excalidraw canvas, and only an older build still draws that.
- Since P8.T1 (`763326c`), main has neither the `SAWIT_NATIVE_CANVAS` flag nor the Excalidraw canvas. Every
  build from main shows the native canvas, and the first time it opens an old board it migrates it.
- The installed Sawit.app (0.0.7) can't stand in for the old canvas: it predates `.nyawit/`, so it doesn't read
  a project's `.nyawit/canvas.excalidraw`.

Set-up:
- The native canvas: `target/debug/sawit` and `target/debug/sawit-mcp`, built from main.
- The old canvas: a build of `3c35377`, the last commit with the Excalidraw canvas.
  - Make a checkout of its own: a separate clone, or `git worktree add --detach <dir> 3c35377`.
  - Build it there with `cargo build -p sawit -p sawit-mcp`. This is a full build of its own, several GB on disk.
  - Start its `sawit` and its `sawit-mcp` without `SAWIT_NATIVE_CANVAS`.
  - For P7-2, excalidraw.com (its Open menu) draws the old canvas too.
- Give each run its own scratch project and Sawit home:
  `SAWIT_HOME=<scratch>/home target/debug/sawit --foreground <scratch>/proj`. Give that build's `sawit-mcp`,
  which the agent calls, the same `SAWIT_HOME`. Then neither build reads or writes your own Sawit data, or the
  other build's.
- Every native run needs a fresh copy of the board. The first open migrates it, and the first autosave writes
  `canvas.figcraft` beside it, which every later open reads instead.

**P7-1. Mermaid parity** (the P7.T3 carry).
- In each build, open the Canvas page and have an agent call `canvas_add_mermaid` for each source below.
- In the native run, also put Pele's own picture of each source under its drawing: render it with
  `render_mermaid` (light), and place it with `canvas_add_elements` `{type: "image", svg, name: "pele.svg"}`.
- Sources:
  - (a) `stateDiagram-v2` / `[*] --> Still` / `Still --> Still : tick` / `Still --> Moving`;
  - (b) `sequenceDiagram` / `Alice->>Bob: Hello` / `Note over Bob: Thinks` / `Bob-->>Alice: Hi` /
    `Note over Alice,Bob: Both`;
  - (c) `classDiagram` / `Animal <|-- Dog` / `Animal *-- Leg` / `Animal ..> Food: get`;
  - (d) `flowchart LR` / `A -.->|maybe| B` / `A ==> C` / `C --> C`.
- Compare each native drawing with Pele's picture under it and with the `3c35377` build's drawing. Choose which
  Skeleton fields to add for parity: `fontSize` for texts and labels, label colour, `strokeStyle` and
  `strokeWidth`, a start cap and the kind of each cap, and curved or routed bound edges. None of them fails the
  task. The plan's Task 7 notes put each difference into words.
- Added from the Plan 7 final review: in the native run, also have the agent call `canvas_add_elements` with a
  rectangle that has no `backgroundColor`. It comes in filled with the sticky yellow (the engine's default),
  where the old canvas draws an outline. Say which default you want (Q14).

Answer: ______

**P7-2. Import losses on a real old board** (the P7.T4 carry (3)). This is also Plan 8's hand check 11, the
real-board walk-through. Answer it before `/consolidate` (Q2).
- Make two copies of the old board. Never use the original:
  `mkdir -p "$S/real/.nyawit" "$S/old/.nyawit" && cp ~/Library/Application\ Support/sawit/canvas.excalidraw "$S/real/.nyawit/" && cp ~/Library/Application\ Support/sawit/canvas.excalidraw "$S/old/.nyawit/"`.
- Run a build from main on `$S/real` (the release build is what ships) and open the Canvas page. The user-flow
  board comes in whole. P8.T4 counted 86 top-level layers: 31 rounded rectangles, 8 rectangles, 3 diamonds, 2
  texts and 42 connectors.
- Open `$S/old` in the `3c35377` build, or open its `canvas.excalidraw` in excalidraw.com.
- Say which losses matter, so that the follow-up adds those Skeleton fields:
  - text sizes: the 36 and 24 px headings come in at 12 px;
  - label colours: the legend's labels come in ink;
  - fonts: Excalidraw's hand-drawn one, and Nunito;
  - marker kinds and a head at each end: crow's feet, bars, diamonds and hollow triangles become one filled
    triangle at the end;
  - dashes and widths;
  - curved routes: 30 edges come in straight, and the four into "Workspace" meet at one point;
  - and for boards drawn by hand: rotation, opacity, fill style, roughness and groups.

Answer: ______

## Plan 8: cutover: 5 checks (its hand checks 4–6 and 9 point at earlier checks, and 8 and 11 are kept as P2-12 (e) and P7-2)

Start: the release build, with `mkdir -p "$S/home" "$S/proj" "$S/proj2" && echo hi > "$S/proj/notes.txt"` before
the recipe (start state A). Open the Canvas page with ⌃⌘C.

**P8-1. Opening, maximizing and the tab bar** (Step 5, ruling P8-C8, and the P8.T1 delegate). Run P1-4 first if
you haven't. Then click empty canvas, so that it has the keyboard, and edit no text:
- ⇧⌘↩ hides the tab bar, and the canvas fills the window. A second ⇧⌘↩ puts the layout back.
- Maximize again, then press ⌘B: the sidebar and the tab bar come back.
- Open `notes.txt` from the Files tab, go back to the canvas tab and maximize again, then press ⌃Tab: the file's
  tab shows, with the tab bar.

Answer: ______

**P8-2. Panning the budget board** (ruling P8-C13). Build the board over the socket:

```bash
for t in retrospective brainstorm weeklyPlanner flowChart kanban retrospective brainstorm weeklyPlanner flowChart kanban; do
  ctl "{\"cmd\":\"canvas\",\"op\":{\"op\":\"command\",\"command\":\"board.template\",\"params\":{\"name\":\"$t\"}}}"
done
for i in 1 2; do
  ctl '{"cmd":"canvas","op":{"op":"command","command":"select.all"}}'
  ctl '{"cmd":"canvas","op":{"op":"command","command":"edit.duplicate"}}'
done
ctl '{"cmd":"layout","sidebar":false,"terminal":false}'
ctl '{"cmd":"canvas","op":{"op":"command","command":"view.zoomFit"}}'
```

- Pan with two fingers. The board follows at once, because the last frame moves with the fingers. A fast pan
  shows the page colour at the leading edge until the board fills in. Nothing jumps when a new frame lands.
- Pinch to zoom. After the fingers stop, the board stays soft for about 0.4–0.7 s. That is the 150 ms wait for
  the zoom to settle, then the 0.2–0.5 s full render P8.T4 measured on this board. Say whether that is
  acceptable until the follow-up in the spec's Risks lands (Q3).

Answer: ______

**P8-3. Bold Inter without Inter installed** (the P2.T7 carry). It replaces the Helvetica Neue workaround of
Plan 2's hand check 4. This Mac has no Inter installed, so the check shows the fonts Sawit embeds.
- Put a sticky with the toolbar's sticky tool, type two words, select one and press ⌘B: that word turns bold on
  the canvas.
- Then select a text layer (T, a click, a word, Escape makes one) and show the Design panel (⌥8). Typography's
  family reads Inter, and its weights run from Thin to Black.

Answer: ______

**P8-4.** Tools, flyouts, stickies, connectors and text with IME: covered by P3-1 to P3-5, P2-1, P2-7 and P2-10.

**P8-5.** Layers, drag and rename: covered by P4-1 to P4-4.

**P8-6.** Design panel fields: covered by P5-1 to P5-8.

**P8-7. Menus, the palette and the sheet** (Step 5). P6-1 to P6-3 and P6-10 cover the menus and the sheet. The
step kept here: with a sticky selected, open the palette (⌘K), type "dupl" and press Enter. The sticky is
duplicated.

Answer: ______

**P8-8.** Copy and paste between two project canvases: kept as P2-12 (e), with this check's steps.

**P8-9.** Export: covered by P6-6 to P6-8 and P5-8.

**P8-10. `canvas_add_mermaid` from Claude Code** (Step 5).
- Point Claude Code's Sawit MCP server at this build: a symlink named `sawit-mcp` to `target/release/sawit`,
  started with the same `SAWIT_HOME`. Or, once `/consolidate` has released it, use the installed app with a
  scratch folder's window in front.
- With the canvas open, ask Claude Code to add `flowchart LR; A[Start]-->B{Ok?}; B-->|yes|C[Done]` to the
  canvas.
- Three editable shapes come in beside what is there, with two connectors joined to them and "yes" on the
  second. The tool answers `added: 5`.
- With Sawit closed, the tool edits the saved file instead. That needs no hand check: sawit-mcp's offline tests
  cover it (`the_other_canvas_tools_edit_the_stored_canvas_while_the_app_is_closed`, the P8.T2 delegate ruling).

Answer: ______

**P8-11.** The real board, walked through: kept as P7-2.

## Questions for you

There are 26 questions. They come from the ledger lines that leave a decision or a fact to you.
- Each names its source. For example, `P3:117` is line 117 of
  `.superpowers/sdd/2026-10-29-native-canvas-3-toolbar/progress.md`.
- Q2 and Q3 are best answered before `/consolidate`.

**Q1. Did you use the debug Sawit windows on 2026-10-30, around 18:17–18:18 and 22:38–22:39?**
- 18:17:20–18:18:34: this was Plan 3 Task 4's live check, on an empty board in lane 4.
  - FigCraft's Flow chart template appeared on the board and was removed again.
  - The zoom went from 100 % to 34 %. No zoom step or fit gives 34 %, but a pinch or a ⌘-scroll does.
  - The window lost key focus in between.
  - `.nyawit/canvas.figcraft.bak`, written at 18:18:21, holds the flow chart.
  - The instance listened on `/tmp/sawit-$USER-945615192beed5f.sock`. Neither sawit-mcp nor the `sawit` CLI
    uses that socket without the check's scratch `SAWIT_HOME`.
- 22:38:47–22:39:26: this was Plan 4 Task 3's live check, on the example design file in lane 3.
  - The zoom went from 15 % to 45 %, and four rows of the layers panel opened. Nothing was edited.
  - The log got 63 render lines, as a continuous zoom gesture leaves.
  - The check's `focus` command had brought the window to the front at 22:38:46.
- If it was you, nothing more is needed. If it wasn't, something drove a debug instance's control socket or
  posted input to its window, and who can reach Sawit's sockets needs its own look.
- Neither check's verdicts depend on these changes.
- Source: P3:117, P3:126, P4:104. See also Plan 3's Task 4 notes and Plan 4's Task 3 notes.

**Q2. Which looks of the old canvas should migrated boards and Mermaid keep?** (P7-1, P7-2)
- The import goes through the Skeleton, the MCP element fields, so it loses:
  - text sizes;
  - label colours;
  - Excalidraw's fonts;
  - curved routes;
  - marker kinds, and a head at each end;
  - dashes and widths;
  - for boards drawn by hand: rotation, opacity, fill style, roughness and groups.
  Mermaid from `canvas_add_mermaid` has the same gaps.
- The `.excalidraw` file is never changed. But the first time a build from main opens an old board, it writes
  `canvas.figcraft` beside it, and every later open reads that instead.
- A Skeleton field added later reaches such a board only if it is imported again (move `canvas.figcraft` aside,
  or use `canvas_load`), and that drops what was drawn since. So answer before `/consolidate`, and before you
  open a real old board in a build from main.
- The follow-up is about 150–250 lines with tests.
- Source: P7:140, P7:149, P7:208, P8:124.

**Q3. Does the slow settle block a release?** (P8-2)
- The spec's budget is a p95 of 50 ms for a re-render after an edit or a settle.
  - Edits pass, at 40.5 ms, and an idle canvas uses about 0 % CPU.
  - Settles miss when many stickies are in view: 536.8 ms on the 776-node budget board and 381.6 ms on a
    501-node board.
  - Both boards are piles, with every template at the view's centre, so those numbers are upper bounds.
  - Laid out over a page, the same 501 nodes settle at 47.0 ms, and the real old board at 22.3 ms.
- The cause: FigCraft blurs each sticky's drop shadow on the CPU in every full render.
- What you see: the board stays soft for about 0.4–0.7 s after a zoom settles. Input is never blocked, because
  renders run on the canvas's own thread.
- The follow-up, in the spec's Risks, is a change to FigCraft's renderer plus a bump of the pinned rev.
- Source: P8:123, and Plan 8's Task 4 notes.

**Q4. Keep the embedded Inter fonts (1.71 MiB)?**
- Since P8.T1, Sawit embeds `InterVariable.ttf` and `InterVariable-Italic.ttf`: 1,789,960 bytes, in the binary
  and in git history.
- Every new sticky, text and label starts in Inter. Without the embed, a Mac without Inter (this one) draws Inter
  text in Helvetica Neue Regular whatever its style. ⌘B, italics and weights would then change the document but
  not the canvas.
- NOTICE and README credit Inter, and Sawit.app ships NOTICE and `OFL-Inter.txt` (`5c878d3`).
- Keeping it costs 1.71 MiB. Removing it brings the fallback face back. P8-3 checks it.
- Source: P2:170, P8:87, P8:100.

**Q5. R-EHFRAME: take option (b)?**
- Every debug link of the app prints
  `ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table`.
- The debug app has 22,694,284 bytes (21.64 MiB) of `__eh_frame`, where release has 3.56 MiB. The debug binary
  is 287 MiB.
- The warning only slows exception unwinding (panics, backtraces) in debug builds.
- In today's gate run, the `sawit` test binary's `__eh_frame` was 15.46 MiB, just under the limit, so
  `cargo test` printed nothing. `cargo build -p sawit` still prints it.
- Option (b): set `opt-level` for all dependencies in the dev profile. That is one Cargo.toml block and a full
  dependency rebuild in every target dir, so it is cheapest once the lanes are gone.
- A faster debug canvas also needs its own `[profile.dev.package.sawit-board]` entry, because (b)'s `"*"` covers
  dependencies, not workspace members.
- Or keep the warning as known.
- Source: P1:160, P1:180, P1:193, P8:102, P8:125, and the program ledger's R-EHFRAME.

**Q6. Deep rows in the layers panel: cap the indent, or scroll sideways?**
- Rows indent 12 pt a level, with no cap.
  - From about depth 11, names are cut.
  - From about depth 14, only the arrow and the icon show.
  - From about depth 19, nothing of a row is inside the 240 pt panel.
- Deep rows stay clickable and selectable, and FigCraft's own panel runs off the same way.
- Either fix is a few lines in the row's drawing.
- Source: P4:113, and Plan 4's Task 3 notes ("Seen, not defects").

**Q7. Enter in a Design panel field: give the keyboard back to the canvas?** (P5-2)
- Today Enter applies the value and keeps the keyboard in the field, with the caret at the end. So typing 5 after
  "40, Enter" gives 405.
- A canvas key typed next (a digit for opacity, Backspace) edits the field, and the next press on the canvas
  applies it.
- The final review recommends giving the keyboard back, or at least selecting the text again. That is one focus
  call in the field's Enter handler.
- Source: P5:181.

**Q8. Scrubbing a "Mixed" value: move each layer from its own value?** (P5-1)
- Today the first pixel of a drag sets every selected layer to one value, counted from the first layer's.
  FigCraft and Figma move each layer from its own value.
- The change is each layer's start plus the change, in one `command.batch`.
- Source: P5:181.

**Q9. The gradient row's swatch: is a grey ramp enough?** (P5-12)
- A real swatch needs the gradient's stops in `PaintKind::Gradient`, in sawit-board.
- Source: P5:181.

**Q10. Tab and ⇧Tab between Design panel fields?** (P5-2)
- Tab in a field does nothing, because the spec gives Tab to the canvas (FigCraft's select-next), and no field
  order exists.
- A field order is a Tab binding in a field-level key context, with one GPUI test.
- Source: P5:183.

**Q11. A narrow window with both panels: should one panel give way?** (P5-13)
- On a design file at the 720 pt minimum, the canvas between the 240 pt and 264 pt panels is a strip.
- In a tab narrower than about 504 pt, the Design panel is pushed past the tab's edge until one panel is hidden.
- No data is at risk.
- Source: P5:179.

**Q12. Should an agent's canvas reads leave your tabs alone?**
- Every canvas op from an agent, `canvas_get` included, opens the canvas tab and makes it active. That shows the
  canvas over a maximized terminal, and it makes the requested project the window's active one.
- With no project named, the app path draws on the active project of the window that has the agent's working folder
  open (else the frontmost window), while the closed-app path draws in the agent's working folder.
- The fix is the browser tools' rule: only ops that show something bring the tab forward. That is a few lines in
  sawit-mcp or `control.rs`.
- The browser follows that rule, and since final-seams m1 it no longer has the second inconsistency: with no
  project named, an agent's `browser_*` ops and `app_open_page browser` use the browser of the project its
  working folder is in, as `browser_navigate` opens that folder's dev server. Before, they used the active
  project's browser, so with projects A and B in one window and B shown, an agent in A opened A's dev server in
  B's browser, with B's cookies, and its screenshots and exports went into B's `.nyawit`. Ops in the background
  change nothing on screen; one that shows a page brings that browser forward with its project. The canvas
  could take the same rule.
- Settled for the second inconsistency (v0.0.8 parity): with no project named, an agent's canvas ops and
  `app_open_page canvas` use the canvas of the project its working folder is in, as its browser does, and with the
  app closed sawit-mcp edits that project's canvas: within the agent's git repository, the nearest folder up from
  its folder whose `.nyawit/` holds a canvas, else the repository's root; outside a repository, the folder itself.
  A `.nyawit/` with no canvas in it, or one above the repository, decides nothing. Every canvas op still brings the
  canvas tab forward.
- Source: P1:186, P7:212.

**Q13. May an agent's op open UI that takes the keyboard?**
- An agent's `object.rename` with no name, `canvas.renameLabel`, or a pointer-op double-click on a frame's name
  opens the rename field in the layers panel, and the field takes the keyboard. This happens even while you type
  elsewhere in the window, such as a terminal pane.
- Your next keys then go into the field, and Enter or a click names the layer with them. One ⌘Z undoes it.
- The same question applies to the Actions palette and the context menu.
- The follow-up starts the field without the keyboard unless the canvas had it: a few lines and one test.
- The run sent this to the controller for Plan 7's final review, and no ruling was recorded.
- The terminal has since answered it for itself: an agent's terminal commands never take the keyboard, don't
  switch away from a terminal you're typing in, and never go into the shell you're typing in (H-TERM-5).
- So have the pages an agent opens with `app_open_page`: Dev Tools and MCP (CATALOG-15), and the Keymap Editor,
  where a ⌫ meant for the terminal removed the selected binding from `keymap.json` (final-seams m2).
- One route still takes it, on purpose for now: `app_open` of a canvas file (`.figcraft`, `.excalidraw`) gives
  that canvas the keyboard, from an agent too, also when its tab was open already and is only shown again
  (review-canvas-ui m2). Typing in a terminal pane then goes to the canvas, where letters pick tools and ⌫
  deletes the selection, until you click back. It is kept because a canvas file opened from Files or with
  `sawit` should take the keyboard (CANVAS-UI-2), and the control socket's `open` runs the CLI's own open.
  Narrowing it to the user's routes is a flag from `cli::handle` for the socket's `open`: a few lines. Decide it
  with the rest of this question.
- Source: P4:112.

**Q14. Agent shapes with no `backgroundColor`: sticky yellow, or no fill?** (P7-1)
- Today a rectangle, ellipse or diamond sent with no `backgroundColor` is filled with the engine's sticky yellow,
  so that the app and the closed-app path draw the same. The old canvas drew outlines.
- An agent can ask for no fill with `backgroundColor: "transparent"`.
- No fill by default is a few lines in `skeleton.rs`.
- Source: P7:209, P7:121.

**Q15. Refuse `font.download` for agents?**
- FigCraft refuses `font.download` unless font downloads are turned on in another Craft app's Missing fonts
  window. That setting is shared between the apps.
- When it is on, an agent's `canvas_command font.download` can install a Google Fonts or Noto family into the
  shared Craft font folder, without a click in Sawit.
- Refusing it is one more id in `ops::refused`.
- Source: P2:178, P8:129.

**Q16. A click away while composing with an input method: keep what it commits, or cancel?** (P2-10 (3))
- Today the sticky keeps what the input method commits on the click (ni or 你), as TextEdit and other Mac text
  views do. FigCraft cancels the composition instead.
- Cancelling needs a hook that gpui-pre lacks: AppKit's current event, or putting off the commit until after the
  press.
- Source: P2:162.

**Q17. ⌘C on the canvas: keep the `sawit-canvas:N` marker on the system clipboard?** (P2-12 (a))
- Today another app pastes `sawit-canvas:N`. Copy as… (Text, CSS, SVG, PNG) gives other apps the content.
- FigCraft puts the text of a text layer, or else an SVG of the selection, on the clipboard, and keeps its marker
  in the clipboard item's metadata.
- Copying that is about 40 lines and two tests.
- Source: P2:181.

**Q18. The old canvas's laser pointer, and a tool lock on the toolbar: do you want them?**
- The native canvas has no laser pointer. FigCraft has nothing like it, so it would need a spec of its own.
- Tool lock is Preferences › Keep tool selected after use, in the ≡ menu, and not on the toolbar. A toolbar
  toggle is a small follow-up.
- Source: P2:183.

**Q19. A right-click on empty canvas while something is selected: clear the selection (today) or keep it?**
- Today, ruling P6-C10 ends any text edit, clears the selection and shows the four empty-canvas items: Paste
  here, Select all, Zoom to fit and Show/Hide UI.
- FigCraft keeps the selection, and then Paste here, Zoom to fit and Show/Hide UI can't be reached from a
  right-click while anything is selected.
- Going back to FigCraft's way is a revert of that ruling.
- Source: P6:78.

**Q20. Export with nothing selected: one file of the whole page (today), or a file per top-level layer?**
- Today it writes one PNG or SVG of the page, named after the page (ruling P6-C8). FigCraft keeps a file per layer
  for Export layers….
- To get a file per layer today, select the layers. Switching the default is a few lines.
- Source: P5:161.

**Q21. Export frames to PDF on a board: a page per item (today), the whole board on one page, or a page per
section?** (P6-7)
- Today every sticky, shape, stamp, connector label and table gets a page, because FigCraft builds them as
  frames. A sticky inside a section gets one too. Connectors, ink, loose text and the sections themselves get
  none.
- The whole board on one page is a few lines and a test. A page per section needs a figcraft-pdf dependency and
  about 20 lines.
- Source: P6:111.

**Q22. A colour picked while a pen path is open: end the path first?** (P5-5)
- Today the pick joins the path's gesture. Escape in the picker keeps the colour, and one ⌘Z after Enter takes
  away the path and its colour together. A label drag does nothing.
- In a rare case, an agent op ends the path while the picker is open. Then each later colour is its own undo
  step.
- Ending the path first, as an open text edit is ended, is a few lines in `scrub_begin`.
- Source: P5:188.

**Q23. Report FigCraft's `node.set` bug upstream?**
- At the pinned rev `0971d09`, `node.set` drops export options whose names have two words, and it makes no undo
  step inside an interaction.
- Sawit works around the first and builds on the second, and tests pin both.
- Reporting it is optional.
- Source: P5:187.

**Q24. Third-party notices for everything Sawit.app bundles?**
- Sawit.app ships NOTICE (FigCraft) and `OFL-Inter.txt` (Inter).
- It doesn't ship the notices of Lucide, material-icon-theme, Pele, QuickJS or the Rust crates (MIT, ISC, BSD and
  Apache-2.0 components). No release ever did.
- The fix is a generated third-party licence file (cargo-about, say) plus the asset notices, which
  `bundle-macos.sh` copies beside NOTICE.
- Source: P8:127.

**Q25. Two one-off pauses: accept them, or follow up?**
- The first `board.template` after a launch held the UI thread for 300 ms. Later ones took 1–23 ms.
- Opening a canvas that migrates an old board held it for 297 ms on the 134-element real board, and would take
  longer on a bigger one.
- The spec sets no budget for either. A follow-up, such as running the import off the UI thread, would remove
  them.
- Source: P8:126, and Plan 8's Task 4 notes ("Not failures, for the user").

**Q26. Commit trailers name the model that wrote each commit: keep them?**
- Under ruling R8, each of the run's commits says `Co-Authored-By: Claude Sonnet 5.5` or
  `Co-Authored-By: Claude Opus 5.5`, for whichever model wrote it, instead of the plans' one fixed line.
- One fixed line would mean rewriting those commits before any push.
- Source: P1:52.

**Questions asked inside the hand checks** (answer them there):
- P2-10 (3), (4), (8) and (9);
- P2-11 and P2-14;
- P3-1 and P3-6;
- P4-4 and P4-6;
- P5-1 (Q8), P5-2 (Q7, Q10), P5-6, P5-12 (Q9) and P5-13 (Q11);
- P6-7 (Q21) and P6-10;
- P7-1 (Q2, Q14) and P7-2 (Q2);
- P8-2 (Q3).

## From the merged pull requests

Added on 2026-10-31 from main `b1cd411`. Main now holds the branches of the three open pull requests: PR #1
(feat/terminal-panes), PR #2 (keymap-editor) and PR #3 (feat/browser-preview). Each body ends with a test plan whose
manual items are still unticked (`gh pr view <n> --json body`, read only). Main contains each PR's head, so GitHub
marks the PRs merged once main is pushed, and their lists are easy to lose after that. They are kept here.
- There are **11 checks**, grouped by pull request. `PR1-3` means PR #1's third unticked item.
  - PR1-1 to PR1-6, PR2-1 and PR3-1 are the pull requests' own items, in their order.
  - PR3-2 is the smoke step of the browser-preview consolidation report (`browser-preview.md`, §6.5).
  - PR2-2 and PR2-3 come from the keymap-editor consolidation review (`keymap-editor-review.md`, M2), which asks for
    them to be carried here.
  - A step added to a check from elsewhere says where it comes from.
- Nobody has run these yet. The consolidation ran only the gates and the GPUI tests on the merged tree, and
  synthetic input can't reach the window (rule R-LIVE).
- Build and launch as under "Before you start", with start state A: leave out the copy line and the `open_page`
  line. A debug build is enough for all of them. Use a fresh scratch folder for each pull request's list.

### PR #1: terminal split panes: 6 checks

Start: A. Press ⌘J for the terminal dock, and click in the terminal so that it has the keyboard.

**PR1-1. Splitting, dividers and focus.**
- Run `cd /tmp`, then press ⌘D. A new pane opens on the right, in `/tmp` (check with `pwd`), and takes the
  keyboard. ⇧⌘D opens one below the focused pane.
- Once a tab has more than one pane, each pane has a header: its name or folder, and Split right, Zoom and Close
  buttons. The focused pane's border has the accent colour.
- Drag a divider: both sides resize, and neither gets smaller than 20 columns by 4 rows. Double-click a divider: the
  two sides even out.
- ⌥⌘ with ←, →, ↑ or ↓ moves the keyboard to the neighbouring pane. The tab's label shows the focused pane's title.

Answer: ______

**PR1-2. The pane menu.** Right-click a pane of a split tab.
- The menu has Rename pane, Split right, Split down, Zoom, Send right-clicks to pane and Close pane. In a tab with
  one pane, Zoom is greyed out.
- Rename pane: the name field takes the keyboard at once (the PR's "Rename focus"). Type a name and press Enter. The
  header shows it, and keeps it when a program sets a title: run `printf '\e]0;other\a'`. Renaming to nothing
  brings back the program's title. Escape leaves the name as it was.
- Send right-clicks to pane: tick it, run `vim`, and type `:set mouse=a`. A right-click now goes to vim, and
  Shift+right-click opens the menu. Shift+right-click opens it in every pane, ticked or not.
- Split right, Split down, Zoom and Close pane do what the keys and the header's buttons do. A closed pane's space
  goes to its neighbour.

Answer: ______

**PR1-3. Exits, SSH and Run in a split.**
- In a split, type `exit` in one pane. The pane closes, and its neighbour takes the space.
- Type `sleep 3; exit` in a pane of a split, then press ⌘T at once for a new terminal tab and keep typing there.
  When the pane in the first tab closes, your typing stays in the new tab (`2c2f4b1`).
- In a tab with one pane, `exit` keeps the tab, with "This process has exited."
- SSH: open a session to any host from the SSH tab, split its tab, and type `exit` in the SSH pane. The pane stays,
  with "Disconnected from <name>.", Reconnect and Close Pane. Reconnect starts a new session in that pane.
- Run: in the Run tab, save a command (`ping localhost`, say) and start it. It runs in a terminal tab of its own.
  Split that tab with ⌘D, then click another tab. Click the command's row in the Run tab: its tab comes forward, with
  the command's pane focused. Stop (Ctrl-C) stops it.

Answer: ______

**PR1-4. ⇧⌘↩ with one pane and with a split.**
- In a tab with one pane, ⇧⌘↩ fills the window with the terminal dock. Pressing it again puts the layout back.
- In a split, ⇧⌘↩ zooms the focused pane over the whole tab. Its header says "Zoomed", and the pane menu's Zoom
  reads Unzoom. Pressing it again unzooms. Splitting, or moving to another pane, unzooms first.
- Both can be on at once: zoom a pane, then click the dock's Maximize Terminal button.

Answer: ______

**PR1-5. Long titles in narrow panes, and a short dock.**
- Split a tab right two or three times. Rename a narrow pane to `a-very-long-pane-name-that-does-not-fit`. The
  title is cut, and the header's buttons stay in view (`161312c`).
- Split a tab down two or three times, then drag the dock's top edge down until the dock is short. PR #1's
  follow-ups say a dock resize doesn't check the 20 × 4 minimum again, so a pane may get smaller. Say what the panes
  do, and whether anything overlaps or can't be used.

Answer: ______

**PR1-6. An idle split stays idle.**
- Split a tab into three panes, each at its shell prompt, and leave the window alone for 30 s.
- Run `ps -o %cpu= -p $PID` a few times, or watch sawit in Activity Monitor. Expect about 0 %, as with one pane.
  Before `161312c`, an idle split redrew about four times a second.

Answer: ______

### PR #2: the Keymap Editor: 3 checks (PR2-2 and PR2-3 are from the consolidation review)

Start: A. Open the Keymap Editor with ⌘K ⌘S, the keyboard button in the title bar, or Sawit ▸ Keymap Editor. With
the recipe's scratch `SAWIT_HOME`, its changes go to `$S/home/keymap.json`, not to your own keymap.

**PR2-1. Rebinding by hand** (the PR's item: the create and edit form, key recording, and saving a rebind, then
confirming that the new key works).
- Type `toggle terminal` in Filter action names. The row "workspace: toggle terminal" shows ⌘J, with the context
  Workspace and the source Default.
- Click its Edit button. In the form, click Keystrokes, press ⌃⌥J, and click Save. The row shows ⌃⌥J with the source
  User, and the default's row shows Removed. `cat "$S/home/keymap.json"` shows the new binding and the unbound ⌘J.
- Click the Files tab, then press ⌃⌥J: the terminal dock hides, and a second press shows it. ⌘J does nothing.
- Click Create Keybinding. Type `close all` in Action and pick a suggestion, record ⌃⌥W, and pick a context from the
  list. Save. Then create a second binding on ⌃⌥W in the same context, for another action: the form warns of the
  clash, and both rows show ⚠.
- Turn on Search by Keystroke, and press ⌃⌥W: only the bindings on those keys are listed.
- Click Restore the Default on the removed row, and remove the User rows: ⌘J toggles the dock again.

Answer: ______

**PR2-2. The canvas's keys** (added from the keymap-editor consolidation review, M2). The canvas's shortcuts are
now app key bindings (`board::Command`), and only GPUI tests have checked them.
- Open the canvas (⌃⌘C) and click it. Each of these works as before: the tools' letters (V, H, R, Q, …), ⌘C and
  ⌘V, ⌘Z, ⌥1 (layers panel), ⌥8 (Design panel), ⌘K (Actions palette), ⌃⇧? (shortcut sheet) and ⇧⌘↩ (maximize).
- Type in a sticky, in the layers panel's filter and in a Design panel field. The letters go into the text, and no
  tool changes.
- In the Keymap Editor, type `canvas:` in the filter. Rebind the rectangle's R to another free key, and remove one
  other canvas binding. On the canvas, the new key picks the rectangle and R doesn't, and the removed key does
  nothing. The canvas's own hints follow: the rectangle's tooltip, its menu item, its row in the Actions palette
  (⌘K) and in the shortcut sheet (⌃⇧?) show the new key, and the removed binding's command shows no key (its sheet
  row goes when it has no key left).

Answer: ______

**PR2-3. A hand edit of keymap.json** (added from the keymap-editor consolidation review, M2).
- With Sawit running, write the file by hand (this replaces what is in it):
  `echo '[{"context": "Workspace", "bindings": {"ctrl-alt-b": "workspace::ToggleSidebar"}}]' > "$S/home/keymap.json"`.
  About a second later the editor lists ⌃⌥B for "workspace: toggle sidebar", with the source User, and ⌃⌥B hides
  and shows the sidebar.
- Break the file: `printf '[' > "$S/home/keymap.json"`. The editor shows the error and the bindings stay as they
  were. A change made in the editor is refused with "Fix keymap.json first." until the file reads again. Write the
  file as above: the error goes.

Answer: ______

### PR #3: the Browser tab's Annotate and Design: 2 checks (PR3-2 is from the consolidation report)

Start: A, with a page to work on. Either start the dev server of one of your web projects and launch the recipe on
that folder instead of `$S/proj`, or serve the repository's fixtures from a checkout of main with
`python3 -m http.server 8765 -d docs/fixtures/browser`. Click Browser in the title bar, and open the dev server's
address (for the fixtures, `localhost:8765/annotate.html`). Annotations and exports go to the project's `.nyawit/`,
which is git-ignored.

**PR3-1. The ⌘⇧. pick flow, Send to terminal, and a design export** (the PR's item, "on a local dev server").
- (a) Annotate.
  - Click the page, then press ⌘⇧.: the Annotate panel opens beside the page, and picking starts. The element under
    the pointer gets an outline and a `tag#id.class W×H` label. ↑ and ↓ walk to the parent and back, and Esc stops
    picking.
  - Click an element (on `annotate.html`, the Submit order button). The click doesn't reach the page. A numbered
    marker shows on the element, and a new row in the panel takes the keyboard: type what should change, and pick
    Fix, Change, Question or Copy.
  - Added from the keymap-editor consolidation review (§6): click the panel, so that Sawit's side of the tab has the
    keyboard, and press ⌘⇧.. The review expects this binding not to fire on a real Mac, because macOS reports ⇧⌘.
    as `cmd->`. The page's own key, used while the page has the keyboard, is not affected. Say what happens.
  - Added from the keymap-editor consolidation report (§6): rebind "browser: toggle annotate" in the Keymap
    Editor. The page's script no longer answers ⌘⇧. (since the hunt's keymap round 3 merged KEYMAP-5 with the
    browser fixes): while the page has the keyboard, the old key does nothing and the new one toggles Annotate.
    Say what happens.
- (b) Send to terminal.
  - Run `claude` in a terminal tab, or leave zsh at its prompt. In the Annotate panel, click Send to terminal. The
    notes paste into that terminal as Markdown, without Enter. The terminal shows and takes the keyboard, and the
    panel's footer says "Sent to Terminal N".
  - With Include console errors ticked, the notes end with "Console errors on …" (`annotate.html` logs one on load).
  - Added from `b1cd411`, which made Send to terminal work with split panes: split that tab, and leave the keyboard
    in the pane running Claude Code. The paste goes to that pane only. Then, with no other terminal tab open, run
    `cat` in the focused pane and click Send to terminal again: the panel copies the notes instead and says "No
    terminal is ready for a paste, so it was copied." A pane without the keyboard is never used, even at a prompt.
    Say whether it should be.
- (c) A design export.
  - Click Design. In Inspect, hover and click elements: the panel shows the box model, typography, colours with
    their contrast, and the :hover and :focus states. Copy CSS, Tailwind and HTML, and paste each somewhere.
  - Colors, Fonts and Assets list what the page uses, and System shows the derived tokens.
  - In System, click Export. The panel says "Exported N files to …/.nyawit/design/<host>". Reveal in Finder shows
    DESIGN.md, tokens.json, theme.css, shadcn.css, variables.css, screenshot.png, components/, assets/,
    assets.json and README.md. screenshot.png shows no outline, label or marker.
  - Optional, if `npx` is installed: `npx @google/design.md lint <that folder>/DESIGN.md` reports no errors.

Answer: ______

**PR3-2. The browser beside the native canvas** (the smoke step in `browser-preview.md`, §6.5). The browser's own
end-to-end runs were on its old base, with the Excalidraw canvas, and on the merged tree only the gates ran.
- Open a page in the Browser tab, then the canvas (⌃⌘C). Nothing of the page shows over the canvas.
- Click the canvas and type: press Q, click, type a word and press Escape, then press V and H. Each key reaches the
  canvas: a sticky with the word, then the Move and Hand tools.
- Go back to the Browser tab, click the page, and press ⌘⇧.: Annotate opens. Press it again: it closes.
- Say whether any of the page, the panel or the keyboard ended up in the wrong tab.

Answer: ______

## Added during the consolidation (2026-10-31)

These come from the consolidation's fixes and reviews. Each starts from a fresh launch of the merged main.

- **C-1 Maximize hint follows a rebind.** Open the keymap editor and rebind the canvas's Maximize (default ⇧⌘↩) to another key. Open a canvas tab and hover the top bar's Maximize button. Look for: the tooltip names the new key, and the new key maximizes the canvas. Answer:
- **C-2 Annotate hint follows a rebind.** Rebind the Browser's Annotate (default ⇧⌘.) to another key with ⌘ and hover the Annotate button. Look for: the tooltip names the new key, and the new key toggles Annotate both while Sawit's side of the tab has the keyboard and after a click in the web page; ⇧⌘. no longer does, from either side. (A key without ⌘ types into the page while the page has the keyboard.) Answer:
- **C-3 ⇧⌘. after a pick (replaces PR3-1 (a)'s third bullet).** In the Browser, turn on Annotate, pick an element so the keyboard moves to the note box, then press ⇧⌘.. Look for: Annotate turns off. Answer:
- **C-4 No tool writes over a canvas file.** Ask an agent to run `browser_screenshot` with `output_path` set to the project's `.nyawit/canvas.figcraft`. Look for: the tool refuses ("would overwrite a canvas file") and the board still opens unchanged. Answer:
- **C-5 ⌫ while naming in Files.** In the Files panel, start a new file, type a name, then press ⌫ until the name is empty and once more. Look for: characters are deleted and nothing goes to the Trash. Then, in the tree with nothing selected, press ⌫. Look for: the project folder is not moved to the Trash. Answer:
- **C-6 terminal_run from an agent in a pane.** Run an agent (e.g. Claude Code) in a terminal pane and ask it to use Sawit's `terminal_run` without a new tab. Look for: the command runs in an idle shell (or a new tab), never typed into the agent's own prompt. Answer:

## Added by the keymap hunt (2026-10-31)

The keymap hunt's fixes (`fix-keymap.md`). GPUI tests cover the rest; these need a real keyboard, a web page or a
second keyboard layout. Each starts from a fresh launch of the merged main with the recipe's scratch `SAWIT_HOME`.

- **K-1 A web page's clipboard keys.** Open a page with a text field in the Browser (the `annotate.html` fixture).
  Type the address and press Enter, click the field, then press ⌘V, ⌘A and ⌘C, and type a word: each acts in the
  field, and the address bar is unchanged. Select some page text and press ⌘C, then paste it in a terminal: it is
  the page's text. Click the terminal, then the page's field, and press ⌘V: it pastes in the page, not the shell.
  In Files, select a file and press ⌘C, click the page's field and press ⌘V: no file is created. Edit ▸ Copy,
  Paste and Select All act on a Sawit text box too. An agent's click leaves your keyboard alone: run an agent (e.g.
  Claude Code) in a terminal, ask it to `browser_click` the page's field, then type: the keys go to the terminal.
  Do it again with another tab in front of the Browser: the keys still go to the terminal, and ⌘T and ⌘1 still
  work. Click in the page, press ⇧⌘J twice: the tab area hides and comes back. Click in the page, press ⌃⌘M, then
  ⌘W: the MCP page closes. Answer:
- **K-2 A German keyboard layout.** Add German in System Settings ▸ Keyboard and switch to it with Sawit running.
  On the canvas and in the Browser, ⌘< and ⇧⌘< cycle Sawit's windows (open a second one with ⇧⌘N). In the Browser,
  ⇧⌘. toggles Annotate with the toolbar focused, and the keymap editor shows Annotate's key as ⌘:. Switch back to
  the US layout: Annotate's key is ⇧⌘. again. Answer:
- **K-3 ⌘E belongs to the keymap editor.** In the terminal, a file editor and a web page, press ⌘E: keymap.json
  doesn't open (the page gets its ⌘E). The Sawit menu's Open Keymap File shows no key; in the keymap editor ⌘E
  opens keymap.json. Answer:
- **K-4 The keystroke field's own keys.** In the keymap editor, Edit a binding: press ⎋, and the field stops
  recording; ⎋ again closes the form. Edit it again, press new keys and ↩: it saves. Turn on Search by Keystroke,
  click another tab, then press ⌘W: that tab closes (the hidden field doesn't record it). Answer:
- **K-5 A chord you add on ⌘K.** Rebind workspace: close tabs left to ⌘K L. On the canvas ⌘K opens the Actions
  palette at once; off the canvas ⌘K L closes the tabs on the left. Creating a binding on ⌘K alone in Workspace
  shows the warning that ⌘K also starts chords. Answer:
- **K-6 The title bar's pages with the tab area off.** Press ⇧⌘J to hide the tab area, then click the title bar's
  keyboard button, Tools and MCP in turn: each time the tab area shows with that page. Answer:
- **K-7 Hand edits and links.** With the keymap editor open, write keymap.json in another editor and save, then
  within a second change a binding in the keymap editor: it says the file changed and asks again, and your edit is
  kept. Make keymap.json a link to a file elsewhere (`ln -s`), change a binding in the editor: the link stays a
  link, and the file it points to has the change. Answer:
- **K-8 Keys once what you typed in goes off screen, and back.** Click in the terminal, then press ⌘J twice: the
  dock hides and comes back; type `ls`: it goes to the terminal. Click the title bar's Terminal button twice, then
  type: it goes to the terminal again. Click in a file editor's text, then press ⌃Tab twice: each press shows the next
  tab. Click another tab, then the file's tab again, and type: it goes to the text. Click in the Git panel's commit
  message, press ⌘2, then ⌘3: Files shows, then Git again; type: it goes to the message. Click in a web page, press
  ⇧⌘J twice: the tab area hides and comes back, and typing waits for a click in the page. With three tabs open,
  click in a file's text and press ⌃Tab three times: each press shows the next tab, and the last one brings the
  file back with its keyboard (type: it goes to the text). Click in the terminal and press ⌘J, ⌃Tab, ⌘J: the
  dock hides, the next tab shows, the dock comes back with the terminal's keyboard. In the Keymap Editor, turn
  on Search by Keystroke, click another tab, then the Keymap Editor's tab again, and press ⌃Tab: the next tab
  shows (the search field doesn't record until a click in it). Answer:

## Added by the hunt (2026-11-01)

The hunt's other lanes (browser, canvas core, canvas UI, catalog, files, MCP, terminal, site and smoke) wrote their
hand checks only into their own reports, which aren't in the repository. They are copied here, with the steps their
reviews added. No test can build a `Workspace` (P8-C8), so for the workspace wiring of those fixes these checks are
the only cover: the canvas taking the keyboard, a canvas file renamed under its tab, the hand-offs from Files, one
browser per project, where the quit question goes up, the center bar's scroll and the restore warning on screen.
- There are **44 checks**. F-1 to F-5 and H-MCP-n are the reports' own numbers (H-MCP-n after the finding MCP-n it
  checks); H-CORE-n and H-UI-n are canvas core's and canvas UI's H-n; the others are numbered here. H-WS-1 to
  H-WS-3 came with the workspace fixes made after the hunt's final review (smoke m3, final-seams m1 and m2).
- The fixers drove some of them over the control socket before their lanes merged. Nobody has run them by hand.
- Each starts from a fresh launch of the merged main, as under "Before you start", with start state A unless it
  says otherwise. A debug build is enough.
- Where a check asks an agent for a tool call, point the agent's Sawit MCP server at this build with the same
  `SAWIT_HOME`, as Plan 7's set-up and P8-10 say.
- A step that says "until … is fixed" checks a finding still open on main when this was written. Say what you see.
- A few checks touch your real files: the catalog's AI client configs, Config Files' shell files and the install
  script's Sawit.app. Each says so.

### Browser: 6 checks

Start: A, with the fixtures served as for PR #3 (`python3 -m http.server 8765 -d docs/fixtures/browser` from a
checkout of main), and `localhost:8765/annotate.html` open in the Browser.

**H-BR-1. A link to another app asks first** (BROWSER-2).
- Make a page with a mail link, serve it, and open `localhost:8766/mail.html`:

  ```bash
  mkdir -p "$S/web" && echo '<a href="mailto:me@example.com">Mail me</a>' > "$S/web/mail.html"
  python3 -m http.server 8766 -d "$S/web"
  ```
- Click the link. Look for: "Open this link in another app?", naming `mailto:me@example.com`. Open hands it to
  Mail, as a new message to that address.
- Click it again: the question also says "If you cancel, the page's links to other apps do nothing until you reload
  it or open another page." Choose Cancel, then click it once more. Look for: nothing opens and nothing asks. Click
  the toolbar's Reload: the link asks again (review-browser m5). Cancel, then open the page again from the address
  bar: it asks again too.

Answer: ______

**H-BR-2. Clear and close with a note not yet sent** (BROWSER-10).
- Turn on Annotate (⇧⌘.), pick the Submit order button and type a note.
- Click Clear. Look for: "Clear the annotations?"; Cancel keeps the note.
- Press ⌘W on the Browser tab. Look for: "Close the browser with its annotations?"; Cancel keeps it. The tab menu's
  Close Others and Close All leave it open, and the × on the title bar's project chip asks too.
- Click Copy, then Clear, then press ⌘W. Look for: neither asks.

Answer: ______

**H-BR-3. One Browser tab per project** (BROWSER-8).
- Open a second project in the window with the title bar's + (Open Another Project in This Window). Click Browser in
  the title bar in each project, and open `annotate.html` in one and `form.html` in the other.
- Look for: each project's group has its own Browser tab. Switch between the two, and from each to another tab and
  back: only the tab in front shows its page, and no page shows over another tab.
- Export from each (Design, then Export in System). Look for: each export goes to its own project's
  `.nyawit/design/`.

Answer: ______

**H-BR-4. Send to terminal goes to the agent** (BROWSER-12, and review-browser-r2 §4's composition check).
- Run `claude` in terminal tab 1. Open tab 2, leave it at its zsh prompt and in front. In the Annotate panel, add a
  note and click Send to terminal.
- Look for: the notes paste into Claude Code in tab 1, not into zsh, without Enter, and the footer names that
  terminal.
- Run `claude` in a third tab too. With tab 2 in front again, Send goes to the third tab, the last one running an
  agent; with tab 1 in front, to tab 1.
- Open a second project with the title bar's + and run `claude` in a terminal tab of its group, in front. Back in
  the first project's Browser, click Send. Look for: it goes to the first project's Claude Code, not the second's
  (review-browser m1).
- Connect to a saved SSH server from the SSH sidebar and leave its tab in front. Click Send. Look for: nothing is
  pasted at the server's prompt; the notes go to the project's Claude Code.

Answer: ______

**H-BR-5. A real pick** (BROWSER-4).
- Click the page, press ⇧⌘. and click the Submit order button with the mouse.
- Look for: the pick works as PR3-1 (a) says: an outline and a label while you hover, a numbered marker, and a new
  row that takes the keyboard. The picker now answers only real input (`isTrusted`), which your clicks and keys are.

Answer: ______

**H-BR-6. Quitting with a note not yet sent and a terminal running** (review-browser-r2 §4's composition check).
- With a note not yet sent and `sleep 600` running in a terminal, press ⌘Q.
- Look for: the question names only the terminal ("A terminal is still running: sleep. Quitting stops it."), not
  the notes. Cancel keeps both. The window's close button asks the same way. Quitting drops the notes, as the
  browser spec's D6 says, while closing the Browser tab or its project still asks about them (H-BR-2).

Answer: ______

### Canvas core: 5 checks

`$EX` below is start state B's example board:

```bash
EX=~/.cargo/git/checkouts/figcraft-bc55fa7002b5fc9c/0971d09/examples/ridgeline-app.figcraft
```

**H-CORE-1. A canvas file overwritten under its tab** (CORE-4).
- Copy `$EX` to `$S/proj/it.figcraft`. Open it from Files, draw a shape and close the tab, so that it is saved.
  Copy `$EX` to `$S/proj/other.figcraft` too.
- Open `it.figcraft` again. Overwrite it from a terminal (`cp "$S/proj/other.figcraft" "$S/proj/it.figcraft"`),
  then draw: a toast appears. Close the tab with ⌘W.
- Look for: Files shows `it (unsaved <date>).figcraft` beside the file, holding the drawing.
- Do it again, quitting with ⌘Q instead of ⌘W.

Answer: ______

**H-CORE-2. Two windows with no project** (CORE-5).
- Open two windows with no project (⇧⌘N, then Continue Without a Project) while the scratch home has no
  `canvas.figcraft` yet, and open the canvas (⌃⌘C) in both.
- Draw in the first, then in the second.
- Look for: the second toasts "a file was made on disk at its place…", and the first's drawing stays in the file.

Answer: ______

**H-CORE-3. An agent's op while the Pen draws** (CORE-8).
- Start: B. Pick the Pen and click twice on the canvas. Ask an agent for `canvas_add_elements`.
- Look for: the call is refused with the Pen sentence.
- Press Enter, and have the agent try again: it goes through. Press ⌘Z.
- Look for: ⌘Z removes only the agent's element.

Answer: ______

**H-CORE-4. A link a cloned repository holds beside the canvas file** (CORE-1).
- Make a repository that commits `victim.txt`, `.nyawit/canvas.figcraft.lossy.bak` as a link to `../victim.txt`,
  and a `.nyawit/canvas.figcraft` with a damaged image: one the canvas opens with notes, keeping the original as
  `.lossy.bak` (`file_with_a_damaged_image` in `crates/board/src/store.rs`'s tests builds one). Clone it, and open
  the clone's canvas.
- Look for: `victim.txt` is unchanged, and the clone's `.lossy.bak` is a file, not a link.

Answer: ______

**H-CORE-5. A pipe, then a device, named as a canvas file** (review-canvas-core I2). Run it in this order: a
regression then hangs the window on the pipe, rather than filling the Mac's memory from `/dev/zero`.
- With Sawit open on a project, run `mkfifo /tmp/p.figcraft`, and ask an agent to `canvas_load` it.
- Look for: it answers "replace: couldn't read … not a regular file" at once; the window stays responsive, and the
  board is unchanged.
- Only once the pipe is refused: run `ln -s /dev/zero /tmp/z.figcraft`, and ask for `canvas_load` of that. Look
  for: the same answer, at once.
- Then `rm /tmp/p.figcraft /tmp/z.figcraft`.

Answer: ______

### Canvas UI: 6 checks

Start: B. `$EX` is as for canvas core.

**H-UI-1. The canvas takes the keyboard** (CANVAS-UI-2).
- Click into a terminal pane, press ⌃⌘C, then R. Do the same from the title bar's Canvas button. Click a terminal,
  then the canvas tab, then press V.
- Look for: the Rectangle tool (then Move) is marked, and nothing is typed in the shell.
- With the canvas focused, press ⌃Tab twice. Open a `.figcraft` from the Files tree and press ⌫.
- Look for: ⌃Tab keeps cycling the tabs; ⌫ moves no file to the Trash.

Answer: ______

**H-UI-2. A renamed or trashed canvas file** (CANVAS-UI-3).
- Copy `$EX` to `$S/proj/x.figcraft`. Open it from Files, draw, then rename it in Files with F2 to `y.figcraft`.
  Draw again, then close and reopen `y.figcraft`.
- Look for: the tab's title follows the rename, `x.figcraft` doesn't come back, and `y.figcraft` has both drawings.
- Move the file to the Trash in Files, and draw. Look for: a toast says the file was saved again.

Answer: ______

**H-UI-3. Hidden in the middle of a drag** (CANVAS-UI-8).
- Start dragging a shape and, holding the button, press ⌃Tab. Release, come back to the canvas, and ask an agent
  for `canvas_add_elements`.
- Look for: the shape stays where it was left, and the agent's call goes through.

Answer: ______

**H-UI-4. The memory of hidden canvas tabs** (CANVAS-UI-9).
- Copy `$EX` to four `.figcraft` files, open each from Files, then switch to a tab that isn't a canvas.
- Look for: in Activity Monitor, Sawit's memory and GPU memory are tens of MB lower per hidden canvas than a build
  from before the hunt (`da0bf0a`) shows for the same steps. A hidden canvas now lets go of its kept render and its
  GPU texture (about 36 MB each at 2× on a 1200 × 800 canvas), and keeps only its last frame's image.

Answer: ______

**H-UI-5. `--wait` on a canvas file** (CANVAS-UI-7).
- From the recipe's shell, run `SAWIT_HOME=$S/home target/debug/sawit --wait "$S/proj/.nyawit/canvas.figcraft"`.
  The canvas tab shows, and the command waits. Close the tab.
- Do the same with an old board: `cp crates/board/tests/fixtures/excalidraw/basic.excalidraw "$S/proj/x.excalidraw"`
  from a checkout of main, then `--wait "$S/proj/x.excalidraw"`.
- Look for: each command returns when its canvas tab closes, and not before.

Answer: ______

**H-UI-6. A project opened through `/tmp`** (CANVAS-UI-4).
- `mkdir -p /tmp/sawit-alias`, then `SAWIT_HOME=$S/home target/debug/sawit /tmp/sawit-alias`. Sawit keeps that
  project as `/private/tmp/sawit-alias`. Open its canvas (⌃⌘C) and draw a shape, so that its file exists.
- Ask an agent for `app_open` of `/tmp/sawit-alias/.nyawit/canvas.figcraft`, or send
  `ctl '{"cmd":"open","path":"/tmp/sawit-alias/.nyawit/canvas.figcraft"}'`.
- Look for: that project's canvas tab comes forward, and no second canvas tab opens.

Answer: ______

### Catalog: 4 checks

These read and write your own AI clients' config files, whatever `SAWIT_HOME` says: back them up first.

**H-CAT-1. Claude Desktop, with Node.js only from nvm** (CATALOG-4, CATALOG-6).
- On a Mac, or a user account, whose Node.js comes only from nvm, install a remote server (Cloudflare, say) for
  Claude Desktop from the MCP page.
- Look for: Claude Desktop's config gets the server's `npx` bridge entry, with an `env.PATH` that starts with nvm's
  Node.js folder. Restart Claude Desktop: the server starts, and its tools are listed.

Answer: ______

**H-CAT-2. An update through `claude mcp add` keeps your `env`** (CATALOG-1).
- Install a server for Claude Code from the MCP page. In `~/.claude.json`, add `"env": {"MY_KEY": "x"}` to its
  entry, and change its `url` or `command` so that the page offers an update. Update it from the MCP page.
- Look for: the entry is the catalog's again, with your `env` back.

Answer: ______

**H-CAT-3. fish as the login shell** (CATALOG-2).
- With fish installed, put `SHELL=$(command -v fish)` in front of the recipe's launch line, and open Tools.
- Look for: the tools you have are found, with their versions, as under zsh. If detection fails, a warning row
  names the shell, and Install Missing is off.

Answer: ______

**H-CAT-4. Closing the window, or quitting, while Dev Tools installs** (final seams m4).
- Open Tools and start an install or a Reinstall that takes a few seconds (Bun's, say). While it runs, click the
  window's close button.
- Look for: "Close this window?" with "Dev Tools is still running: Installing Bun. Closing the window stops it."
  Cancel: the window stays, and the install goes on.
- Start another one, and press ⌘Q.
- Look for: "Quit Sawit?" says the same. Quit: Sawit quits, and `pgrep -fl 'bun.sh/install'` lists nothing, so the
  installer didn't run on alone. Stopped partway, the tool may need installing again.

Answer: ______

### Files: 5 checks

Start: A, with `a.rs`, `b.rs` and a folder of many files (a copy of a `node_modules`, say) in `$S/proj`.

**F-1. Reopen, then type** (FILES-1).
- Open `a.rs` and `b.rs` from the tree, click `a.rs` in the tree again, then type, and press ⌫ and ⌘Z.
- Look for: they edit `a.rs`; nothing goes to the Trash, and no file change is undone.
- Click a folder row: it is outlined. Click into the editor: the outline goes.

Answer: ______

**F-2. ⌫ and ⌘⌫ in Files and Config Files** (FILES-1, FILES-2).
- In Files, ⌫ on a file asks; Cancel keeps it. ⌘⌫ moves it to the Trash at once; ⌘Z brings it back; ⇧⌘Z moves it
  to the Trash again, and Finder's Put Back still works. Its open tab says it was deleted.
- In Config Files, ⌫ asks the same way. These are your real files: if you try ⌘⌫ there, Finder's Put Back brings
  the file back (the panel has no ⌘Z).

Answer: ______

**F-3. A big Duplicate, then ⌘Z** (FILES-5).
- Rename a file. Then press ⌘D on the folder of many files, and ⌘Z at once.
- Look for: the header's spinner while it copies, then the copy goes to the Trash; the rename is untouched.

Answer: ______

**F-4. Open Project Folder** (FILES-9, and review-files M1).
- With projects A and B in one window (the title bar's + adds B) and A shown, pick B with the Files tab's folder
  icon.
- Look for: B is shown, and there's no third project. Pick a folder open in another window: that window comes
  forward.
- With A shown, click the folder icon and, while the picker is open, click a tab of B's group. Then pick a new
  folder C.
- Look for: A is shown again with C as its folder, and A's tabs lose their group. B keeps its folder and tabs.
  Before review-files M1 was fixed, B's folder became C instead.

Answer: ______

**F-5. Initialize Repository** (FILES-15).
- In a folder that isn't a repository, click Initialize Repository in the Git tab, then right-click a file in
  Files.
- Look for: Add to .gitignore and View History are enabled.

Answer: ______

### MCP: 6 checks

**H-MCP-1. Two agents on one closed canvas** (MCP-1).
- Quit Sawit. Run two Claude Code sessions in one project, with the scratch `SAWIT_HOME` so that they find no
  Sawit running, and ask each to add ten stickies to the canvas.
- Look for: no error answered, and all twenty on the canvas when Sawit opens the project.

Answer: ______

**H-MCP-2. A project opened through a linked folder** (MCP-2, and review-mcp M1).
- Run `mkdir -p "$S/code/shop" && ln -s "$S/code" "$S/code-link"`. With the Open Folder dialog, open
  `$S/code-link/shop`. Look for: the project is `shop`, and `app_state` and the status bar's Copy Path give it as
  the folder on disk, `$S/code/shop` (with `/private` in front if `$S` is under `/tmp`).
- From an agent, call `canvas_get {project: "$S/code-link/shop"}`, the other spelling. Look for: it answers from
  that window.
- From the agent, call `app_open` of `$S/code/shop`, the folder as it is on disk, which is what an agent's working
  folder is. Look for: that project shows, and no second project is added for it.
- From the agent, call `save_command {project_dir: "$S/code/shop", name: "hello", commands: ["echo hi"]}`. Look
  for: the Run panel lists hello under that project.
- Open the welcome screen in a new window (⇧⌘N): Recent Projects lists the folder once.

Answer: ______

**H-MCP-4. A relative `cwd` runs nothing** (MCP-4).
- Start Sawit with `/` as its working folder: the installed Sawit.app from the Dock, or `cd /` before the recipe's
  launch line. Have an agent call `terminal_run {cwd: "web", command: "pwd"}`.
- Look for: "Nothing was run", and no shell opens in `/`.

Answer: ______

The next three need a second project in the window, `proj2`, shown in front of `proj`:

```bash
mkdir -p "$S/proj2" && touch "$S/proj/f01.txt" "$S/proj2/g01.txt"
ctl "{\"cmd\":\"open\",\"path\":\"$S/proj2\"}"
ctl "{\"cmd\":\"open\",\"path\":\"$S/proj/f01.txt\"}"
ctl "{\"cmd\":\"open\",\"path\":\"$S/proj2/g01.txt\"}"
```

**H-WS-1. An agent's tab switch brings the tab's project** (smoke m3).
- Click into the terminal (⌘J shows it). Run `ctl '{"cmd":"activate_tab","tab":"f01.txt"}'`.
- Look for: `f01.txt` is shown, with proj's Files tree in the sidebar and proj's chip active in the title bar, and
  `ctl '{"cmd":"state"}'` names proj as `project`. Typing still goes to the terminal.
- Click `g01.txt`'s tab, fold proj's group with a click on its header, and run the line again. Look for: the group
  unfolds, with `f01.txt` active.

Answer: ______

**H-WS-2. The Keymap Editor an agent opens leaves your keyboard** (final-seams m2).
- Open the Keymap Editor (⌘K ⌘S) and click a binding to select it. Click into the terminal and type `echo hi`,
  without Enter.
- Run `ctl '{"cmd":"open_page","page":"keymap"}'`, which is an agent's `app_open_page keymap`, then press ⌫ twice.
- Look for: the Keymap Editor comes forward, and the ⌫s erase `hi` in the terminal. The binding you selected is
  still there, and `$S/home/keymap.json` wasn't written.
- Close the Keymap Editor's tab and do it again. Look for: it opens, and the keys still go to the terminal.

Answer: ______

**H-WS-3. An agent's browser is its own project's** (final-seams m1).
- Serve the browser fixtures as for the Browser checks (`python3 -m http.server 8765 -d docs/fixtures/browser`),
  with proj2 shown. Run what an agent working in `$S/proj` sends when it names no project:

  ```bash
  ctl "{\"agent_folder\":\"$S/proj\",\"cmd\":\"browser\",\"op\":{\"op\":\"navigate\",\"url\":\"http://localhost:8765/form.html\"}}"
  ```
- Look for: proj comes forward with a Browser tab of its own, in proj's group, showing `form.html`. proj2 has
  no Browser tab.
- Click `g01.txt`'s tab, then run the same line with `{\"op\":\"state\"}` as the op. Look for: the answer's
  `url` is `form.html`, and nothing on screen changes: proj2 and `g01.txt` stay in front.
- Run `ctl "{\"agent_folder\":\"$S/proj\",\"cmd\":\"open_page\",\"page\":\"browser\"}"`. Look for: proj's
  Browser tab comes forward, and no second one opens.
- Before final-seams m1 was fixed, the page opened in proj2's browser, with proj2's cookies, and the `state` op
  answered from proj2's.

Answer: ______

### Terminal: 5 checks

Start: A. Open a second window, where a check needs one, with ⇧⌘N.

**H-TERM-1. The quit question comes forward with its window** (review-terminal I2).
- In a second window, run `sleep 600` in the terminal. Click the first window, whose terminals are idle, and press
  ⌘Q.
- Look for: the second window comes forward with "Quit Sawit?" ("A terminal is still running: sleep. Quitting
  stops it."), so ⌘Q never looks dead.
- Cancel, minimise the second window (⌘M), and press ⌘Q in the first: the second comes back with the question.
  Move the second window to another Space and do it again: Sawit shows that Space with the question.

Answer: ______

**H-TERM-2. Return, Esc and the close button** (TERMINAL-6).
- With `sleep 600` running, press ⌘Q, then Esc. Look for: the question goes, and Sawit and `sleep` keep running.
- Press ⌘Q, then Return. Look for: Sawit quits.
- Relaunch, run `sleep 600` again, and click the window's close button. Look for: "Close this window?", naming
  `sleep`, with Close Window and Cancel.

Answer: ______

**H-TERM-3. The Dock's Quit doesn't ask** (TERMINAL-6's known limit).
- With `sleep 600` running, quit from the Dock (right-click Sawit's icon, then Quit).
- Look for: Sawit quits without asking, and `sleep` ends with it. gpui-pre doesn't pass
  `applicationShouldTerminate` on to the app, so the Dock's Quit and logging out can't ask. Logging out, if you try
  it, does the same. Say whether that's acceptable.

Answer: ______

**H-TERM-4. A file dropped on a pane without the keyboard** (TERMINAL-7).
- Split a tab (⌘D) and keep the keyboard in the right pane. Drag a file from Finder onto the left pane.
- Look for: the file's path is typed into the left pane, which takes the keyboard and the tab's focus (its border
  has the accent colour). ⌘D now splits the left pane.

Answer: ______

**H-TERM-5. An agent's command while you type at a prompt** (final-seams I3).
- Click in a terminal at its zsh prompt, type `echo mine`, and don't press Enter. Then, from the recipe's shell,
  send `ctl '{"cmd":"terminal_run","command":"date"}'`, or ask an agent in another tab to wait ten seconds and
  then `terminal_run` `date`, and go back to the prompt.
- Look for: `date` runs in another idle shell, or in a new tab, in the background; your line still reads
  `echo mine`, and Enter runs it as typed.
- Click a file's text, so that the terminal doesn't have the keyboard, and send it again. Look for: `date` runs in
  the terminal in front, which shows without taking the keyboard.

Answer: ______

### Site lane: unsaved files on quit, the install script and the DMG: 6 checks

Start: A, with `echo 'on disk' > "$S/proj/notes.txt"`. "An unsaved file" below is `notes.txt`, opened from Files
and typed into without saving.

**H-SITE-1. ⌘Q and Quit Sawit with an unsaved file** (SITE-1).
- Press ⌘Q. Look for: "Save changes to notes.txt before quitting?", with Save, Don't Save and Cancel. Cancel keeps
  the app. Save writes the file and quits.
- Relaunch, type again, press ⌘Q and choose Don't Save. Look for: Sawit quits, the file keeps its saved text, and
  the next launch doesn't bring the changes back.
- Do the same from the Sawit menu's Quit Sawit. With two unsaved files the question says "2 files", names them,
  and offers Save All. Once an update waits, Restart to Update asks "… before restarting?" the same way.

Answer: ______

**H-SITE-2. A window's close button** (SITE-1).
- With an unsaved file in each of two windows, click one window's close button.
- Look for: it asks about that window's file only ("… before closing this window?").

Answer: ______

**H-SITE-3. The Dock's Quit keeps unsaved changes** (SITE-1).
- With an unsaved file, quit from the Dock (right-click, then Quit). Look for: no question.
- Relaunch. Look for: the file reopens with your changes, unsaved, and the note "Unsaved changes Sawit kept when it
  quit are back. Save them, or close the tab to drop them."; the file on disk is unchanged.
- Logging out, if you try it, does the same.

Answer: ______

**H-SITE-4. The restore warning on screen** (review-site I1).
- With an unsaved file, quit from the Dock. Change the file from Terminal.app (`echo pulled >> "$S/proj/notes.txt"`),
  then relaunch.
- Look for: a red note under the toolbar: "Unsaved changes Sawit kept when it quit are back, but this file changed
  on disk after they were made. Press ⌘S to overwrite the version on disk, or close the tab to drop them." It is
  still there a few seconds later: the disk watcher doesn't clear it.
- Press ⌘S. Look for: it overwrites, with no second question.

Answer: ______

**H-SITE-5. The install script while Sawit has an unsaved file** (SITE-1, SITE-18).
- This replaces your installed Sawit.app, so use the installed app, not the scratch build. With an unsaved file in
  it, run README's install command in Terminal.app. Not in Sawit's own terminal: the script ends with Sawit there.
- Look for: Sawit quits without asking, the script waits until it has quit, then installs. The next launch brings
  the unsaved changes back, as in H-SITE-3.

Answer: ______

**H-SITE-6. The DMG on macOS 15 or 26** (SITE-3).
- After the release, download `Sawit.dmg` in Safari, drag Sawit to Applications and open it: macOS refuses. Choose
  Done, then Open Anyway in System Settings ▸ Privacy & Security.
- Look for: Sawit opens, and the site's DMG card and README say what you saw.

Answer: ______

### Smoke: 1 check

**H-SMOKE-1. The center bar scrolls to the tab you open or switch to** (SMOKE-1).
- In a window about 1280 × 800, open sixteen files from the recipe's shell:

  ```bash
  for i in $(seq -w 1 16); do touch "$S/proj/f$i.txt"; ctl "{\"cmd\":\"open\",\"path\":\"$S/proj/f$i.txt\"}"; done
  ```
- Look for: the bar ends at the active `f16.txt`, fully shown with its × and highlighted.
- Click `f03.txt` in the Files tree. Look for: its tab comes in flush at the left edge, highlighted.
- Run `mkdir -p "$S/proj2" && touch "$S/proj2/g01.txt"`, then `ctl "{\"cmd\":\"open\",\"path\":\"$S/proj2\"}"` and
  `ctl "{\"cmd\":\"open\",\"path\":\"$S/proj2/g01.txt\"}"`.
- Look for: the bar ends with proj2's header and the active `g01.txt` in proj2's colour, both fully shown.

Answer: ______
