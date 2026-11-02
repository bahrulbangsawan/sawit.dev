  // ---- Host: one closed shadow root on <html>, made once the page has settled ----

  let host = null;
  let shadow = null;
  let rootNode = null;
  let layer = null;
  let agentsLayer = null;
  let keepAgents = false;
  let ui = null;
  let ready = false;
  let hidden = false;
  let mode = "off";
  let current = null;
  let down = [];
  let items = [];
  let watching = false;
  let frame = 0;
  let lastHover = 0;
  let hoverTimer = 0;
  // Inspect: the picked element, and whether Alt is held to measure from it.
  let pinned = null;
  let altHeld = false;
  // What `render` drew last (cleared each time), and the markers by number (kept, so one
  // keeps the keyboard).
  let drawn = [];
  let marks = [];
  // `items` changed since the markers were made (before the overlay could be drawn, say).
  let stale = false;
  // The open popover: `{ n, el, intent, down }`, `n` null for a new annotation.
  let pop = null;
  // A press outside closed the popover: the click that follows picks nothing.
  let dismissed = false;
  // Where the keyboard goes back to when the popover closes (the marker that opened it).
  let returnTo = null;
  // The overlay's clickable nodes: `[node, action]`, read by index.
  let actions = [];

  const applyTheme = () => {
    if (!rootNode) return;
    const vars = [
      ["--bg", theme.bg], ["--fg", theme.fg], ["--muted", theme.muted], ["--border", theme.border],
      ["--field", theme.field], ["--accent", theme.accent], ["--primary", theme.primary],
      ["--primary-fg", theme.primaryFg], ["--danger", theme.danger], ["--ring", theme.ring],
      ["--font", theme.font], ["--mono", theme.mono],
    ];
    for (let i = 0; i < vars.length; i++) prop(rootNode, vars[i][0], vars[i][1]);
  };

  // The host waits for the page to settle (`ready`), unless an agent's cursor needs it now
  // (`now`): an agent can act on a page the moment it has loaded.
  const ensureHost = (now) => {
    if ((!ready && !now) || !document.documentElement || !N.createElement || !N.attachShadow || !N.inner) return null;
    if (!host) {
      let made;
      try {
        made = make(HOST_TAG);
        shadow = apply(N.attachShadow, made, [{ __proto__: null, mode: "closed" }]);
      } catch (_) {
        // A page that took the name for an element of its own gets no overlay.
        return null;
      }
      host = made;
      attr(host, "data-sawit-overlay", "");
      css(host, "position:fixed;inset:0;z-index:2147483647;pointer-events:none;margin:0;padding:0;border:0;display:block");
      let styled = false;
      if (SHEET && N.replaceSync && N.sheets) {
        try {
          const sheet = new SHEET();
          apply(N.replaceSync, sheet, [STYLE]);
          apply(N.sheets, shadow, [[sheet]]);
          styled = true;
        } catch (_) {}
      }
      apply(N.inner, shadow, [(styled ? "" : "<style>" + STYLE + "</style>") + MARKUP]);
      rootNode = find(shadow, ".root");
      layer = find(shadow, ".layer");
      agentsLayer = find(shadow, ".agents");
      const one = (id) => find(shadow, "#" + id);
      ui = {
        pop: one("pop-comment") && find(shadow, ".pop"),
        badge: one("pop-n"),
        title: one("pop-title"),
        sub: one("pop-sub"),
        walk: one("pop-walk"),
        up: one("pop-up"),
        down: one("pop-down"),
        kinds: [one("kind-fix"), one("kind-change"), one("kind-question"), one("kind-copy")],
        text: one("pop-comment"),
        remove: one("pop-remove"),
        cancel: one("pop-cancel"),
        save: one("pop-save"),
      };
      actions = [
        [ui.up, "parent"], [ui.down, "child"], [ui.remove, "remove"], [ui.cancel, "cancel"], [ui.save, "save"],
        [ui.kinds[0], "kind:fix"], [ui.kinds[1], "kind:change"], [ui.kinds[2], "kind:question"], [ui.kinds[3], "kind:copy"],
      ];
      applyTheme();
      applyMode();
    }
    if (!read(N.connected, host, "isConnected")) put(document.documentElement, host);
    return host;
  };
  // Hidden for a screenshot, the overlay shows nothing, or (`keepAgents`, when the user wants
  // agents' cursors in agents' screenshots) only the agents' cursors.
  const applyMode = () => {
    if (!host) return;
    prop(host, "display", hidden && !keepAgents ? "none" : "block");
    if (rootNode) attr(rootNode, "class", hidden && keepAgents ? "root shot" : "root");
  };
  const box = (cssText, text) => {
    const div = make("div");
    attr(div, "class", "box");
    css(div, cssText);
    if (text !== undefined) write(div, text);
    put(layer, div);
    drawn[drawn.length] = div;
    return div;
  };
  const px = (n) => Math.round(n * 100) / 100 + "px";
