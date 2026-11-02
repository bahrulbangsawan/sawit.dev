  // ---- The overlay's own events, kept from the page ----
  //
  // Events on the overlay reach the page with the host as their target. Each is stopped at
  // the window, before any page listener (this one was added first), and handled here: the
  // page never sees a key typed into the comment, nor a press on a marker. Stopping it there
  // leaves its default alone (the field still types, takes the caret and focus). Events a
  // page dispatches at the host are dropped.

  const ownAt = (x, y) => {
    for (let i = 0; i < actions.length; i++) {
      const node = actions[i][0];
      if (!node) continue;
      const b = boxOf(node);
      if (b.width && x >= b.left && x < b.right && y >= b.top && y < b.bottom) return actions[i][1];
    }
    for (let i = 0; i < marks.length; i++) {
      const b = boxOf(marks[i].node);
      if (b.width && x >= b.left && x < b.right && y >= b.top && y < b.bottom) return marks[i];
    }
    return null;
  };
  const ownOf = (node) => {
    for (let i = 0; i < actions.length; i++) if (actions[i][0] === node) return actions[i][1];
    return markerAction(node);
  };
  const activeOwn = () => (N.active && shadow ? apply(N.active, shadow, []) : null);
  const run = (what) => {
    if (!what) return;
    if (typeof what === "string") {
      // A button inside a popover that has closed meanwhile does nothing.
      if (pop) act(what);
    } else openMarker(what);
  };
  const ownClick = (e) => {
    // A click from the keyboard (Enter or Space on a button) has no point.
    const keyed = read(N.detail, e, "detail") === 0;
    run(keyed ? ownOf(activeOwn()) : ownAt(read(N.x, e, "clientX"), read(N.y, e, "clientY")));
  };
  const ownKey = (e) => {
    const key = read(N.key, e, "key");
    const active = activeOwn();
    if (pop) {
      const meta = read(N.meta, e, "metaKey") || read(N.ctrl, e, "ctrlKey");
      if (key === "Escape") {
        preventNow(e);
        close();
      } else if (key === "Enter" && active === ui.text && !read(N.composing, e, "isComposing") && (meta || !read(N.shift, e, "shiftKey"))) {
        preventNow(e);
        save();
      } else if ((key === "ArrowUp" || key === "ArrowDown") && read(N.keyAlt, e, "altKey")) {
        preventNow(e);
        retarget(key === "ArrowUp");
      } else if ((key === "ArrowLeft" || key === "ArrowRight") && ownOf(active) && String(ownOf(active)).startsWith("kind:")) {
        preventNow(e);
        const at = KINDS.indexOf(pop.intent);
        const next = (at + (key === "ArrowRight" ? 1 : KINDS.length - 1)) % KINDS.length;
        pop.intent = KINDS[next];
        showKind();
        focusOn(ui.kinds[next]);
      } else if (key === "Tab") {
        // The keyboard stays in the popover until it closes.
        const order = [];
        if (pop.n === null) {
          order[order.length] = ui.up;
          order[order.length] = ui.down;
        }
        order[order.length] = ui.kinds[KINDS.indexOf(pop.intent)];
        order[order.length] = ui.text;
        if (pop.n !== null) order[order.length] = ui.remove;
        order[order.length] = ui.cancel;
        order[order.length] = ui.save;
        let at = -1;
        for (let i = 0; i < order.length; i++) if (order[i] === active) at = i;
        preventNow(e);
        const step = read(N.shift, e, "shiftKey") ? order.length - 1 : 1;
        focusOn(order[(at + step + order.length) % order.length]);
      }
      return;
    }
    const m = markerAction(active);
    if (m && (key === "Backspace" || key === "Delete")) {
      preventNow(e);
      removeItem(m.item.n);
    } else if (key === "Escape") {
      // The keyboard left on a marker (after its note closed): Esc gives it back to the page
      // and, as anywhere else, stops picking. Beside mesurer (picking.js) the marker is the
      // innermost layer: Esc leaves it, and the next one unwinds mesurer's before picking stops.
      preventNow(e);
      if (active && N.blur) apply(N.blur, active, []);
      if (mode !== "off" && !mesurerShown()) {
        send("key", "escape");
        stopPicking();
      }
    }
  };
  const grow = () => {
    if (!ui || !ui.text || !N.scrollHeight) return;
    prop(ui.text, "height", "auto");
    prop(ui.text, "height", Math.min(apply(N.scrollHeight, ui.text, []) + 2, 160) + "px");
  };
  const own = (e) => {
    const type = read(N.type, e, "type");
    if (type === "click") ownClick(e);
    else if (type === "keydown") ownKey(e);
    else if (type === "input") grow();
  };
  // With the popover open, a press elsewhere closes it, and Esc does wherever the keyboard is.
  const outside = (e) => {
    const type = read(N.type, e, "type");
    if (type === "pointerdown" || type === "mousedown") {
      if (type === "pointerdown") {
        returnTo = null;
        close();
        dismissed = mode !== "off";
      }
    } else if (type === "keydown" && read(N.key, e, "key") === "Escape") {
      stopNow(e);
      preventNow(e);
      close();
    }
  };
  const OWN_EVENTS = [
    "keydown", "keyup", "keypress", "beforeinput", "input", "textInput", "compositionstart", "compositionupdate",
    "compositionend", "focusin", "focusout", "focus", "blur", "pointerdown", "pointerup", "pointermove", "pointerover",
    "pointerout", "pointerenter", "pointerleave", "pointercancel", "gotpointercapture", "lostpointercapture",
    "mousedown", "mouseup", "mousemove", "mouseover", "mouseout", "mouseenter", "mouseleave", "click", "dblclick",
    "auxclick", "contextmenu", "copy", "cut", "paste", "select", "selectstart", "dragstart", "drop",
  ];
  const guard = (e) => {
    if (host && read(N.target, e, "target") === host) {
      stopNow(e);
      if (e.isTrusted) own(e);
      return;
    }
    if (!e.isTrusted) return;
    if (pop) outside(e);
    else if (dismissed && read(N.type, e, "type") === "pointerdown") dismissed = false;
  };
  for (let i = 0; i < OWN_EVENTS.length; i++) listen(OWN_EVENTS[i], guard, CAPTURE);
  listen("wheel", guard, SCROLLS);
