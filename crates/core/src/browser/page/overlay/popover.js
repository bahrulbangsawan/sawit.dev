  // ---- The popover ----

  const showKind = () => {
    for (let i = 0; i < KINDS.length; i++) {
      attr(ui.kinds[i], "aria-checked", pop.intent === KINDS[i] ? "true" : "false");
      attr(ui.kinds[i], "tabindex", pop.intent === KINDS[i] ? "0" : "-1");
    }
  };
  const fill = () => {
    write(ui.badge, pop.n === null ? "New" : String(pop.n));
    write(ui.title, short(pop.el));
    // The whole label, where the line cuts it.
    attr(ui.title, "title", short(pop.el));
    write(ui.sub, summary(pop.el));
    attr(ui.walk, "hidden", pop.n === null ? null : "");
    attr(ui.remove, "hidden", pop.n === null ? "" : null);
    attr(ui.save, "aria-label", pop.n === null ? "Save the annotation" : "Save the changes");
    if (ui.up) {
      const parent = parentOf(pop.el);
      if (parent && parent !== document.documentElement) attr(ui.up, "disabled", null);
      else attr(ui.up, "disabled", "");
      if (pop.down.length) attr(ui.down, "disabled", null);
      else attr(ui.down, "disabled", "");
    }
    showKind();
  };
  const open = (el, n, intent, comment) => {
    if (!ensureHost() || !ui || !ui.pop) return false;
    pop = { n, el, intent: KINDS.includes(intent) ? intent : "fix", down: [] };
    if (n === null) current = el;
    fill();
    apply(N.setValue, ui.text, [comment || ""]);
    attr(ui.pop, "hidden", null);
    attr(ui.pop, "class", "pop in");
    syncMarkers();
    watch();
    render();
    focusOn(ui.text);
    return true;
  };
  const close = () => {
    if (!pop) return;
    pop = null;
    if (ui) {
      attr(ui.pop, "hidden", "");
      attr(ui.pop, "class", "pop");
      if (N.blur && ui.text) apply(N.blur, ui.text, []);
    }
    const back = returnTo;
    returnTo = null;
    if (back && read(N.connected, back, "isConnected")) focusOn(back);
    watch();
    schedule();
  };
  const commentOf = () => (ui && ui.text ? apply(N.value, ui.text, []) : "");
  const save = () => {
    if (!pop) return;
    const comment = commentOf();
    if (pop.n === null) {
      // A new note goes to Sawit only while it is picking for Annotate.
      if (mode === "annotate" && pop.el.isConnected) send("picked", { mode, element: details(pop.el), comment, intent: pop.intent });
    } else {
      send("annotation", { action: "update", n: pop.n, comment, intent: pop.intent });
      for (let i = 0; i < items.length; i++) {
        if (items[i].n === pop.n) {
          items[i].comment = comment;
          items[i].intent = pop.intent;
        }
      }
      syncMarkers();
    }
    close();
  };
  const removeItem = (n) => {
    send("annotation", { action: "remove", n });
    const left = [];
    for (let i = 0; i < items.length; i++) if (items[i].n !== n) left[left.length] = items[i];
    items = left;
    if (pop && pop.n === n) {
      returnTo = null;
      close();
    }
    stale = true;
    syncMarkers();
    watch();
    schedule();
  };
  const retarget = (toParent) => {
    if (!pop || pop.n !== null) return;
    if (toParent) {
      const parent = parentOf(pop.el);
      if (!parent || parent === document.documentElement) return;
      pop.down.push(pop.el);
      pop.el = parent;
    } else if (pop.down.length) {
      pop.el = pop.down.pop();
    } else return;
    current = pop.el;
    fill();
    render();
  };
  const openMarker = (m) => {
    let el = null;
    try {
      el = resolve({ selector: m.item.selector });
    } catch (_) {}
    if (!el) return;
    returnTo = m.node;
    open(el, m.item.n, m.item.intent, m.item.comment);
  };
  const act = (what) => {
    if (what === "save") save();
    else if (what === "cancel") close();
    else if (what === "remove" && pop && pop.n !== null) removeItem(pop.n);
    else if (what === "parent") retarget(true);
    else if (what === "child") retarget(false);
    else if (what.startsWith("kind:") && pop) {
      pop.intent = what.slice(5);
      showKind();
    }
  };
