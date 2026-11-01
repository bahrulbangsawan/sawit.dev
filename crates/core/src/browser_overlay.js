// Sawit's page overlay module: picking, highlights, markers, the annotation popover and flash.
// Spliced into the page agent (see browser_agent.js), so `TOKEN`, `send`, `resolve`,
// `describe`, `role`, `nameOf`, `clip`, `oneLine`, `apply`, `modules` and `helpers` are in
// scope.
//
// Picker and selector logic adapted from Visual Annotator (MIT License, ©
// 2026 wernerstrauch).
{
  const HOST_TAG = "sawit-overlay";

  // The listener methods as they are before any page script runs (this module runs with the
  // agent, at document start), called through the agent's own `apply`, with options that
  // have no prototype a page could give a `signal` or `once`. The picker's listeners go only
  // through these, and only by index: a page script that got one (by patching the methods,
  // the array iterator that `for…of` and destructuring use, or `Array.prototype`) could call
  // it with an `isTrusted: true` object of its own and pick for the user. Nor can a page
  // take one from the `caller` of a function a listener calls (a getter on the event, a
  // replaced `preventDefault`): the agent is strict-mode code, so `caller` is null there.
  const addListener = EventTarget.prototype.addEventListener;
  const removeListener = EventTarget.prototype.removeEventListener;
  const listen = (type, fn, options) => apply(addListener, window, [type, fn, options]);
  const unlisten = (type, fn, options) => apply(removeListener, window, [type, fn, options]);
  const CAPTURE = { __proto__: null, capture: true };
  const SCROLLS = { __proto__: null, capture: true, passive: true };
  const PASSIVE = { __proto__: null, passive: true };
  const ONCE = { __proto__: null, once: true };

  // ---- Natives, taken at document start ----
  //
  // The popover holds what the user types for the agent, which reaches it as the user's own
  // words. So no page script may get hold of a node of the overlay (it could read or rewrite
  // the comment), nor read a key typed into it. Every DOM call on the overlay's nodes goes
  // through these, never through a method or getter a page could have replaced by the time
  // the overlay is drawn; arrays that hold its nodes are read and written by index. Where a
  // native is missing (a test's stand-in page), the plain property is used.
  const protoOf = (name) => {
    const C = window[name];
    return typeof C === "function" && C.prototype ? C.prototype : null;
  };
  const method = (p, name) => (p && typeof p[name] === "function" ? p[name] : null);
  const getter = (p, name) => {
    const d = p ? Object.getOwnPropertyDescriptor(p, name) : null;
    return d && d.get ? d.get : null;
  };
  const setter = (p, name) => {
    const d = p ? Object.getOwnPropertyDescriptor(p, name) : null;
    return d && d.set ? d.set : null;
  };
  const EV = protoOf("Event");
  const KEV = protoOf("KeyboardEvent");
  const MEV = protoOf("MouseEvent");
  const UIEV = protoOf("UIEvent");
  const NODE = protoOf("Node");
  const ELEM = protoOf("Element");
  const HTML = protoOf("HTMLElement");
  const DOC = protoOf("Document");
  const FRAG = protoOf("DocumentFragment");
  const SHADOW = protoOf("ShadowRoot");
  const AREA = protoOf("HTMLTextAreaElement");
  const CSSD = protoOf("CSSStyleDeclaration");
  const SHEET = window.CSSStyleSheet;
  const N = {
    target: getter(EV, "target"),
    type: getter(EV, "type"),
    stop: method(EV, "stopImmediatePropagation"),
    prevent: method(EV, "preventDefault"),
    key: getter(KEV, "key"),
    composing: getter(KEV, "isComposing"),
    meta: getter(KEV, "metaKey"),
    shift: getter(KEV, "shiftKey"),
    ctrl: getter(KEV, "ctrlKey"),
    keyAlt: getter(KEV, "altKey"),
    mouseAlt: getter(MEV, "altKey"),
    x: getter(MEV, "clientX"),
    y: getter(MEV, "clientY"),
    detail: getter(UIEV, "detail"),
    createElement: method(DOC, "createElement"),
    attachShadow: method(ELEM, "attachShadow"),
    setAttribute: method(ELEM, "setAttribute"),
    removeAttribute: method(ELEM, "removeAttribute"),
    box: method(ELEM, "getBoundingClientRect"),
    scrollHeight: getter(ELEM, "scrollHeight"),
    remove: method(ELEM, "remove"),
    appendChild: method(NODE, "appendChild"),
    text: setter(NODE, "textContent"),
    connected: getter(NODE, "isConnected"),
    style: getter(HTML, "style"),
    focus: method(HTML, "focus"),
    blur: method(HTML, "blur"),
    query: method(FRAG, "querySelector"),
    inner: setter(SHADOW, "innerHTML"),
    active: getter(SHADOW, "activeElement"),
    sheets: setter(SHADOW, "adoptedStyleSheets"),
    value: getter(AREA, "value"),
    setValue: setter(AREA, "value"),
    cssText: setter(CSSD, "cssText"),
    setProperty: method(CSSD, "setProperty"),
    replaceSync: SHEET && method(SHEET.prototype, "replaceSync"),
  };
  // An event's field, through its native getter.
  const read = (get, e, name) => (get ? apply(get, e, []) : e[name]);
  const stopNow = (e) => (N.stop ? apply(N.stop, e, []) : e.stopImmediatePropagation());
  const preventNow = (e) => (N.prevent ? apply(N.prevent, e, []) : e.preventDefault());
  // DOM calls on the overlay's own nodes.
  const make = (tag) => apply(N.createElement, document, [tag]);
  const put = (parent, child) => apply(N.appendChild, parent, [child]);
  const drop = (node) => apply(N.remove, node, []);
  const attr = (node, name, value) =>
    value === null ? apply(N.removeAttribute, node, [name]) : apply(N.setAttribute, node, [name, value]);
  const write = (node, text) => apply(N.text, node, [text]);
  const css = (node, text) => apply(N.cssText, apply(N.style, node, []), [text]);
  const prop = (node, name, value) => apply(N.setProperty, apply(N.style, node, []), [name, value]);
  const boxOf = (node) => apply(N.box, node, []);
  const find = (root, selector) => apply(N.query, root, [selector]);
  const focusOn = (node) => apply(N.focus, node, [{ __proto__: null, preventScroll: true }]);

  // ---- Theme: Sawit's colours, which the app sends (`theme`) ----

  const theme = {
    bg: "#282c33",
    fg: "#dce0e5",
    muted: "#a9afbc",
    border: "#464b57",
    field: "#1f2227",
    accent: "#363c46",
    primary: "#74ade8",
    primaryFg: "#1b1d22",
    danger: "#d07277",
    ring: "#74ade8",
    font: "-apple-system, BlinkMacSystemFont, system-ui, sans-serif",
    mono: "ui-monospace, Menlo, monospace",
  };
  const MEASURE = "#ff375f";

  const STYLE = `
:host { all: initial; }
.root { position: absolute; inset: 0; overflow: hidden; font: 12px/1.4 var(--font); color: var(--fg);
  -webkit-font-smoothing: antialiased; letter-spacing: 0; text-transform: none; text-align: left; direction: ltr; }
.root * { box-sizing: border-box; }
.box { position: absolute; box-sizing: border-box; pointer-events: none; }
.label { position: absolute; pointer-events: none; display: flex; gap: 6px; align-items: baseline; padding: 3px 7px;
  border-radius: 6px; background: var(--bg); border: 1px solid var(--border); color: var(--fg); white-space: nowrap;
  font: 500 11px/16px var(--mono); box-shadow: 0 2px 8px rgba(0,0,0,.24); max-width: calc(100vw - 16px); overflow: hidden; }
.label .size { color: var(--muted); font-weight: 400; font-variant-numeric: tabular-nums; }
.mark { position: absolute; pointer-events: auto; min-width: 24px; height: 24px; padding: 0 6px; border-radius: 12px;
  border: 2px solid var(--bg); background: var(--primary); color: var(--primary-fg); font: 600 11px/20px var(--font);
  font-variant-numeric: tabular-nums; text-align: center; cursor: pointer; box-shadow: 0 1px 4px rgba(0,0,0,.3); margin: 0; }
.mark:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
.mark.open { background: var(--fg); color: var(--bg); }
.pop { position: absolute; pointer-events: auto; width: 320px; max-width: calc(100vw - 16px); display: flex;
  flex-direction: column; gap: 12px; padding: 12px; border-radius: 10px; background: var(--bg); color: var(--fg);
  border: 1px solid var(--border); box-shadow: 0 12px 32px rgba(0,0,0,.32), 0 2px 6px rgba(0,0,0,.2); }
.pop[hidden] { display: none; }
.head { display: flex; gap: 8px; align-items: flex-start; }
.badge { flex: none; min-width: 22px; height: 22px; padding: 0 6px; border-radius: 11px; background: var(--primary);
  color: var(--primary-fg); font: 600 11px/22px var(--font); text-align: center; font-variant-numeric: tabular-nums; }
.what { flex: 1; min-width: 0; }
.title { font: 500 12px/18px var(--mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sub { color: var(--muted); font-size: 11px; line-height: 16px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  font-variant-numeric: tabular-nums; }
.walk { flex: none; display: flex; gap: 4px; }
.walk[hidden] { display: none; }
button { font: 500 12px/1 var(--font); color: var(--fg); cursor: pointer; }
.icon { width: 24px; height: 24px; display: inline-flex; align-items: center; justify-content: center; padding: 0;
  border-radius: 6px; border: 1px solid var(--border); background: transparent; color: var(--muted); }
.icon:hover { color: var(--fg); background: var(--accent); }
.icon:disabled { opacity: .45; cursor: default; background: transparent; }
.kinds { display: flex; padding: 2px; gap: 2px; border-radius: 7px; background: var(--field); border: 1px solid var(--border); }
.kind { flex: 1; height: 24px; border: 0; border-radius: 5px; background: transparent; color: var(--muted); }
.kind:hover { color: var(--fg); }
.kind[aria-checked="true"] { background: var(--accent); color: var(--fg); box-shadow: 0 1px 2px rgba(0,0,0,.25); }
textarea { display: block; width: 100%; min-height: 64px; max-height: 160px; resize: none; margin: 0; padding: 8px 10px;
  border-radius: 7px; border: 1px solid var(--border); background: var(--field); color: var(--fg);
  font: 13px/1.45 var(--font); outline: none; }
textarea::placeholder { color: var(--muted); opacity: 1; }
textarea:focus { border-color: var(--ring); box-shadow: 0 0 0 2px color-mix(in srgb, var(--ring) 30%, transparent); }
.foot { display: flex; gap: 8px; align-items: center; }
.grow { flex: 1; }
.btn { height: 28px; padding: 0 10px; display: inline-flex; gap: 6px; align-items: center; border-radius: 7px;
  border: 1px solid var(--border); background: transparent; }
.btn:hover { background: var(--accent); }
.btn.primary { background: var(--primary); border-color: var(--primary); color: var(--primary-fg); }
.btn.primary:hover { filter: brightness(1.08); }
.btn.danger { color: var(--danger); border-color: transparent; padding: 0 6px; }
.btn.danger:hover { background: color-mix(in srgb, var(--danger) 14%, transparent); }
.btn[hidden] { display: none; }
kbd { font: 500 10px/1 var(--font); opacity: .7; }
button:focus-visible, .kind:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
.flash { position: absolute; pointer-events: none; border-radius: 2px; outline: 3px solid var(--primary); outline-offset: 2px; }
.root.shot > :not(.agents) { display: none; }
.agents, .agents * { pointer-events: none; }
.agent { position: absolute; left: 0; top: 0; z-index: 1; }
.agent.faded { opacity: .4; }
.agent.faded .doing, .agent.faded .typing { display: none; }
.agent .tip { position: absolute; left: 0; top: 0; width: 13px; height: 19px;
  filter: drop-shadow(1px 0 0 #fff) drop-shadow(-1px 0 0 #fff) drop-shadow(0 1px 0 #fff) drop-shadow(0 -1px 0 #fff) drop-shadow(0 1px 2px rgba(0,0,0,.35)); }
.agent .arrow { width: 100%; height: 100%; background: var(--c);
  clip-path: polygon(0 0, 0 16px, 4.4px 12.3px, 7.3px 18.6px, 9.9px 17.4px, 7.1px 11.2px, 12.6px 11.2px); }
.agent .pill { position: absolute; left: 12px; top: 17px; display: flex; gap: 6px; align-items: baseline; max-width: 280px;
  padding: 2px 8px; border-radius: 10px; background: var(--c); color: #fff; font: 600 11px/16px var(--font); white-space: nowrap;
  overflow: hidden; text-overflow: ellipsis; box-shadow: 0 0 0 1.5px #fff, 0 2px 6px rgba(0,0,0,.25); }
.agent .doing { font-weight: 400; overflow: hidden; text-overflow: ellipsis; }
.agent .typing { font-weight: 400; }
.agent.west .pill { left: auto; right: 2px; }
.agent.north .pill { top: auto; bottom: 4px; }
.ring { position: absolute; border-radius: 4px; outline: 2px solid var(--c); outline-offset: 2px;
  box-shadow: 0 0 0 4px rgba(255,255,255,.55); }
.ripple { position: absolute; width: 28px; height: 28px; margin: -14px 0 0 -14px; border-radius: 50%;
  border: 2px solid var(--c); box-shadow: 0 0 0 1px #fff; opacity: 0; }
.caret { position: absolute; width: 2px; border-radius: 1px; background: var(--c); box-shadow: 0 0 0 1px #fff; }
@media (prefers-reduced-motion: reduce) {
  .ripple { opacity: 1; }
}
@media (prefers-reduced-motion: no-preference) {
  .pop.in { animation: pop-in 140ms cubic-bezier(0.2, 0, 0, 1); }
  .flash { animation: flash 600ms ease-out forwards; }
  .agent { transition-property: transform, opacity; transition-duration: var(--glide, 120ms), 200ms;
    transition-timing-function: cubic-bezier(0.2, 0, 0, 1), ease-out; }
  .ripple { animation: ripple 420ms cubic-bezier(0.2, 0, 0, 1) var(--delay, 0ms) forwards; }
  .caret, .agent .typing { animation: blink 1s steps(1) infinite; }
}
@keyframes pop-in { from { opacity: 0; transform: translateY(var(--rise, 4px)); } to { opacity: 1; transform: none; } }
@keyframes flash { 0% { opacity: 1; } 30% { opacity: .2; } 60% { opacity: 1; } 100% { opacity: 0; } }
@keyframes ripple { from { opacity: 1; transform: scale(.4); } to { opacity: 0; transform: scale(1.6); } }
@keyframes blink { 50% { opacity: 0; } }
`;
  const UP = '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 13V3M4 7l4-4 4 4"/></svg>';
  const DOWN = '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 3v10M4 9l4 4 4-4"/></svg>';
  const TRASH = '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4"/></svg>';
  // Static markup: what the page wrote is set as text later, never parsed.
  const MARKUP =
    '<div class="root" part="root"><div class="layer"></div><div class="agents" aria-hidden="true"></div>' +
    '<div class="pop" role="dialog" aria-labelledby="pop-title" aria-describedby="pop-sub" hidden>' +
    '<div class="head"><span class="badge" id="pop-n"></span><div class="what"><div class="title" id="pop-title"></div>' +
    '<div class="sub" id="pop-sub"></div></div><div class="walk" id="pop-walk">' +
    '<button class="icon" id="pop-up" type="button" aria-label="Select the parent element" title="Parent element (⌥↑)">' + UP + "</button>" +
    '<button class="icon" id="pop-down" type="button" aria-label="Select the child element again" title="Child element (⌥↓)">' + DOWN + "</button>" +
    "</div></div>" +
    '<div class="kinds" role="radiogroup" aria-label="Kind">' +
    '<button class="kind" id="kind-fix" type="button" role="radio">Fix</button>' +
    '<button class="kind" id="kind-change" type="button" role="radio">Change</button>' +
    '<button class="kind" id="kind-question" type="button" role="radio">Question</button>' +
    '<button class="kind" id="kind-copy" type="button" role="radio" title="Ask the agent to reuse this element; adds its HTML">Copy</button>' +
    "</div>" +
    '<textarea id="pop-comment" rows="3" placeholder="What should change?" aria-label="Comment for the agent"></textarea>' +
    '<div class="foot"><button class="btn danger" id="pop-remove" type="button" aria-label="Remove this annotation">' + TRASH + "Remove</button>" +
    '<span class="grow"></span><button class="btn" id="pop-cancel" type="button">Cancel <kbd>Esc</kbd></button>' +
    '<button class="btn primary" id="pop-save" type="button">Save <kbd>↩</kbd></button></div>' +
    "</div></div>";
  const KINDS = ["fix", "change", "question", "copy"];

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

  // ---- Selectors ----

  const unique = (el, selector) => {
    try {
      const found = el.getRootNode().querySelectorAll(selector);
      return found.length === 1 && found[0] === el;
    } catch (_) {
      return false;
    }
  };
  // A class or id a build tool made up (`css-1x2y3z`, `sc-bdVaJa`, `jsx-4012`, `radix-:r1:`):
  // it changes with the next build, so a selector for the agent doesn't lean on it.
  const madeUp = (name) =>
    !name || name.length > 40 || /\d{3,}/.test(name) || /[:[\]/@]/.test(name) ||
    /^(?:css|sc|jsx|svelte|emotion|styled|chakra|mui|tw|ng)-/i.test(name) ||
    (/\d/.test(name) && /^[a-z]{1,3}[A-Z0-9]\w{3,}$/.test(name));
  helpers.madeUp = madeUp;
  const quoted = (value) => '"' + String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"') + '"';
  const TEST_ATTRS = ["data-testid", "data-test-id", "data-test", "data-cy", "data-qa"];
  // A selector that names `el` alone, short and lasting: its id, a test id, its tag and
  // classes, an accessible label or name, else a path from the nearest named ancestor.
  const selectorFor = (el) => {
    const tag = el.localName;
    if (el.id && !madeUp(el.id)) {
      const byId = "#" + CSS.escape(el.id);
      if (unique(el, byId)) return byId;
    }
    for (let i = 0; i < TEST_ATTRS.length; i++) {
      const v = el.getAttribute && el.getAttribute(TEST_ATTRS[i]);
      if (v) {
        const s = "[" + TEST_ATTRS[i] + "=" + quoted(v) + "]";
        if (unique(el, s)) return s;
        if (unique(el, tag + s)) return tag + s;
      }
    }
    const classes = [...el.classList].filter((c) => !/^\d/.test(c) && !madeUp(c)).map((c) => "." + CSS.escape(c));
    for (let n = 1; n <= Math.min(classes.length, 3); n++) {
      const s = tag + classes.slice(0, n).join("");
      if (unique(el, s)) return s;
    }
    const ATTRS = ["aria-label", "name", "title", "alt", "placeholder", "href"];
    for (let i = 0; i < ATTRS.length; i++) {
      const v = el.getAttribute && el.getAttribute(ATTRS[i]);
      if (v && v.length <= 80) {
        const s = tag + "[" + ATTRS[i] + "=" + quoted(v) + "]";
        if (unique(el, s)) return s;
      }
    }
    const parts = [];
    let node = el;
    while (node && node.nodeType === Node.ELEMENT_NODE) {
      if (node !== el && node.id && !madeUp(node.id) && unique(node, "#" + CSS.escape(node.id))) {
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
  const sizeOf = (el) => {
    const r = el.getBoundingClientRect();
    return Math.round(r.width) + "×" + Math.round(r.height);
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

  // Computed values worth an agent's time, each left out at its default.
  const STYLE_PROPS = [
    ["position", "static"],
    ["z-index", "auto"],
    ["overflow", "visible"],
    ["opacity", "1"],
    ["flex-direction", "row"],
    ["justify-content", "normal"],
    ["align-items", "normal"],
    ["gap", "normal"],
    ["grid-template-columns", "none"],
    ["padding", "0px"],
    ["margin", "0px"],
    ["border-radius", "0px"],
    ["font-family"],
    ["font-size", "16px"],
    ["font-weight", "400"],
    ["line-height", "normal"],
    ["letter-spacing", "normal"],
    ["text-align", "start"],
    ["text-transform", "none"],
    ["color", "rgb(0, 0, 0)"],
    ["background-color", "transparent", "rgba(0, 0, 0, 0)"],
    ["background-image", "none"],
    ["box-shadow", "none"],
    ["transform", "none"],
  ];
  const stylesOf = (el) => {
    const s = getComputedStyle(el);
    const out = [["display", s.getPropertyValue("display")]];
    for (const [name, ...defaults] of STYLE_PROPS) {
      const v = s.getPropertyValue(name);
      if (v && !defaults.includes(v)) out.push([name, clip(oneLine(v), 160)]);
      // The border, after the radius, when there is one.
      if (name === "border-radius" && s.getPropertyValue("border-top-style") !== "none") {
        const w = s.getPropertyValue("border-width");
        if (w && w !== "0px") out.push(["border", oneLine(w + " " + s.getPropertyValue("border-style") + " " + s.getPropertyValue("border-color"))]);
      }
    }
    return out;
  };
  // The attributes that name or wire an element; never `value`, which can hold what was typed.
  const KEY_ATTR = /^(?:data-(?:testid|test-id|test|cy|qa|component|track)|aria-[a-z-]+|role|href|src|alt|name|type|title|placeholder|for|action|method|target|rel|disabled|required|readonly|checked)$/;
  const attributesOf = (el) => {
    const out = [];
    const list = el.attributes || [];
    for (let i = 0; i < list.length && out.length < 16; i++) {
      const a = list[i];
      if (a && KEY_ATTR.test(a.name)) out.push([a.name, clip(oneLine(a.value), 200)]);
    }
    return out;
  };
  const parentOf = (el) => {
    if (el.parentElement) return el.parentElement;
    const root = el.getRootNode && el.getRootNode();
    return root && root instanceof ShadowRoot ? root.host : null;
  };
  // `body > main > div.actions > button#buy`: at most ten levels, the middle ones cut.
  const pathOf = (el) => {
    const parts = [];
    for (let n = el; n && n.nodeType === Node.ELEMENT_NODE && parts.length < 60; n = parentOf(n)) {
      parts.unshift(short(n));
      if (n.localName === "body") break;
    }
    if (parts.length > 10) parts.splice(3, parts.length - 9, "…");
    return parts.join(" > ");
  };
  const SKIP = new Set(["script", "style", "template", "link", "meta", "noscript", HOST_TAG]);
  const sibling = (el, forward) => {
    let n = forward ? el.nextElementSibling : el.previousElementSibling;
    while (n && SKIP.has(n.localName)) n = forward ? n.nextElementSibling : n.previousElementSibling;
    return n;
  };
  const neighbour = (n) => {
    if (!n) return null;
    const text = clip(oneLine(n.innerText !== undefined ? n.innerText : n.textContent), 80);
    return { label: short(n), text: text || null };
  };
  const contextOf = (el) => {
    const parent = parentOf(el);
    return {
      parent: parent ? { label: short(parent), text: null } : null,
      before: neighbour(sibling(el, false)),
      after: neighbour(sibling(el, true)),
    };
  };
  const typeName = (t) => (t && (typeof t === "function" || typeof t === "object") ? t.displayName || t.name || t.__name || "" : "");
  // A React component's name: its function or class, through `memo` and `forwardRef`.
  const componentName = (t) => typeName(t) || (t && (typeName(t.type) || typeName(t.render))) || "";
  // React 19 has no `_debugSource`: the JSX call's place is the first frame of `_debugStack`
  // that isn't React's own.
  const stackSource = (stack) => {
    const text = stack && typeof stack.stack === "string" ? stack.stack : "";
    const lines = text.split("\n");
    for (let i = 0; i < lines.length; i++) {
      const m = /((?:https?|file|webpack|webpack-internal):\/\/[^\s)]+?):(\d+)(?::\d+)?\)?\s*$/.exec(lines[i]);
      if (!m || /node_modules|react-dom|react\.|jsx-dev-runtime|chunk-|\.vite\/deps/.test(m[1])) continue;
      let file = m[1];
      try {
        const u = new URL(file);
        if (u.origin === location.origin) file = u.pathname.replace(/^\/(?!@fs\/)/, "");
      } catch (_) {}
      return { file, line: Number(m[2]) || null };
    }
    return null;
  };
  const reactHint = (el) => {
    const key = Object.keys(el).find((k) => k.startsWith("__reactFiber$") || k.startsWith("__reactInternalInstance$"));
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
      if (!file && f._debugStack) {
        const at = stackSource(f._debugStack);
        if (at) {
          file = at.file;
          line = at.line;
        }
      }
      if (!component && f.type && typeof f.type !== "string") component = componentName(f.type);
      if (file && component) break;
    }
    if (!file && !component) return null;
    return { framework: "React", file, line, component: component || null };
  };
  // `file:line[:col]`, as dev plugins write it into an attribute.
  const fileLine = (text, framework, component) => {
    const m = /^(.*?)(?::(\d+))?(?::\d+)?$/.exec(text);
    return { framework, file: m[1], line: m[2] ? Number(m[2]) : null, component: component || null };
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
    if (!el.getAttribute) return null;
    const get = (name) => el.getAttribute(name);
    if (get("data-astro-source-file")) {
      const at = (get("data-astro-source-loc") || "").split(":")[0];
      return { framework: "Astro", file: get("data-astro-source-file"), line: Number(at) || null, component: null };
    }
    if (get("data-v-inspector")) return fileLine(get("data-v-inspector"), "Vue", null);
    if (get("data-sentry-source-file")) {
      return { framework: "source", file: get("data-sentry-source-file"), line: null, component: get("data-sentry-component") || null };
    }
    if (get("data-source")) return fileLine(get("data-source"), "source", null);
    const ip = get("data-inspector-relative-path");
    if (ip) {
      const ln = Number(get("data-inspector-line"));
      return { framework: "source", file: ip, line: ln || null, component: null };
    }
    if (get("data-component")) return { framework: "source", file: "", line: null, component: get("data-component") };
    return null;
  };
  const sourceOf = (el) => {
    try {
      for (let n = el; n && n.nodeType === Node.ELEMENT_NODE; n = n.parentElement) {
        const h = hintOf(n);
        if (h) return h;
      }
      // Any build: the nearest custom element names the component.
      for (let n = el; n && n.nodeType === Node.ELEMENT_NODE; n = parentOf(n)) {
        if (n.localName.includes("-") && n.localName !== HOST_TAG) return { framework: "custom element", file: "", line: null, component: n.localName };
      }
    } catch (_) {}
    return null;
  };
  const details = (el) => {
    const r = el.getBoundingClientRect();
    const rect = { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) };
    const sx = window.scrollX || 0;
    const sy = window.scrollY || 0;
    let accessible = null;
    let name = null;
    try {
      accessible = role(el);
      name = accessible ? clip(nameOf(el, accessible), 200) || null : null;
    } catch (_) {}
    return {
      ...info(el, 500),
      rect,
      styles: stylesOf(el),
      source: sourceOf(el),
      html: (el.outerHTML || "").slice(0, 20000),
      off_screen: r.left < 0 || r.top < 0 || r.right > innerWidth || r.bottom > innerHeight,
      path: pathOf(el),
      attributes: attributesOf(el),
      role: accessible,
      name,
      page_rect: { x: Math.round(r.x + sx), y: Math.round(r.y + sy), width: rect.width, height: rect.height },
      viewport: [Math.round(innerWidth), Math.round(innerHeight)],
      context: contextOf(el),
    };
  };
  // What the popover's second line says: `button “Buy” · 120×40`.
  const summary = (el) => {
    let r = null;
    let name = "";
    try {
      r = role(el);
      name = r ? clip(nameOf(el, r), 40) : "";
    } catch (_) {}
    const who = r ? r + (name ? " “" + name + "”" : "") + " · " : "";
    return who + sizeOf(el);
  };

  // ---- Placement ----

  // Where a floating box of `size` goes beside `anchor` (both in viewport px) inside a
  // viewport of `view`: below it, else above it when that fits and below doesn't (flip), at
  // the anchor's left edge moved inside the viewport (shift). An anchor with room on neither
  // side (one taller than the viewport) gets the box inside its visible part.
  const place = (anchor, size, view, gap = 8, margin = 8) => {
    const clamp = (v, lo, hi) => Math.max(lo, Math.min(v, Math.max(lo, hi)));
    const below = view.height - anchor.bottom - gap - margin;
    const above = anchor.top - gap - margin;
    let side;
    let top;
    if (size.height <= below) {
      side = "below";
      top = anchor.bottom + gap;
    } else if (size.height <= above) {
      side = "above";
      top = anchor.top - gap - size.height;
    } else {
      side = "inside";
      top = Math.max(anchor.top, 0) + margin;
    }
    top = clamp(top, margin, view.height - size.height - margin);
    const left = clamp(anchor.left, margin, view.width - size.width - margin);
    return { left: Math.round(left), top: Math.round(top), side };
  };
  helpers.place = place;

  // ---- Drawing ----

  const drawHighlight = (el, label) => {
    if (!el || !el.isConnected) return;
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    const n = (name) => parseFloat(s[name]) || 0;
    const m = { t: n("marginTop"), r: n("marginRight"), b: n("marginBottom"), l: n("marginLeft") };
    const bw = { t: n("borderTopWidth"), r: n("borderRightWidth"), b: n("borderBottomWidth"), l: n("borderLeftWidth") };
    const p = { t: n("paddingTop"), r: n("paddingRight"), b: n("paddingBottom"), l: n("paddingLeft") };
    const at = (x, y, w, h) => "left:" + px(x) + ";top:" + px(y) + ";width:" + px(Math.max(w, 0)) + ";height:" + px(Math.max(h, 0)) + ";";
    box(at(r.left - m.l, r.top - m.t, r.width + m.l + m.r, r.height + m.t + m.b) + "background:rgba(246,178,107,.3)");
    const px0 = r.left + bw.l;
    const py0 = r.top + bw.t;
    const pw = r.width - bw.l - bw.r;
    const ph = r.height - bw.t - bw.b;
    box(at(px0, py0, pw, ph) + "background:rgba(147,196,125,.3)");
    box(at(r.left, r.top, r.width, r.height) + "outline:2px solid var(--primary);outline-offset:-1px");
    if (!label) return;
    const chip = make("div");
    attr(chip, "class", "label");
    const name = make("span");
    write(name, short(el));
    const size = make("span");
    attr(size, "class", "size");
    write(size, sizeOf(el));
    put(chip, name);
    put(chip, size);
    css(chip, "left:" + px(Math.max(Math.min(r.left, innerWidth - 120), 4)) + ";top:" + px(r.top < 28 ? Math.min(r.bottom + 6, innerHeight - 26) : r.top - 26));
    put(layer, chip);
    drawn[drawn.length] = chip;
  };
  // Each marker sits on its element's top-left corner, kept inside the viewport.
  const placeMarkers = () => {
    for (let i = 0; i < marks.length; i++) {
      const m = marks[i];
      let el = null;
      try {
        el = resolve({ selector: m.item.selector });
      } catch (_) {}
      if (!el) {
        prop(m.node, "display", "none");
        continue;
      }
      const r = el.getBoundingClientRect();
      const out = r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth;
      prop(m.node, "display", out ? "none" : "block");
      prop(m.node, "left", px(Math.max(2, Math.min(r.left - 12, innerWidth - 28))));
      prop(m.node, "top", px(Math.max(2, Math.min(r.top - 12, innerHeight - 28))));
      attr(m.node, "class", pop && pop.n === m.item.n ? "mark open" : "mark");
    }
  };
  const placePopover = () => {
    if (!pop || !ui) return;
    if (!pop.el.isConnected) return close();
    const r = pop.el.getBoundingClientRect();
    const b = boxOf(ui.pop);
    const at = place(r, { width: b.width || 320, height: b.height || 200 }, { width: innerWidth, height: innerHeight });
    prop(ui.pop, "left", at.left + "px");
    prop(ui.pop, "top", at.top + "px");
    prop(ui.pop, "--rise", at.side === "above" ? "-4px" : "4px");
  };
  const drawMeasure = () => {
    if (mode !== "inspect" || !altHeld || !pinned || !pinned.isConnected || !current || current === pinned) return;
    const a = pinned.getBoundingClientRect();
    const b = current.getBoundingClientRect();
    box("left:" + px(a.left) + ";top:" + px(a.top) + ";width:" + px(a.width) + ";height:" + px(a.height) +
      ";outline:1px dashed " + MEASURE + ";outline-offset:-1px");
    const tag = (x, y, d) => {
      box("left:" + px(x) + ";top:" + px(y) + ";transform:translate(-50%,-50%);padding:1px 4px;border-radius:3px;background:" +
        MEASURE + ";color:#fff;font:10px/13px system-ui,sans-serif;white-space:nowrap", Math.round(d * 10) / 10 + "px");
    };
    const hLine = (x1, x2, y) => {
      if (x2 - x1 < 0.5) return;
      box("left:" + px(x1) + ";top:" + px(y) + ";width:" + px(x2 - x1) + ";height:1px;background:" + MEASURE);
      tag((x1 + x2) / 2, y, x2 - x1);
    };
    const vLine = (y1, y2, x) => {
      if (y2 - y1 < 0.5) return;
      box("left:" + px(x) + ";top:" + px(y1) + ";width:1px;height:" + px(y2 - y1) + ";background:" + MEASURE);
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
    if (!host && items.length === 0 && mode === "off" && !pop) return;
    if (!ensureHost()) return;
    for (let i = 0; i < drawn.length; i++) drop(drawn[i]);
    drawn = [];
    if (stale) syncMarkers();
    placeMarkers();
    if (pop) drawHighlight(pop.el, false);
    else if (mode !== "off") drawHighlight(current, true);
    drawMeasure();
    placePopover();
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
    const want = items.length > 0 || mode !== "off" || !!pop;
    if (want === watching) return;
    watching = want;
    const act = want ? listen : unlisten;
    act("scroll", schedule, SCROLLS);
    act("resize", schedule, PASSIVE);
    if (want) timer = setInterval(schedule, 500);
    else clearInterval(timer);
  };

  if (document.readyState === "complete") {
    setTimeout(() => {
      ready = true;
      render();
    }, 1000);
  } else {
    listen("load", () => {
      setTimeout(() => {
        ready = true;
        render();
      }, 1000);
    }, ONCE);
  }

  // ---- Markers ----

  const markerAction = (node) => {
    for (let i = 0; i < marks.length; i++) if (marks[i].node === node) return marks[i];
    return null;
  };
  // The markers for `items`: kept where the number stays, so a focused one keeps the keyboard.
  const syncMarkers = () => {
    if (!ensureHost()) return;
    stale = false;
    const kept = [];
    for (let i = 0; i < marks.length; i++) {
      let still = null;
      for (let j = 0; j < items.length; j++) if (items[j].n === marks[i].item.n) still = items[j];
      if (still) {
        marks[i].item = still;
        kept[kept.length] = marks[i];
      } else drop(marks[i].node);
    }
    for (let j = 0; j < items.length; j++) {
      let has = false;
      for (let i = 0; i < kept.length; i++) if (kept[i].item.n === items[j].n) has = true;
      if (has) continue;
      const node = make("button");
      attr(node, "type", "button");
      attr(node, "class", "mark");
      write(node, String(items[j].n));
      put(rootNode, node);
      kept[kept.length] = { node, item: items[j] };
    }
    for (let i = 0; i < kept.length; i++) {
      const it = kept[i].item;
      const what = it.comment ? clip(oneLine(it.comment), 80) : it.label || "";
      attr(kept[i].node, "aria-label", "Annotation " + it.n + (what ? ": " + what : "") + ". Enter opens it, Delete removes it.");
      attr(kept[i].node, "title", "Annotation " + it.n + (what ? ": " + what : ""));
    }
    marks = kept;
  };

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
      // and, as anywhere else, stops picking.
      preventNow(e);
      if (active && N.blur) apply(N.blur, active, []);
      if (mode !== "off") {
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

  // ---- Agents: the cursors of the agents that drive the page ----
  //
  // Each agent Sawit drives the page for has a pointer in its colour and a pill with its name and
  // what it does, in the overlay's own layer. They take no event (`pointer-events: none`, so
  // `elementFromPoint` passes them by and a click lands where it would), change no layout (the
  // host is fixed over the viewport), are hidden from assistive technology (the app announces
  // what agents do), and hide with the rest of the overlay for a screenshot. Sawit sends the
  // cursor's op just before the action's, so the pointer glides to the target as the action
  // happens; it adds no more than the glide to the action. Only Sawit's call, signed with the
  // token, draws one: a page can't draw a cursor that claims to be an agent's.

  const AGENT_FADE = 5000;
  const AGENT_GONE = 30000;
  // How long the target's ring and a click's ripple stay after the pointer arrives.
  const AGENT_SETTLE = 900;
  const AGENT_COLOR = /^#[0-9a-f]{6}$/i;
  // `[{ id, node, name, doing, typing, x, y, fade, gone }]`, read by index.
  let agentCursors = [];
  // What an agent's op does, in words, by the action's name: `doing|done`.
  const AGENT_VERBS = {
    click: "clicking|clicked",
    hover: "pointing at|pointed at",
    type: "typing in|typed in",
    select: "choosing|chose",
    scroll: "scrolling|scrolled",
    press: "pressing|pressed",
  };
  // The element as the agent's caption names it: its accessible name, else its role or tag.
  const agentTargetName = (el) => {
    if (!el) return "";
    let name = "";
    let r = null;
    try {
      r = role(el);
      name = oneLine(nameOf(el, r) || "");
    } catch (_) {}
    return clip(name || r || el.localName || "", 40);
  };
  // What the agent does with `op` on `el`, in words: "clicking Sign up", "typing in Email",
  // "choosing Large in Size", "pressing Enter", "scrolling".
  const agentAction = (action, el, op) => {
    const verb = (AGENT_VERBS[action] || "working on|worked on").split("|");
    const name = agentTargetName(el);
    let object = name;
    if (action === "press") object = clip(String(op.key || "a key"), 24);
    else if (action === "select") {
      const values = clip((op.values || []).map(String).join(", "), 30);
      object = values && name ? values + " in " + name : values || name;
    } else if (action === "scroll") object = el ? name : "";
    const words = (v) => (object ? v + " " + object : v.replace(/ (in|at|on)$/, ""));
    return { doing: words(verb[0]), done: words(verb[1]) };
  };
  helpers.agentAction = agentAction;
  // Where the pointer goes for `el`: its centre in the viewport, kept inside the viewport.
  const agentAim = (el) => {
    const r = el.getBoundingClientRect();
    const x = Math.min(Math.max(r.left + r.width / 2, 0), innerWidth - 1);
    const y = Math.min(Math.max(r.top + r.height / 2, 0), innerHeight - 1);
    return { x: Math.round(x), y: Math.round(y), r };
  };
  // Where the agent's caret goes in a text field before its text lands: after the text the field
  // holds, in its content box, as tall as a line.
  const agentCaret = (el, r) => {
    let left = r.left + 4;
    let top = r.top + 4;
    let height = Math.max(r.height - 8, 10);
    try {
      const s = getComputedStyle(el);
      const n = (v) => parseFloat(v) || 0;
      const size = n(s.fontSize) || 13;
      const padL = n(s.paddingLeft) + n(s.borderLeftWidth);
      const text = el.isContentEditable ? el.textContent || "" : String(el.value || "");
      let width = text.length * size * 0.55;
      try {
        const ctx = make("canvas").getContext("2d");
        ctx.font = s.font || size + "px sans-serif";
        width = ctx.measureText(text).width;
      } catch (_) {}
      height = Math.min(Math.max(size * 1.3, 10), r.height - 4);
      left = r.left + padL + Math.min(width, Math.max(r.width - padL * 2, 0));
      top = r.top + (r.height - height) / 2;
    } catch (_) {}
    return { left, top, height };
  };
  // After `css`, which sets the whole inline style, the colour of what an agent draws.
  const agentPaint = (node, color) => prop(node, "--c", AGENT_COLOR.test(color || "") ? color : "#2563eb");
  // Drops a node of the agents' layer after `ms`.
  const dropLater = (node, ms) => setTimeout(() => drop(node), ms);
  const agentOf = (id) => {
    for (let i = 0; i < agentCursors.length; i++) if (agentCursors[i].id === id) return agentCursors[i];
    return null;
  };
  const forgetAgent = (a) => {
    clearTimeout(a.fade);
    clearTimeout(a.gone);
    drop(a.node);
    const kept = [];
    for (let i = 0; i < agentCursors.length; i++) if (agentCursors[i] !== a) kept[kept.length] = agentCursors[i];
    agentCursors = kept;
  };
  // The cursor of agent `who` (`{id, name, color}`), made the first time it acts.
  const agentCursor = (who) => {
    let a = agentOf(who.id);
    if (a) return a;
    const node = make("div");
    attr(node, "class", "agent");
    const tip = make("div");
    attr(tip, "class", "tip");
    const arrow = make("div");
    attr(arrow, "class", "arrow");
    put(tip, arrow);
    const pill = make("div");
    attr(pill, "class", "pill");
    const name = make("span");
    attr(name, "class", "name");
    const typing = make("span");
    attr(typing, "class", "typing");
    const doing = make("span");
    attr(doing, "class", "doing");
    put(pill, name);
    put(pill, typing);
    put(pill, doing);
    put(node, tip);
    put(node, pill);
    put(agentsLayer, node);
    a = { id: who.id, node, name, doing, typing, x: null, y: null, fade: 0, gone: 0 };
    agentCursors[agentCursors.length] = a;
    return a;
  };
  // An agent acts: its pointer glides to the target (`lead` ms, Sawit's head start for the
  // action; jumps under reduced motion or the first time), the target is ringed in its colour,
  // a click ripples where the pointer lands, and a field it types into shows its caret first.
  // The cursor fades after a few seconds idle and goes after 30 s.
  const presence = (op) => {
    if (op.token !== TOKEN) return { shown: false };
    if (op.action === "clear") {
      while (agentCursors.length) forgetAgent(agentCursors[0]);
      return { shown: false };
    }
    const who = op.agent || {};
    if (typeof who.id !== "string" || !who.id) throw new Error("An agent's cursor needs the agent's id.");
    let el = null;
    try {
      el = op.action === "press" ? document.activeElement : resolve(op);
    } catch (_) {}
    if (el && (el === document.body || el === document.documentElement)) el = null;
    // The action scrolls its element into view first; the cursor goes where it will be.
    if (el && el.scrollIntoView && (op.action === "click" || op.action === "hover")) {
      el.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
    } else if (el && el.scrollIntoView && (op.action === "type" || (op.action === "scroll" && !op.dx && !op.dy))) {
      el.scrollIntoView({ block: "center", behavior: "instant" });
    }
    const words = agentAction(op.action, el, op);
    if (!ensureHost(true) || !agentsLayer) return { shown: false, doing: words.doing, done: words.done };
    const a = agentCursor(who);
    const lead = Math.max(0, Math.min(Number(op.lead) || 0, 150));
    const aim = el ? agentAim(el) : { x: a.x === null ? Math.round(innerWidth / 2) : a.x, y: a.y === null ? Math.round(innerHeight / 2) : a.y, r: null };
    const first = a.x === null;
    agentPaint(a.node, who.color);
    prop(a.node, "--glide", (first ? 0 : lead) + "ms");
    prop(a.node, "transform", "translate(" + aim.x + "px," + aim.y + "px)");
    const side = (aim.x > innerWidth - 220 ? " west" : "") + (aim.y > innerHeight - 48 ? " north" : "");
    attr(a.node, "class", "agent" + side);
    write(a.name, String(who.name || "Agent").slice(0, 40));
    write(a.doing, words.doing);
    write(a.typing, op.action === "type" ? "▍" : "");
    a.x = aim.x;
    a.y = aim.y;
    const settle = (first ? 0 : lead) + AGENT_SETTLE;
    if (aim.r) {
      const ring = make("div");
      attr(ring, "class", "ring");
      css(ring, "left:" + px(aim.r.left) + ";top:" + px(aim.r.top) + ";width:" + px(aim.r.width) + ";height:" + px(aim.r.height));
      agentPaint(ring, who.color);
      put(agentsLayer, ring);
      dropLater(ring, settle);
    }
    if (op.action === "click" && aim.r) {
      const ripple = make("div");
      attr(ripple, "class", "ripple");
      css(ripple, "left:" + px(aim.x) + ";top:" + px(aim.y) + ";--delay:" + (first ? 0 : lead) + "ms");
      agentPaint(ripple, who.color);
      put(agentsLayer, ripple);
      dropLater(ripple, settle);
    }
    if (op.action === "type" && aim.r) {
      const at = agentCaret(el, aim.r);
      const caret = make("div");
      attr(caret, "class", "caret");
      css(caret, "left:" + px(at.left) + ";top:" + px(at.top) + ";height:" + px(at.height));
      agentPaint(caret, who.color);
      put(agentsLayer, caret);
      dropLater(caret, settle);
    }
    clearTimeout(a.fade);
    clearTimeout(a.gone);
    a.fade = setTimeout(() => attr(a.node, "class", "agent faded" + side), AGENT_FADE);
    a.gone = setTimeout(() => forgetAgent(a), AGENT_GONE);
    return { shown: true, doing: words.doing, done: words.done, x: aim.x, y: aim.y };
  };

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
}
