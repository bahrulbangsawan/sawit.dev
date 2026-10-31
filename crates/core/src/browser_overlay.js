// Sawit's page overlay module: picking, highlights, markers, flash and the
// annotate shortcut. Spliced into the page agent (see browser_agent.js), so
// `send`, `resolve`, `describe`, `clip`, `oneLine`, `modules` and `helpers`
// are in scope.
//
// Picker and selector logic adapted from Visual Annotator (MIT License, ©
// 2026 wernerstrauch).
{
  const HOST_TAG = "sawit-overlay";
  const ACCENT = "#0a84ff";

  // ---- Host: one closed shadow root on <html>, made once the page has settled ----

  let host = null;
  let layer = null;
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

  const ensureHost = () => {
    if (!ready || !document.documentElement) return null;
    if (!host) {
      host = document.createElement(HOST_TAG);
      host.setAttribute("data-sawit-overlay", "");
      const shadow = host.attachShadow({ mode: "closed" });
      layer = document.createElement("div");
      layer.style.cssText = "position:absolute;inset:0;overflow:hidden";
      shadow.appendChild(layer);
      host.style.cssText = "position:fixed;inset:0;z-index:2147483647;pointer-events:none;margin:0;padding:0;border:0";
      applyMode();
    }
    if (!host.isConnected) document.documentElement.appendChild(host);
    return host;
  };
  const applyMode = () => {
    if (!host) return;
    // Shown is no inline display at all (position:fixed already makes it a block).
    host.style.display = hidden ? "none" : "";
  };
  const box = (css, text) => {
    const div = document.createElement("div");
    div.style.cssText = "position:absolute;box-sizing:border-box;" + css;
    if (text !== undefined) div.textContent = text;
    return div;
  };
  const px = (n) => Math.round(n * 100) / 100 + "px";

  // ---- Selectors ----

  const unique = (el, selector) => {
    try {
      const found = el.getRootNode().querySelectorAll(selector);
      return found.length === 1 && found[0] === el;
    } catch (_) {
      return false;
    }
  };
  const selectorFor = (el) => {
    if (el.id) {
      const byId = "#" + CSS.escape(el.id);
      if (unique(el, byId)) return byId;
    }
    const tag = el.localName;
    const classes = [...el.classList].filter((c) => !/^\d/.test(c)).map((c) => "." + CSS.escape(c)).join("");
    if (classes && unique(el, tag + classes)) return tag + classes;
    const parts = [];
    let node = el;
    while (node && node.nodeType === Node.ELEMENT_NODE) {
      if (node.id && node !== el) {
        parts.unshift("#" + CSS.escape(node.id));
        break;
      }
      if (node.localName === "body" || node.localName === "html") {
        parts.unshift(node.localName);
        break;
      }
      const same = node.parentElement ? [...node.parentElement.children].filter((c) => c.localName === node.localName) : [node];
      parts.unshift(node.localName + (same.length > 1 ? ":nth-of-type(" + (same.indexOf(node) + 1) + ")" : ""));
      node = node.parentElement;
    }
    return parts.join(" > ");
  };
  helpers.selectorFor = selectorFor;

  const rectOf = (el) => {
    const r = el.getBoundingClientRect();
    return { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) };
  };
  // `button#id.a.b`: the element's tag, id and first two classes.
  const short = (el) => {
    const cls = typeof el.className === "string" && el.className.trim() ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".") : "";
    return el.localName + (el.id ? "#" + el.id : "") + cls;
  };
  const chip = (el) => {
    const r = el.getBoundingClientRect();
    return short(el) + " " + Math.round(r.width) + "×" + Math.round(r.height);
  };
  const info = (el, max = 120) => ({
    selector: selectorFor(el),
    label: short(el),
    tag: el.localName,
    id: el.id || "",
    classes: typeof el.className === "string" ? el.className.trim().split(/\s+/).filter(Boolean) : [],
    text: clip(oneLine(el.innerText || el.textContent), max),
    rect: rectOf(el),
  });

  // ---- Pick details ----

  const STYLE_PROPS = [
    ["position", "static"],
    ["overflow", "visible"],
    ["z-index", "auto"],
    ["opacity", "1"],
    ["background-color", "transparent", "rgba(0, 0, 0, 0)"],
    ["color", "rgb(0, 0, 0)"],
    ["font-size", "16px"],
    ["font-weight", "400"],
    ["border-radius", "0px"],
    ["gap", "normal"],
    ["padding", "0px"],
    ["margin", "0px"],
  ];
  const stylesOf = (el) => {
    const s = getComputedStyle(el);
    const out = [["display", s.getPropertyValue("display")]];
    for (const [prop, ...defaults] of STYLE_PROPS) {
      const v = s.getPropertyValue(prop);
      if (v && !defaults.includes(v)) out.push([prop, v]);
    }
    return out;
  };
  const nameOf = (t) => (t && (typeof t === "function" || typeof t === "object") ? t.displayName || t.name || t.__name || "" : "");
  const reactHint = (el) => {
    const key = Object.keys(el).find((k) => k.startsWith("__reactFiber$"));
    if (!key) return null;
    let file = "";
    let line = null;
    let component = "";
    let f = el[key];
    for (let i = 0; f && i < 50; i++, f = f.return) {
      if (!file && f._debugSource && f._debugSource.fileName) {
        file = f._debugSource.fileName;
        line = f._debugSource.lineNumber || null;
      }
      if (!component && typeof f.type === "function") component = nameOf(f.type);
      if (file && component) break;
    }
    if (!file && !component) return null;
    return { framework: "React", file, line, component: component || null };
  };
  const hintOf = (el) => {
    const react = reactHint(el);
    if (react) return react;
    const v3 = el.__vueParentComponent && el.__vueParentComponent.type;
    if (v3) return { framework: "Vue", file: v3.__file || "", line: null, component: v3.name || v3.__name || null };
    const v2 = el.__vue__ && el.__vue__.$options;
    if (v2) return { framework: "Vue", file: v2.__file || "", line: null, component: v2.name || null };
    const loc = el.__svelte_meta && el.__svelte_meta.loc;
    if (loc && loc.file) return { framework: "Svelte", file: loc.file, line: (loc.line || 0) + 1, component: null };
    const ds = el.getAttribute && el.getAttribute("data-source");
    if (ds) {
      const m = /^(.*?)(?::(\d+))?(?::\d+)?$/.exec(ds);
      return { framework: "source", file: m[1], line: m[2] ? Number(m[2]) : null, component: null };
    }
    const ip = el.getAttribute && el.getAttribute("data-inspector-relative-path");
    if (ip) {
      const ln = Number(el.getAttribute("data-inspector-line"));
      return { framework: "source", file: ip, line: ln || null, component: null };
    }
    return null;
  };
  const sourceOf = (el) => {
    try {
      for (let n = el; n && n.nodeType === Node.ELEMENT_NODE; n = n.parentElement) {
        const h = hintOf(n);
        if (h) return h;
      }
    } catch (_) {}
    return null;
  };
  const details = (el) => {
    const r = el.getBoundingClientRect();
    const rect = { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) };
    return {
      ...info(el, 500),
      rect,
      styles: stylesOf(el),
      source: sourceOf(el),
      html: (el.outerHTML || "").slice(0, 20000),
      off_screen: r.left < 0 || r.top < 0 || r.right > innerWidth || r.bottom > innerHeight,
    };
  };

  // ---- Drawing ----

  const drawHighlight = () => {
    if (!current || !current.isConnected) return;
    const r = current.getBoundingClientRect();
    const s = getComputedStyle(current);
    const n = (name) => parseFloat(s[name]) || 0;
    const m = { t: n("marginTop"), r: n("marginRight"), b: n("marginBottom"), l: n("marginLeft") };
    const bw = { t: n("borderTopWidth"), r: n("borderRightWidth"), b: n("borderBottomWidth"), l: n("borderLeftWidth") };
    const p = { t: n("paddingTop"), r: n("paddingRight"), b: n("paddingBottom"), l: n("paddingLeft") };
    const at = (x, y, w, h) => "left:" + px(x) + ";top:" + px(y) + ";width:" + px(Math.max(w, 0)) + ";height:" + px(Math.max(h, 0)) + ";";
    layer.appendChild(box(at(r.left - m.l, r.top - m.t, r.width + m.l + m.r, r.height + m.t + m.b) + "background:rgba(246,178,107,.35)"));
    const px0 = r.left + bw.l;
    const py0 = r.top + bw.t;
    const pw = r.width - bw.l - bw.r;
    const ph = r.height - bw.t - bw.b;
    layer.appendChild(box(at(px0, py0, pw, ph) + "background:rgba(147,196,125,.35)"));
    layer.appendChild(box(at(px0 + p.l, py0 + p.t, pw - p.l - p.r, ph - p.t - p.b) + "outline:2px solid " + ACCENT + ";outline-offset:-2px"));
    const label = box("left:" + px(Math.max(r.left, 0)) + ";top:" + px(r.top < 24 ? r.bottom + 4 : r.top - 22) +
      ";padding:3px 6px;border-radius:4px;background:#1d1d1f;color:#fff;font:11px/14px system-ui,sans-serif;white-space:nowrap", chip(current));
    layer.appendChild(label);
  };
  const drawMarkers = () => {
    for (const item of items) {
      let el;
      try {
        el = resolve({ selector: item.selector });
      } catch (_) {
        continue;
      }
      const r = el.getBoundingClientRect();
      layer.appendChild(box("left:" + px(r.left - 10) + ";top:" + px(r.top - 10) +
        ";width:20px;height:20px;border-radius:50%;background:" + ACCENT +
        ";color:#fff;font:bold 11px/20px system-ui,sans-serif;text-align:center", String(item.n)));
    }
  };
  // Alt-measure: distance lines with px labels between the picked box `a` and
  // the hovered box `b`.
  const MEASURE = "#ff375f";
  const drawMeasure = () => {
    if (mode !== "inspect" || !altHeld || !pinned || !pinned.isConnected || !current || current === pinned) return;
    const a = pinned.getBoundingClientRect();
    const b = current.getBoundingClientRect();
    layer.appendChild(box("left:" + px(a.left) + ";top:" + px(a.top) + ";width:" + px(a.width) + ";height:" + px(a.height) +
      ";outline:1px dashed " + MEASURE + ";outline-offset:-1px"));
    const tag = (x, y, d) => {
      layer.appendChild(box("left:" + px(x) + ";top:" + px(y) + ";transform:translate(-50%,-50%);padding:1px 4px;border-radius:3px;background:" +
        MEASURE + ";color:#fff;font:10px/13px system-ui,sans-serif;white-space:nowrap", Math.round(d * 10) / 10 + "px"));
    };
    const hLine = (x1, x2, y) => {
      if (x2 - x1 < 0.5) return;
      layer.appendChild(box("left:" + px(x1) + ";top:" + px(y) + ";width:" + px(x2 - x1) + ";height:1px;background:" + MEASURE));
      tag((x1 + x2) / 2, y, x2 - x1);
    };
    const vLine = (y1, y2, x) => {
      if (y2 - y1 < 0.5) return;
      layer.appendChild(box("left:" + px(x) + ";top:" + px(y1) + ";width:1px;height:" + px(y2 - y1) + ";background:" + MEASURE));
      tag(x, (y1 + y2) / 2, y2 - y1);
    };
    // Where a line between the boxes runs: the middle of their overlap, else the picked box's middle.
    const midY = Math.max(a.top, b.top) < Math.min(a.bottom, b.bottom) ? (Math.max(a.top, b.top) + Math.min(a.bottom, b.bottom)) / 2 : a.top + a.height / 2;
    const midX = Math.max(a.left, b.left) < Math.min(a.right, b.right) ? (Math.max(a.left, b.left) + Math.min(a.right, b.right)) / 2 : a.left + a.width / 2;
    // Horizontal: the gap between them, else (overlapping) the distance between matching edges.
    if (b.left >= a.right) hLine(a.right, b.left, midY);
    else if (a.left >= b.right) hLine(b.right, a.left, midY);
    else {
      hLine(Math.min(a.left, b.left), Math.max(a.left, b.left), midY);
      hLine(Math.min(a.right, b.right), Math.max(a.right, b.right), midY);
    }
    if (b.top >= a.bottom) vLine(a.bottom, b.top, midX);
    else if (a.top >= b.bottom) vLine(b.bottom, a.top, midX);
    else {
      vLine(Math.min(a.top, b.top), Math.max(a.top, b.top), midX);
      vLine(Math.min(a.bottom, b.bottom), Math.max(a.bottom, b.bottom), midX);
    }
  };
  const render = () => {
    if (!host && items.length === 0 && mode === "off") return;
    if (!ensureHost()) return;
    for (const child of [...layer.children]) {
      if (!child.hasAttribute("data-flash")) child.remove();
    }
    drawMarkers();
    if (mode !== "off") drawHighlight();
    drawMeasure();
  };
  const schedule = () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      render();
    });
  };
  let timer = 0;
  const watch = () => {
    const want = items.length > 0 || mode !== "off";
    if (want === watching) return;
    watching = want;
    const act = want ? "addEventListener" : "removeEventListener";
    window[act]("scroll", schedule, { capture: true, passive: true });
    window[act]("resize", schedule, { passive: true });
    if (want) timer = setInterval(schedule, 500);
    else clearInterval(timer);
  };

  if (document.readyState === "complete") {
    setTimeout(() => {
      ready = true;
      render();
    }, 1000);
  } else {
    window.addEventListener("load", () => {
      setTimeout(() => {
        ready = true;
        render();
      }, 1000);
    }, { once: true });
  }

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
    const parent = current.parentElement || (current.getRootNode() instanceof ShadowRoot ? current.getRootNode().host : null);
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
    const h = hit(e.clientX, e.clientY);
    const el = current && h && current.contains(h) ? current : h || current;
    if (!el) return;
    if (mode === "inspect") pinned = el;
    send("picked", { mode, element: details(el) });
  };
  const altKey = (e) => {
    if (e.key === "Alt" && altHeld !== (e.type === "keydown")) {
      altHeld = e.type === "keydown";
      schedule();
    }
  };
  const keydown = (e) => {
    altKey(e);
    if (e.key === "Escape") {
      swallow(e);
      send("key", "escape");
      stopPicking();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      swallow(e);
      if (e.key === "ArrowUp") up();
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
  const LISTENERS = [
    ["pointermove", move],
    ["mousemove", move],
    ...SWALLOWED.map((type) => [type, swallow]),
    ["click", click],
    ["keydown", keydown],
    ["keyup", altKey],
    ["wheel", wheel],
  ];
  const stopPicking = () => {
    if (mode === "off") return;
    for (const [type, fn] of LISTENERS) window.removeEventListener(type, fn, { capture: true });
    mode = "off";
    current = null;
    pinned = null;
    altHeld = false;
    down = [];
    clearTimeout(hoverTimer);
    hoverTimer = 0;
    applyMode();
    watch();
    render();
  };

  // The annotate shortcut works on every page, picking or not. Synthetic
  // events (browser_press_key "Meta+Shift+.") carry no `code`, so then the key
  // counts; a real key's `key` would also match ⌘⇧< on layouts where Shift-< is ">".
  window.addEventListener("keydown", (e) => {
    const period = e.code ? e.code === "Period" : e.key === "." || e.key === ">";
    if (e.metaKey && e.shiftKey && period) {
      e.preventDefault();
      send("key", "toggle-annotate");
    }
  }, { capture: true });

  // ---- Ops ----

  const overlay = (op) => {
    hidden = !!op.hidden;
    applyMode();
    return { hidden };
  };
  const pick = (op) => {
    if (op.mode !== "annotate" && op.mode !== "inspect" && op.mode !== "off") throw new Error("Pick mode is annotate, inspect or off.");
    if (op.mode === "off") {
      stopPicking();
    } else if (mode === "off") {
      mode = op.mode;
      for (const [type, fn] of LISTENERS) window.addEventListener(type, fn, { capture: true });
      applyMode();
      watch();
    } else {
      mode = op.mode;
    }
    if (mode !== "inspect") pinned = null;
    return { mode };
  };
  const markers = (op) => {
    items = [];
    const placed = [];
    const missing = [];
    for (const item of op.items || []) {
      try {
        resolve({ selector: item.selector });
        items.push({ n: item.n, selector: item.selector });
        placed.push(item.n);
      } catch (_) {
        missing.push(item.n);
      }
    }
    watch();
    render();
    return { placed, missing };
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
      const ring = box("left:" + px(r.left) + ";top:" + px(r.top) + ";width:" + px(r.width) + ";height:" + px(r.height) +
        ";outline:3px solid " + ACCENT + ";outline-offset:2px;border-radius:2px");
      ring.setAttribute("data-flash", "");
      layer.appendChild(ring);
      if (ring.animate) ring.animate([{ opacity: 1 }, { opacity: 0.15 }, { opacity: 1 }, { opacity: 0 }], { duration: 600 });
      setTimeout(() => ring.remove(), 600);
    }
    return { found: true };
  };
  Object.assign(modules, { overlay, pick, markers, flash });
}
