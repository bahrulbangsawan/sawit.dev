  // ---- Keys and chrome (lane 3: mesurer-ui) ----
  //
  // `HANDLERS.key({key, code?, meta?, shift?, alt?, ctrl?})`: a key the app hands to mesurer, as a
  // keydown and keyup at the element with the keyboard, as the page would get them. GPUI takes ⌘K,
  // ⇧⌘K (the workspace's ⌘K chords would hold them) and ⌘A in mesurer's Select tool (the Edit
  // menu's Select All would take it) before the page sees them, and gives them to mesurer this way
  // (crates/app/src/browser/mesurer/keys.rs). Answers `{key, prevented}`.
  //
  // Links: mesurer opens its website with `window.open` (Settings → Website), which in this world
  // posts `link.open` {url}: the app opens an https link in the default browser, never in the tab.
  //
  // The gate's order: mesurer's keyboard gate goes into the page's world after mesurer's first
  // mount, so mesurer's own window listeners come before it. Mounted again on the same page
  // (Annotate closed and opened), mesurer's come after the gate, which, while mesurer has the
  // keyboard, stops every key it doesn't bridge to mesurer: L, B, ⌥S and ⌥ among them. This
  // listener is there from the glue on, before the gate, and hands exactly those keys to mesurer
  // itself: the original is stopped and prevented, as the gate would do, and the same key is
  // dispatched on mesurer's shadow root, where mesurer listens too, not composed, so it goes no
  // further (the page never sees it).
  const OWNED_ATTR = "data-mesurer-keyboard-owned";
  const BRIDGED_KEYS = new Set(["1", "2", "a", "backspace", "c", "d", "delete", "escape", "g", "h", "i", "m", "n", "p", "r", "s", "t", "v", "x"]);
  const MODIFIED_KEYS = new Set([",", "a", "k", "z"]);

  const deepActive = () => {
    let active = document.activeElement;
    while (active && active.shadowRoot && active.shadowRoot.activeElement) active = active.shadowRoot.activeElement;
    return active;
  };
  const editable = (node) => {
    if (!node || node.nodeType !== 1) return false;
    const flag = node.getAttribute("contenteditable");
    if (flag === "false") return false;
    return /^(INPUT|TEXTAREA|SELECT)$/.test(node.tagName) || Boolean(node.isContentEditable) || flag !== null;
  };

  // mesurer's own rules (core/keyboard-ownership.ts, keyboard-gate.ts), for the keys the gate
  // stops outright: not bridged, not an annotation key, not a chord the browser keeps.
  const toolGroupKey = (e) =>
    ["1", "!", "&", "2", "@", "é"].includes(e.key) || ["Digit1", "Numpad1", "Digit2", "Numpad2"].includes(e.code);
  const bridgedKey = (e) => {
    const key = String(e.key || "").toLowerCase();
    const mod = e.metaKey || e.ctrlKey;
    return (
      !e.altKey &&
      ((toolGroupKey(e) && !mod) ||
        (BRIDGED_KEYS.has(key) && !mod && !e.shiftKey) ||
        (MODIFIED_KEYS.has(key) && mod && (key === "k" || key === "z" || !e.shiftKey)))
    );
  };
  const annotationKey = (e) => {
    const key = String(e.key || "").toLowerCase();
    return key === "backspace" || key === "delete" || ((e.metaKey || e.ctrlKey) && (key === "a" || e.code === "KeyA"));
  };
  const reservedChord = (e) => {
    if (!e.metaKey && !e.ctrlKey) return false;
    const key = String(e.key || "").toLowerCase();
    if (key === "k" && !e.altKey) return false;
    return key !== "z" && key !== "a" && key !== ",";
  };
  const gateStops = (e) => {
    const root = document.documentElement;
    if (!root || !root.hasAttribute(OWNED_ATTR)) return false;
    const active = deepActive();
    if (editable(active) && shadow && shadow.contains(active)) return false;
    return !bridgedKey(e) && !annotationKey(e) && !reservedChord(e);
  };
  const handOver = (e) => {
    if (!mountedNow || !shadow || (e.type !== "keydown" && e.type !== "keyup") || !gateStops(e)) return;
    e.stopImmediatePropagation();
    e.preventDefault();
    const copy = new KeyboardEvent(e.type, {
      key: e.key, code: e.code, location: e.location, repeat: e.repeat, altKey: e.altKey, ctrlKey: e.ctrlKey,
      metaKey: e.metaKey, shiftKey: e.shiftKey, bubbles: true, cancelable: true, composed: false,
    });
    try {
      shadow.dispatchEvent(copy);
    } catch (err) {
      log("warn", "a key couldn't be handed to mesurer:", err);
    }
  };
  if (typeof window.addEventListener === "function") {
    window.addEventListener("keydown", handOver, true);
    window.addEventListener("keyup", handOver, true);
  }

  HANDLERS.key = (args) => {
    const key = args && typeof args.key === "string" ? args.key : "";
    if (!key) throw new Error("A key to press needs its name.");
    const code = typeof args.code === "string" ? args.code : /^[a-z]$/i.test(key) ? `Key${key.toUpperCase()}` : "";
    const init = {
      key, code, metaKey: Boolean(args.meta), shiftKey: Boolean(args.shift), altKey: Boolean(args.alt),
      ctrlKey: Boolean(args.ctrl), bubbles: true, cancelable: true, composed: true,
    };
    const target = deepActive() || document.body || document.documentElement;
    const proceed = target.dispatchEvent(new KeyboardEvent("keydown", init));
    target.dispatchEvent(new KeyboardEvent("keyup", init));
    return { key, prevented: !proceed, mounted: mountedNow };
  };

  // mesurer's links, to the app (`link.open`): an absolute http(s) address only.
  const openLink = (url) => {
    let href = "";
    try {
      href = new URL(String(url), location.href).href;
    } catch {
      return;
    }
    if (!/^https?:\/\//i.test(href)) return;
    bridge("link.open", { url: href }).catch((e) => log("warn", "mesurer's link didn't open:", e));
  };
  try {
    window.open = function open(url) {
      openLink(url);
      return null;
    };
  } catch (e) {
    log("warn", "mesurer's links stay the page's:", e);
  }
  // A link in mesurer's own UI, should it have one.
  if (typeof window.addEventListener === "function") {
    window.addEventListener(
      "click",
      (e) => {
        if (!host || typeof e.composedPath !== "function") return;
        const path = e.composedPath();
        if (!path.includes(host)) return;
        const link = path.find((n) => n && n.nodeType === 1 && n.tagName === "A" && n.href);
        if (!link) return;
        e.preventDefault();
        openLink(link.href);
      },
      true,
    );
  }
