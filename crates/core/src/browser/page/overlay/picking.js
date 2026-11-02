  // ---- Picking ----

  // Whether the last `hit` found mesurer's host on top: mesurer has the pointer, so the picker
  // draws no hover box and picks nothing there.
  let yielded = false;
  const hit = (x, y) => {
    let el = null;
    yielded = false;
    for (const candidate of document.elementsFromPoint(x, y)) {
      if (candidate === host) continue;
      if (isMesurer(candidate)) {
        yielded = true;
        return null;
      }
      if (!candidate.hasAttribute("data-sawit-overlay")) {
        el = candidate;
        break;
      }
    }
    while (el && el.shadowRoot) {
      const inner = el.shadowRoot.elementsFromPoint(x, y).find((c) => c !== el && c !== host);
      if (!inner) break;
      el = inner;
    }
    return el;
  };
  const hover = () => {
    hoverTimer = 0;
    lastHover = Date.now();
    if (mode === "inspect" && current) send("hovered", { selector: selectorFor(current), label: describe(current), rect: rectOf(current) });
  };
  const set = (el) => {
    current = el;
    schedule();
    if (mode !== "inspect" || !el || hoverTimer) return;
    const wait = 100 - (Date.now() - lastHover);
    if (wait <= 0) hover();
    else hoverTimer = setTimeout(hover, wait);
  };
  const move = (e) => {
    if (e.altKey !== altHeld) {
      altHeld = e.altKey;
      schedule();
    }
    // The popover's element stays picked while it is open.
    if (pop) return;
    const el = hit(e.clientX, e.clientY);
    if (yielded) {
      if (current) {
        down = [];
        set(null);
      }
      return;
    }
    if (el && el !== current) {
      down = [];
      set(el);
    }
  };
  // A press, click or wheel on mesurer (its toolbar, a panel, or a tool's overlay over the page;
  // as seen at the window, the event's target is mesurer's host) is mesurer's: left untouched.
  const onMesurer = (e) => isMesurer(read(N.target, e, "target"));
  const swallow = (e) => {
    if (onMesurer(e)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
  };
  const up = () => {
    if (!current) return;
    const parent = parentOf(current);
    if (parent && parent !== document.documentElement) {
      down.push(current);
      set(parent);
    }
  };
  const back = () => {
    if (down.length) set(down.pop());
  };
  const click = (e) => {
    if (onMesurer(e)) return;
    swallow(e);
    // The press that closed the popover picks nothing.
    if (dismissed) {
      dismissed = false;
      return;
    }
    if (pop) return;
    const h = hit(e.clientX, e.clientY);
    let el = current && h && current.contains(h) ? current : h || current;
    if (!el) return;
    if (mode === "inspect") {
      pinned = el;
      send("picked", { mode, element: details(el) });
      return;
    }
    // ⌥-click picks the parent of what is under the pointer.
    if (e.altKey) {
      const parent = parentOf(el);
      if (parent && parent !== document.documentElement) el = parent;
    }
    // The popover asks for the comment on the page; before the overlay can be drawn (the
    // page's first second), the pick goes to the panel, whose row asks for it.
    if (!open(el, null, "fix", "")) send("picked", { mode, element: details(el) });
  };
  const altKey = (e) => {
    if (e.key === "Alt" && altHeld !== (e.type === "keydown")) {
      altHeld = e.type === "keydown";
      schedule();
    }
  };
  // ---- Keys beside mesurer ----
  //
  // While mesurer's host is on the page (`data-sawit-mesurer`, Annotate's measuring tools), Esc
  // unwinds from the innermost layer out: Sawit's popover first (its guard runs before anything),
  // then whatever mesurer has open (a menu, a tool, a selection), then mesurer itself (it
  // minimizes), and only then Sawit's picking. So the picker doesn't swallow Esc there: once the
  // event has gone through the page (a task later), it stops picking only if no one took the key
  // (`defaultPrevented`, read through the native getter: mesurer, and its keyboard gate while
  // mesurer has the keyboard, prevent it). The walk keys (↑ ↓ [ ]) are Sawit's only while it has
  // the pointer (an element under it); over mesurer they are mesurer's. A page can put up a host
  // of its own with that mark: Esc is then left to it as well, which only keeps the picker on.
  const PREVENTED = getter(EV, "defaultPrevented");
  const BY_ID = method(DOC, "getElementById");
  const mesurerShown = () => {
    try {
      const id = "mesurer-extension-host";
      const found = BY_ID ? apply(BY_ID, document, [id]) : typeof document.getElementById === "function" ? document.getElementById(id) : null;
      return isMesurer(found);
    } catch {
      return false;
    }
  };
  // Esc typed into one of mesurer's own fields (its comment search, a text note) is the field's:
  // mesurer leaves it there, so Sawit keeps picking.
  const ROOT_OF = getter(ELEM, "shadowRoot");
  const TAG = getter(ELEM, "tagName");
  const EDITABLE = getter(HTML, "isContentEditable");
  const typingInMesurer = (e) => {
    try {
      const target = read(N.target, e, "target");
      if (!isMesurer(target)) return false;
      const root = ROOT_OF ? apply(ROOT_OF, target, []) : target.shadowRoot;
      const a = root ? (N.active ? apply(N.active, root, []) : root.activeElement) : null;
      if (!a) return false;
      const tag = TAG ? apply(TAG, a, []) : a.tagName;
      return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || (EDITABLE ? apply(EDITABLE, a, []) : a.isContentEditable) === true;
    } catch {
      return false;
    }
  };
  const escapeAfter = (e) => {
    setTimeout(() => {
      if (read(PREVENTED, e, "defaultPrevented") || mode === "off") return;
      send("key", "escape");
      stopPicking();
    }, 0);
  };
  const keydown = (e) => {
    altKey(e);
    if (pop) return;
    const beside = mesurerShown();
    if (e.key === "Escape") {
      if (beside) {
        if (!typingInMesurer(e)) escapeAfter(e);
        return;
      }
      swallow(e);
      send("key", "escape");
      stopPicking();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "[" || e.key === "]") {
      if (beside && !current) return;
      swallow(e);
      if (e.key === "ArrowUp" || e.key === "[") up();
      else back();
    }
  };
  const wheel = (e) => {
    if (!e.altKey || onMesurer(e)) return;
    swallow(e);
    if (e.deltaY < 0) up();
    else if (e.deltaY > 0) back();
  };
  const SWALLOWED = ["pointerdown", "pointerup", "mousedown", "mouseup", "auxclick", "contextmenu"];
  // Only the user's own input picks: a page can arm the picker (`__sawitAgent` is on the
  // page) and dispatch events, and those, like an agent's clicks, go to the page untouched.
  // `isTrusted` holds only while no page script has the listener itself (see `listen`), nor
  // the raw `fn` behind it: `click` reads `e.clientX`, a getter a page can replace.
  const user = (fn) => (e) => {
    if (e.isTrusted) fn(e);
  };
  // Built now, before any page script runs; read only by index from here on.
  const LISTENERS = [
    ["pointermove", move],
    ["mousemove", move],
    ...SWALLOWED.map((type) => [type, swallow]),
    ["click", click],
    ["keydown", keydown],
    ["keyup", altKey],
    ["wheel", wheel],
  ].map(([type, fn]) => [type, user(fn)]);
  const stopPicking = () => {
    if (mode === "off") return;
    for (let i = 0; i < LISTENERS.length; i++) unlisten(LISTENERS[i][0], LISTENERS[i][1], CAPTURE);
    mode = "off";
    current = null;
    pinned = null;
    altHeld = false;
    down = [];
    dismissed = false;
    clearTimeout(hoverTimer);
    hoverTimer = 0;
    // A new note can't be saved once picking stops; an open one can.
    if (pop && pop.n === null) close();
    applyMode();
    watch();
    render();
  };

  // Annotate's key (⇧⌘. unless the keymap moves it) is never the page's to answer. The
  // user's own reaches Sawit's keymap first, wherever it is bound (GPUI sees ⌘ keys before
  // the page does), so a key the keymap doesn't bind to Annotate doesn't toggle it. A key
  // the page dispatches mustn't open Annotate and arm the picker, and Sawit answers an
  // agent's browser_press_key "Meta+Shift+." itself.
