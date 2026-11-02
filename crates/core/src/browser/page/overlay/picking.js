  // ---- Picking ----

  const hit = (x, y) => {
    let el = null;
    for (const candidate of document.elementsFromPoint(x, y)) {
      if (candidate !== host && !candidate.hasAttribute("data-sawit-overlay")) {
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
    if (el && el !== current) {
      down = [];
      set(el);
    }
  };
  const swallow = (e) => {
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
  const keydown = (e) => {
    altKey(e);
    if (pop) return;
    if (e.key === "Escape") {
      swallow(e);
      send("key", "escape");
      stopPicking();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "[" || e.key === "]") {
      swallow(e);
      if (e.key === "ArrowUp" || e.key === "[") up();
      else back();
    }
  };
  const wheel = (e) => {
    if (!e.altKey) return;
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
