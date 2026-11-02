  // ---- Ops ----

  // Hides the overlay for a screenshot (`hidden`), all of it, or (`agents`) all but the agents'
  // cursors, which the user can have in agents' screenshots.
  const overlay = (op) => {
    hidden = !!op.hidden;
    keepAgents = hidden && !!op.agents;
    applyMode();
    return { hidden };
  };
  const pick = (op) => {
    if (op.mode !== "annotate" && op.mode !== "inspect" && op.mode !== "off") throw new Error("Pick mode is annotate, inspect or off.");
    if (op.mode === "off") {
      stopPicking();
    } else if (mode === "off") {
      mode = op.mode;
      for (let i = 0; i < LISTENERS.length; i++) listen(LISTENERS[i][0], LISTENERS[i][1], CAPTURE);
      applyMode();
      watch();
    } else {
      mode = op.mode;
    }
    if (mode !== "inspect") pinned = null;
    if (mode !== "annotate" && pop && pop.n === null) close();
    return { mode };
  };
  // The page's markers. A page can call this op too (`__sawitAgent` is on the page); only
  // Sawit's call, signed with the token no page script can read, carries the notes' comments,
  // so a page can't put words of its own in a popover the user then saves as theirs.
  const markers = (op) => {
    const sawit = op.token === TOKEN;
    items = [];
    const placed = [];
    const missing = [];
    for (const item of op.items || []) {
      try {
        resolve({ selector: item.selector });
        items.push({
          n: item.n,
          selector: item.selector,
          comment: sawit && typeof item.comment === "string" ? item.comment : "",
          intent: sawit && typeof item.intent === "string" ? item.intent : "fix",
          label: typeof item.label === "string" ? item.label : "",
        });
        placed.push(item.n);
      } catch (_) {
        missing.push(item.n);
      }
    }
    // An open note whose row went away closes with it.
    if (pop && pop.n !== null && !placed.includes(pop.n)) {
      returnTo = null;
      close();
    }
    stale = true;
    syncMarkers();
    watch();
    render();
    return { placed, missing };
  };
  // Sawit's colours for the overlay: `{colors: {bg, fg, muted, border, field, accent, primary,
  // primaryFg, danger, ring}}`, each a CSS colour.
  const themeOp = (op) => {
    const colors = op.colors || {};
    const names = Object.keys(theme);
    for (let i = 0; i < names.length; i++) {
      const v = colors[names[i]];
      if (typeof v !== "string") continue;
      const font = names[i] === "font" || names[i] === "mono";
      if (font ? /^[\w\s"',.-]{1,200}$/.test(v) : /^#[0-9a-f]{3,8}$|^rgba?\([\d.,\s%]+\)$/i.test(v)) theme[names[i]] = v;
    }
    applyTheme();
    return { theme: true };
  };
  const flash = (op) => {
    let el;
    try {
      el = resolve({ selector: op.selector });
    } catch (_) {
      return { found: false };
    }
    el.scrollIntoView({ block: "center", inline: "nearest", behavior: "instant" });
    if (ensureHost()) {
      const r = el.getBoundingClientRect();
      const ring = make("div");
      attr(ring, "class", "flash");
      css(ring, "left:" + px(r.left) + ";top:" + px(r.top) + ";width:" + px(r.width) + ";height:" + px(r.height));
      put(rootNode, ring);
      setTimeout(() => drop(ring), 600);
    }
    return { found: true };
  };
  Object.assign(modules, { overlay, pick, markers, flash, theme: themeOp, presence });
