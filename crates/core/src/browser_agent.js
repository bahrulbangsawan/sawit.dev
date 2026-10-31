// Sawit's browser agent, injected into every page of the browser tab before
// the page's own scripts (see crates/core/src/browser.rs).
//
// - Reports console messages, page errors, requests and URL changes to the
//   app over IPC, each message signed with the webview's token (held in this
//   closure, out of the page's reach).
// - Answers the agent's ops through `window.__sawitAgent.run(op)`, which
//   returns a JSON string synchronously: `{"ok":true,…}` or
//   `{"ok":false,"error":…}`.
(() => {
  if (window.__sawitAgent || !window.ipc || !window.ipc.postMessage) return;
  const TOKEN = "__SAWIT_TOKEN__";
  const post = window.ipc.postMessage.bind(window.ipc);
  const send = (kind, data) => {
    try {
      post(TOKEN + "\n" + kind + "\n" + JSON.stringify(data));
    } catch (_) {}
  };
  const now = () => performance.now();
  const clip = (text, max) =>
    text.length > max ? text.slice(0, max) + "… (" + text.length + " chars)" : text;
  const oneLine = (text) => (text || "").replace(/\s+/g, " ").trim();
  const absolute = (url) => {
    try {
      return new URL(url, location.href).href;
    } catch (_) {
      return String(url);
    }
  };

  // ---- Console and page errors ----

  const format = (value) => {
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.stack || String(value);
    if (value instanceof Element) return describe(value);
    try {
      const json = JSON.stringify(value);
      return json === undefined ? String(value) : json;
    } catch (_) {
      return String(value);
    }
  };
  const LEVELS = { log: "log", info: "info", warn: "warning", error: "error", debug: "debug", trace: "debug" };
  for (const name of Object.keys(LEVELS)) {
    const original = console[name];
    if (typeof original !== "function") continue;
    console[name] = function (...args) {
      send("console", { level: LEVELS[name], text: clip(args.map(format).join(" "), 4000), time_ms: now() });
      return original.apply(this, args);
    };
  }
  const failedResources = new Set();
  addEventListener(
    "error",
    (event) => {
      const target = event.target;
      if (target && target !== window && target instanceof Element) {
        const url = target.currentSrc || target.src || target.href;
        if (url) {
          failedResources.add(url);
          send("network", { method: "GET", url: String(url), kind: target.localName, error: "Failed to load", time_ms: now() });
        }
        return;
      }
      const text = (event.error && event.error.stack) || event.message || "Script error";
      send("console", { level: "error", text: clip(text, 4000), url: event.filename || undefined, line: event.lineno || undefined, time_ms: now() });
    },
    true,
  );
  addEventListener("unhandledrejection", (event) => {
    send("console", { level: "error", text: clip("Unhandled promise rejection: " + format(event.reason), 4000), time_ms: now() });
  });

  // ---- Network ----

  const TEXTUAL = /json|text|xml|javascript|graphql|x-www-form-urlencoded/i;
  const bodyText = (body) => {
    if (body == null) return undefined;
    if (typeof body === "string") return clip(body, 2048);
    if (body instanceof URLSearchParams) return clip(body.toString(), 2048);
    if (body instanceof FormData) {
      const parts = [];
      for (const [key, value] of body.entries()) parts.push(key + "=" + (typeof value === "string" ? value : "[file " + value.name + "]"));
      return clip(parts.join("&"), 2048);
    }
    return "[" + (body.constructor ? body.constructor.name : "binary") + "]";
  };
  const originalFetch = window.fetch;
  if (originalFetch) {
    window.fetch = function (input, init) {
      const start = now();
      const request = input instanceof Request ? input : null;
      const method = String((init && init.method) || (request && request.method) || "GET").toUpperCase();
      const url = absolute(request ? request.url : String(input));
      const entry = { method, url, kind: "fetch", request_body: bodyText(init && init.body), time_ms: start };
      const promise = originalFetch.apply(this, arguments);
      promise.then(
        (response) => {
          entry.status = response.status;
          const done = (body) => {
            if (body !== undefined) entry.response_body = clip(body, 4096);
            entry.duration_ms = now() - start;
            send("network", entry);
          };
          const type = response.headers.get("content-type") || "";
          const size = Number(response.headers.get("content-length") || 0);
          if (!TEXTUAL.test(type) || size > 1 << 20 || /event-stream/.test(type)) return done();
          // A body that never ends (a stream) is reported without it.
          const timer = setTimeout(() => done(), 3000);
          response
            .clone()
            .text()
            .then(
              (body) => {
                clearTimeout(timer);
                done(body);
              },
              () => {
                clearTimeout(timer);
                done();
              },
            );
        },
        (error) => {
          entry.error = String((error && error.message) || error);
          entry.duration_ms = now() - start;
          send("network", entry);
        },
      );
      return promise;
    };
  }
  const xhrs = new WeakMap();
  const xhrOpen = XMLHttpRequest.prototype.open;
  const xhrSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (method, url) {
    xhrs.set(this, { method: String(method).toUpperCase(), url: absolute(String(url)) });
    return xhrOpen.apply(this, arguments);
  };
  XMLHttpRequest.prototype.send = function (body) {
    const info = xhrs.get(this);
    if (info) {
      const start = now();
      const request_body = bodyText(body);
      this.addEventListener("loadend", () => {
        const entry = { method: info.method, url: info.url, kind: "xhr", request_body, time_ms: start, duration_ms: now() - start };
        if (this.status) {
          entry.status = this.status;
          if (this.responseType === "" || this.responseType === "text") entry.response_body = clip(this.responseText || "", 4096);
        } else {
          entry.error = "Request failed";
        }
        send("network", entry);
      });
    }
    return xhrSend.apply(this, arguments);
  };
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        if (e.initiatorType === "fetch" || e.initiatorType === "xmlhttprequest" || failedResources.has(e.name)) continue;
        send("network", { method: "GET", url: e.name, kind: e.initiatorType || "other", status: e.responseStatus || undefined, duration_ms: e.duration, time_ms: e.startTime });
      }
    }).observe({ type: "resource", buffered: true });
  } catch (_) {}

  // ---- URL changes without a load ----

  const notifyUrl = () => send("url", location.href);
  for (const name of ["pushState", "replaceState"]) {
    const original = history[name];
    history[name] = function () {
      const result = original.apply(this, arguments);
      notifyUrl();
      return result;
    };
  }
  addEventListener("popstate", notifyUrl);
  addEventListener("hashchange", notifyUrl);

  // ---- Elements ----

  let refs = new Map();
  let nextRef = 0;
  addEventListener("pagehide", () => (refs = new Map()));

  const deepQuery = (selector, root = document) => {
    const found = root.querySelector(selector);
    if (found) return found;
    for (const el of root.querySelectorAll("*")) {
      if (el.shadowRoot) {
        const inner = deepQuery(selector, el.shadowRoot);
        if (inner) return inner;
      }
    }
    return null;
  };

  const resolve = (op) => {
    if (op.ref) {
      const el = refs.get(op.ref);
      if (!el) throw new Error("No element " + op.ref + ". Take a new browser_snapshot (refs reset on every snapshot and navigation).");
      if (!el.isConnected) throw new Error("Element " + op.ref + " is no longer on the page. Take a new browser_snapshot.");
      return el;
    }
    if (op.selector) {
      let el;
      try {
        el = deepQuery(op.selector);
      } catch (_) {
        throw new Error("“" + op.selector + "” isn't a valid CSS selector.");
      }
      if (!el) throw new Error("Nothing on the page matches “" + op.selector + "”.");
      return el;
    }
    return null;
  };
  const required = (op) => {
    const el = resolve(op);
    if (!el) throw new Error("Give a ref (from browser_snapshot) or a CSS selector.");
    return el;
  };

  const INPUT_ROLES = {
    button: "button", submit: "button", reset: "button", image: "button", file: "button",
    checkbox: "checkbox", radio: "radio", range: "slider", number: "spinbutton", search: "searchbox",
    color: "button", hidden: null,
  };
  const TAG_ROLES = {
    button: "button", textarea: "textbox", h1: "heading", h2: "heading", h3: "heading", h4: "heading",
    h5: "heading", h6: "heading", nav: "navigation", main: "main", aside: "complementary", form: "form",
    ul: "list", ol: "list", menu: "list", li: "listitem", table: "table", tr: "row", th: "columnheader",
    td: "cell", dialog: "dialog", summary: "button", option: "option", article: "article",
    fieldset: "group", progress: "progressbar", meter: "meter", hr: "separator", iframe: "iframe",
    p: "paragraph", label: "label", video: "video", audio: "audio", search: "search",
  };
  const role = (el) => {
    const explicit = (el.getAttribute("role") || "").trim().split(/\s+/)[0];
    if (explicit && explicit !== "presentation" && explicit !== "none") return explicit;
    const tag = el.localName;
    if (tag === "a" || tag === "area") return el.hasAttribute("href") ? "link" : null;
    if (tag === "input") return el.type in INPUT_ROLES ? INPUT_ROLES[el.type] : "textbox";
    if (tag === "select") return el.multiple || el.size > 1 ? "listbox" : "combobox";
    if (tag === "img") return el.getAttribute("alt") === "" ? null : "img";
    if (tag === "svg") return el.getAttribute("aria-label") ? "img" : null;
    if (tag === "section") return el.getAttribute("aria-label") || el.getAttribute("aria-labelledby") ? "region" : null;
    if (tag === "header") return el.closest("article, aside, main, nav, section") ? null : "banner";
    if (tag === "footer") return el.closest("article, aside, main, nav, section") ? null : "contentinfo";
    if (el.isContentEditable && !(el.parentElement && el.parentElement.isContentEditable)) return "textbox";
    return TAG_ROLES[tag] || null;
  };
  const NAME_FROM_CONTENT = new Set([
    "button", "link", "heading", "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "option",
    "cell", "columnheader", "rowheader", "treeitem", "switch", "tooltip", "label", "paragraph",
  ]);
  const LEAF = new Set(["button", "link", "heading", "option", "img", "textbox", "searchbox", "checkbox", "radio", "slider", "spinbutton", "combobox", "switch", "progressbar", "meter", "separator", "iframe", "paragraph", "label"]);
  const textOf = (el) => oneLine(el.innerText !== undefined ? el.innerText : el.textContent);
  const nameOf = (el, r) => {
    const by = el.getAttribute("aria-labelledby");
    if (by) {
      const text = oneLine(by.split(/\s+/).map((id) => (document.getElementById(id) || {}).textContent || "").join(" "));
      if (text) return text;
    }
    const label = el.getAttribute("aria-label");
    if (label && label.trim()) return oneLine(label);
    if (el.labels && el.labels.length) {
      const text = oneLine([...el.labels].map((l) => l.textContent).join(" "));
      if (text) return text;
    }
    if (el.localName === "img" || el.localName === "area" || (el.localName === "input" && el.type === "image")) {
      if (el.alt) return oneLine(el.alt);
    }
    if (el.localName === "input" && ["button", "submit", "reset"].includes(el.type)) {
      return el.value || (el.type === "submit" ? "Submit" : el.type === "reset" ? "Reset" : "");
    }
    if (el.localName === "fieldset") {
      const legend = el.querySelector(":scope > legend");
      if (legend) return textOf(legend);
    }
    if (NAME_FROM_CONTENT.has(r)) {
      const text = textOf(el);
      if (text) return text;
    }
    return oneLine(el.getAttribute("title") || el.getAttribute("placeholder") || "");
  };
  const describe = (el) => {
    if (!(el instanceof Element)) return String(el);
    const r = role(el);
    const name = r ? clip(nameOf(el, r), 60) : "";
    if (r) return r + (name ? " “" + name + "”" : "");
    const id = el.id ? "#" + el.id : "";
    const cls = typeof el.className === "string" && el.className.trim() ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".") : "";
    return el.localName + id + cls;
  };
  const shown = (el) => {
    if (el.getAttribute("aria-hidden") === "true") return false;
    if (el.checkVisibility) return el.checkVisibility({ visibilityProperty: true });
    const style = getComputedStyle(el);
    return style.display !== "none" && style.visibility !== "hidden";
  };
  const clickable = (el) => {
    if (el.hasAttribute("onclick") || (el.tabIndex >= 0 && el.hasAttribute("tabindex"))) return true;
    const cursor = getComputedStyle(el).cursor;
    return cursor === "pointer" && !(el.parentElement && getComputedStyle(el.parentElement).cursor === "pointer");
  };
  const valueOf = (el) => {
    if (el.localName === "select") return [...el.selectedOptions].map((o) => o.label).join(", ");
    if (el.isContentEditable) return textOf(el);
    if (el.type === "password") return el.value ? "•".repeat(Math.min(el.value.length, 8)) : "";
    return el.value;
  };

  const MAX_LINES = 2000;
  const snapshot = () => {
    refs = new Map();
    nextRef = 0;
    const lines = [];
    let truncated = false;
    const focused = document.activeElement;
    const children = (node) => {
      if (node.localName === "slot") return node.assignedNodes({ flatten: true });
      if (node.shadowRoot) return node.shadowRoot.childNodes;
      if (node.localName === "iframe") return [];
      return node.childNodes;
    };
    const line = (depth, text) => {
      if (lines.length >= MAX_LINES) {
        truncated = true;
        return false;
      }
      lines.push("  ".repeat(depth) + "- " + text);
      return true;
    };
    const visit = (node, depth) => {
      if (truncated) return;
      if (node.nodeType === Node.TEXT_NODE) {
        const text = oneLine(node.textContent);
        if (text) line(depth, "text: " + JSON.stringify(clip(text, 200)));
        return;
      }
      if (node.nodeType !== Node.ELEMENT_NODE && node.nodeType !== Node.DOCUMENT_FRAGMENT_NODE) return;
      const el = node;
      if (el.nodeType === Node.ELEMENT_NODE) {
        if (["script", "style", "noscript", "template", "head", "meta", "link", "title"].includes(el.localName)) return;
        if (el.hasAttribute("data-sawit-overlay")) return;
        const contents = getComputedStyle(el).display === "contents";
        if (!contents && !shown(el)) return;
        let r = role(el);
        if (!r && !contents && clickable(el)) r = "clickable";
        if (r) {
          const ref = "e" + ++nextRef;
          refs.set(ref, el);
          const name = r === "clickable" ? clip(textOf(el), 100) : clip(nameOf(el, r), 100);
          let text = r + (name ? " " + JSON.stringify(name) : "");
          if (r === "heading") text += " [level=" + (Number(el.getAttribute("aria-level")) || Number(el.localName.slice(1)) || 2) + "]";
          if (["textbox", "searchbox", "combobox", "listbox", "spinbutton", "slider"].includes(r)) {
            text += " value=" + JSON.stringify(clip(valueOf(el) || "", 200));
          }
          if (el.checked || el.getAttribute("aria-checked") === "true") text += " [checked]";
          if (el.getAttribute("aria-expanded")) text += el.getAttribute("aria-expanded") === "true" ? " [expanded]" : " [collapsed]";
          if (el.getAttribute("aria-selected") === "true" || (r === "option" && el.selected)) text += " [selected]";
          if (el.disabled || el.getAttribute("aria-disabled") === "true") text += " [disabled]";
          if (el.required) text += " [required]";
          if (el === focused) text += " [focused]";
          if (r === "link") {
            const href = el.getAttribute("href") || "";
            if (href && !href.startsWith("javascript:")) text += " /url: " + clip(href, 120);
          }
          text += " [ref=" + ref + "]";
          if (LEAF.has(r) || r === "clickable") {
            line(depth, text);
            // Fields and buttons rarely hold controls; links and clickable
            // cards sometimes do.
            if (["link", "clickable", "label", "paragraph"].includes(r)) visitControlsOnly(el, depth + 1);
            return;
          }
          const at = lines.length;
          if (!line(depth, text + ":")) return;
          for (const child of children(el)) visit(child, depth + 1);
          // Nothing inside: no colon.
          if (lines.length === at + 1) lines[at] = lines[at].slice(0, -1);
          return;
        }
      }
      for (const child of children(el)) visit(child, depth);
    };
    // Only interactive descendants, for elements whose text is their name.
    const visitControlsOnly = (el, depth) => {
      for (const child of el.querySelectorAll("a[href], button, input, select, textarea, [role=button], [role=link], [role=checkbox], [tabindex]")) {
        if (truncated || !shown(child)) continue;
        const r = role(child) || "clickable";
        const ref = "e" + ++nextRef;
        refs.set(ref, child);
        const name = clip(nameOf(child, r), 100);
        line(depth, r + (name ? " " + JSON.stringify(name) : "") + " [ref=" + ref + "]");
      }
    };
    if (document.body) visit(document.body, 0);
    const dialog = document.querySelector("dialog[open]");
    return {
      url: location.href,
      title: document.title,
      ready: document.readyState,
      refs: nextRef,
      truncated,
      modal: dialog ? describe(dialog) : undefined,
      snapshot: lines.join("\n") + (truncated ? "\n… (cut at " + MAX_LINES + " lines; scroll or use a selector)" : ""),
    };
  };

  // ---- Pointer ----

  const pointer = (el, type, x, y, init) => {
    const Ctor = type.startsWith("pointer") && window.PointerEvent ? PointerEvent : MouseEvent;
    return el.dispatchEvent(
      new Ctor(type, {
        bubbles: !type.endsWith("enter") && !type.endsWith("leave"),
        cancelable: true,
        composed: true,
        view: window,
        clientX: x,
        clientY: y,
        pointerId: 1,
        pointerType: "mouse",
        isPrimary: true,
        ...init,
      }),
    );
  };
  const hitTest = (x, y) => {
    let hit = document.elementFromPoint(x, y);
    while (hit && hit.shadowRoot) {
      const inner = hit.shadowRoot.elementFromPoint(x, y);
      if (!inner || inner === hit) break;
      hit = inner;
    }
    return hit;
  };
  const composedContains = (outer, inner) => {
    for (let node = inner; node; node = node.parentNode || node.host) if (node === outer) return true;
    return false;
  };
  // Where a real click on `el` would land, after scrolling it into view.
  const aim = (el) => {
    el.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
    const rect = el.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) throw new Error(describe(el) + " has no size on screen.");
    const x = Math.min(Math.max(rect.left + rect.width / 2, 0), innerWidth - 1);
    const y = Math.min(Math.max(rect.top + rect.height / 2, 0), innerHeight - 1);
    const hit = hitTest(x, y);
    const label = hit && hit.closest && hit.closest("label");
    const ok = hit && (composedContains(el, hit) || composedContains(hit, el) || (label && label.control === el));
    if (!ok) throw new Error(describe(el) + " is covered by " + (hit ? describe(hit) : "nothing clickable") + ". Close or scroll past it, or click that element instead.");
    return { x, y, target: composedContains(el, hit) ? hit : el };
  };
  const focusable = (el) => {
    for (let node = el; node && node !== document.body; node = node.parentElement || (node.getRootNode && node.getRootNode().host)) {
      if (node.tabIndex >= 0 || node.isContentEditable) return node;
    }
    return null;
  };

  const click = (op) => {
    const el = required(op);
    if (el.disabled || el.getAttribute("aria-disabled") === "true") throw new Error(describe(el) + " is disabled.");
    const { x, y, target } = aim(el);
    const right = op.button === "right";
    const button = right ? 2 : 0;
    pointer(target, "pointerover", x, y, {});
    pointer(target, "pointerenter", x, y, {});
    pointer(target, "mouseover", x, y, {});
    pointer(target, "mouseenter", x, y, {});
    pointer(target, "pointermove", x, y, {});
    pointer(target, "mousemove", x, y, {});
    const times = op.double ? 2 : 1;
    for (let detail = 1; detail <= times; detail++) {
      pointer(target, "pointerdown", x, y, { button, buttons: right ? 2 : 1, detail });
      const focusIt = pointer(target, "mousedown", x, y, { button, buttons: right ? 2 : 1, detail });
      if (focusIt && detail === 1) {
        const f = focusable(target);
        if (f && f !== document.activeElement) f.focus({ preventScroll: true });
      }
      pointer(target, "pointerup", x, y, { button, detail });
      pointer(target, "mouseup", x, y, { button, detail });
      if (right) {
        pointer(target, "contextmenu", x, y, { button, detail });
        break;
      }
      pointer(target, "click", x, y, { button, detail });
    }
    if (op.double) pointer(target, "dblclick", x, y, { detail: 2 });
    return { clicked: describe(el), url: location.href };
  };

  const hover = (op) => {
    const el = required(op);
    const { x, y, target } = aim(el);
    for (const type of ["pointerover", "pointerenter", "mouseover", "mouseenter", "pointermove", "mousemove"]) pointer(target, type, x, y, {});
    return { hovered: describe(el), note: "Hover handlers ran; CSS :hover styles don't apply to simulated pointers." };
  };

  // ---- Keyboard and text ----

  const TEXT_INPUTS = new Set(["text", "search", "email", "url", "tel", "password", "number", ""]);
  const editable = (el) =>
    el.isContentEditable ||
    el.localName === "textarea" ||
    (el.localName === "input" && (TEXT_INPUTS.has(el.type) || ["date", "time", "datetime-local", "month", "week"].includes(el.type)));
  const setValue = (el, value) => {
    const proto = el.localName === "textarea" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
    el.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "insertText", data: value }));
  };
  const selectAll = (el) => {
    if (el.isContentEditable) {
      const range = document.createRange();
      range.selectNodeContents(el);
      const selection = getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    } else {
      try {
        el.select();
      } catch (_) {}
    }
  };
  const caretToEnd = (el) => {
    if (el.isContentEditable) {
      const range = document.createRange();
      range.selectNodeContents(el);
      range.collapse(false);
      const selection = getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    } else {
      try {
        el.setSelectionRange(el.value.length, el.value.length);
      } catch (_) {}
    }
  };
  const currentText = (el) => (el.isContentEditable ? el.textContent : el.value);

  const type = (op) => {
    const el = resolve(op) || document.activeElement;
    if (!el || el === document.body || el === document.documentElement) throw new Error("Nothing is focused. Give a ref or selector to type into.");
    if (!editable(el)) throw new Error(describe(el) + " doesn't take text.");
    if (el.disabled || el.readOnly) throw new Error(describe(el) + " is " + (el.disabled ? "disabled" : "read-only") + ".");
    el.scrollIntoView({ block: "center", behavior: "instant" });
    el.focus({ preventScroll: true });
    if (op.clear) {
      selectAll(el);
      if (!document.execCommand("delete") && !el.isContentEditable && el.value) setValue(el, "");
    } else {
      caretToEnd(el);
    }
    const before = currentText(el);
    let inserted = false;
    try {
      inserted = document.execCommand("insertText", false, op.text) && currentText(el) !== before;
    } catch (_) {}
    if (!inserted && op.text) {
      if (el.isContentEditable) el.textContent = before + op.text;
      else setValue(el, before + op.text);
    }
    if (!el.isContentEditable) el.dispatchEvent(new Event("change", { bubbles: true }));
    const result = { typed: op.text.length, into: describe(el), value: clip(valueOf(el) || "", 200) };
    if (op.submit) Object.assign(result, press({ key: "Enter" }));
    return result;
  };

  const KEY_CODES = {
    Enter: "Enter", Tab: "Tab", Escape: "Escape", Backspace: "Backspace", Delete: "Delete", " ": "Space",
    ArrowUp: "ArrowUp", ArrowDown: "ArrowDown", ArrowLeft: "ArrowLeft", ArrowRight: "ArrowRight",
    Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown",
  };
  const KEY_ALIASES = { esc: "Escape", return: "Enter", space: " ", up: "ArrowUp", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight", del: "Delete" };
  const tabbables = () =>
    [...document.querySelectorAll("a[href], button, input, select, textarea, [tabindex], [contenteditable]")].filter(
      (el) => el.tabIndex >= 0 && !el.disabled && shown(el) && el.getClientRects().length,
    );
  const press = (op) => {
    const parts = String(op.key).split("+");
    let key = parts.pop() || "+";
    if (key === "" && parts.length) key = "+";
    key = KEY_ALIASES[key.toLowerCase()] || (key.length === 1 ? key : key[0].toUpperCase() + key.slice(1));
    if (key.length > 1 && !KEY_CODES[key] && !/^F\d+$/.test(key)) throw new Error("Unknown key “" + op.key + "”. Use Enter, Tab, Escape, Backspace, Delete, Space, arrows, Home, End, PageUp, PageDown or one character.");
    const mods = new Set(parts.map((p) => p.toLowerCase()));
    const MODIFIERS = ["meta", "cmd", "command", "ctrl", "control", "alt", "option", "shift"];
    for (const mod of mods) {
      if (!MODIFIERS.includes(mod)) throw new Error("Unknown modifier “" + mod + "”. Use Meta, Ctrl, Alt or Shift.");
    }
    const meta = mods.has("meta") || mods.has("cmd") || mods.has("command");
    const ctrl = mods.has("ctrl") || mods.has("control");
    const alt = mods.has("alt") || mods.has("option");
    const shift = mods.has("shift");
    const target = document.activeElement || document.body;
    const code = KEY_CODES[key] || (/^[a-z]$/i.test(key) ? "Key" + key.toUpperCase() : /^\d$/.test(key) ? "Digit" + key : "");
    const init = { key, code, bubbles: true, cancelable: true, composed: true, metaKey: meta, ctrlKey: ctrl, altKey: alt, shiftKey: shift };
    const proceed = target.dispatchEvent(new KeyboardEvent("keydown", init));
    if (proceed) {
      const textField = editable(target);
      if (key.length === 1 && !meta && !ctrl) {
        if (target.dispatchEvent(new KeyboardEvent("keypress", init)) && textField) document.execCommand("insertText", false, key);
        else if (key === " " && (target.localName === "button" || target.type === "checkbox" || target.type === "radio")) target.click();
      } else if (key === "a" && meta) {
        if (textField) selectAll(target);
        else document.execCommand("selectAll");
      } else if (key === "Enter") {
        if (target.localName === "textarea" || target.isContentEditable) {
          document.execCommand("insertLineBreak");
        } else if (target.form && target.localName === "input") {
          target.form.requestSubmit();
        } else if (target.localName === "a" || target.localName === "button" || target.getAttribute("role") === "button") {
          target.click();
        }
      } else if (key === "Backspace" && textField) {
        document.execCommand("delete");
      } else if (key === "Delete" && textField) {
        document.execCommand("forwardDelete");
      } else if (key === "Tab") {
        const all = tabbables();
        const at = all.indexOf(target);
        const next = all[(at + (shift ? -1 : 1) + all.length) % all.length];
        if (next) next.focus();
      } else if (!textField && ["PageDown", "PageUp", "Home", "End", "ArrowDown", "ArrowUp", " "].includes(key)) {
        const page = innerHeight * 0.85;
        const by = { PageDown: page, PageUp: -page, ArrowDown: 40, ArrowUp: -40, " ": page }[key];
        if (key === "Home") scrollTo(scrollX, 0);
        else if (key === "End") scrollTo(scrollX, document.documentElement.scrollHeight);
        else scrollBy(0, by);
      }
    }
    target.dispatchEvent(new KeyboardEvent("keyup", init));
    return { pressed: op.key, focused: describe(document.activeElement || document.body), url: location.href };
  };

  const select = (op) => {
    const el = required(op);
    if (el.localName !== "select") throw new Error(describe(el) + " isn't a <select>; click it and pick the option instead.");
    const options = [...el.options];
    const wanted = op.values.map(String);
    const picked = wanted.map((w) => {
      const match =
        options.find((o) => o.value === w) ||
        options.find((o) => o.label.trim() === w.trim()) ||
        options.find((o) => o.label.trim().toLowerCase() === w.trim().toLowerCase());
      if (!match) throw new Error("No option “" + w + "”. Options: " + options.slice(0, 30).map((o) => JSON.stringify(o.label.trim())).join(", "));
      return match;
    });
    if (!el.multiple && picked.length > 1) throw new Error(describe(el) + " takes one option.");
    for (const o of options) o.selected = picked.includes(o);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
    return { selected: picked.map((o) => o.label.trim()) };
  };

  const scroll = (op) => {
    const el = resolve(op);
    const dx = Number(op.dx) || 0;
    const dy = Number(op.dy) || 0;
    if (el && !dx && !dy) {
      el.scrollIntoView({ block: "center", behavior: "instant" });
    } else {
      (el || document.scrollingElement || document.documentElement).scrollBy({ left: dx, top: dy, behavior: "instant" });
    }
    const page = document.scrollingElement || document.documentElement;
    return {
      scroll_x: Math.round(scrollX),
      scroll_y: Math.round(scrollY),
      page_height: page.scrollHeight,
      viewport_height: innerHeight,
      at_bottom: scrollY + innerHeight >= page.scrollHeight - 2,
    };
  };

  const query = (op) => {
    let found = true;
    if (op.text) found = found && !!document.body && document.body.innerText.toLowerCase().includes(String(op.text).toLowerCase());
    if (op.selector) {
      let el = null;
      try {
        el = deepQuery(op.selector);
      } catch (_) {
        throw new Error("“" + op.selector + "” isn't a valid CSS selector.");
      }
      found = found && !!el && shown(el);
    }
    return { found, url: location.href, title: document.title, ready: document.readyState };
  };

  const rect = (op) => {
    const el = required(op);
    el.scrollIntoView({ block: "center", behavior: "instant" });
    const r = el.getBoundingClientRect();
    const x = Math.max(r.left, 0);
    const y = Math.max(r.top, 0);
    return { x, y, width: Math.min(r.right, innerWidth) - x, height: Math.min(r.bottom, innerHeight) - y };
  };

  // ---- Scripts ----

  const settledResults = new Map();
  let nextPromise = 0;
  const toJson = (value) => {
    if (value === undefined) return null;
    const seen = new WeakSet();
    const text = JSON.stringify(value, (_, v) => {
      if (v instanceof Element) return "[" + describe(v) + "]";
      if (typeof v === "function") return "[function]";
      if (typeof v === "bigint") return v.toString();
      if (v && typeof v === "object") {
        if (seen.has(v)) return "[circular]";
        seen.add(v);
      }
      return v;
    });
    if (text === undefined) return String(value);
    return text.length > 20000 ? clip(text, 20000) : JSON.parse(text);
  };
  const evaluate = (op) => {
    let fn;
    try {
      fn = new Function("return (" + op.script + "\n);");
    } catch (_) {
      fn = new Function(op.script);
    }
    const value = fn.call(window);
    if (value && typeof value.then === "function") {
      const id = ++nextPromise;
      settledResults.set(id, { done: false });
      value.then(
        (v) => settledResults.set(id, { done: true, ok: true, value: v }),
        (e) => settledResults.set(id, { done: true, ok: false, error: String((e && e.message) || e) }),
      );
      return { pending: id };
    }
    return { value: toJson(value) };
  };
  const settled = (op) => {
    const state = settledResults.get(op.id);
    if (!state) throw new Error("That promise belongs to a page that has since gone; run the script again.");
    if (!state.done) return { done: false };
    settledResults.delete(op.id);
    if (!state.ok) throw new Error(state.error);
    return { done: true, value: toJson(state.value) };
  };

  // Page modules (see browser.rs `page_script`) add ops and share helpers.
  const modules = {};
  const helpers = {};
  /*__SAWIT_MODULES__*/

  const OPS = { snapshot, click, hover, type, press, select, scroll, query, rect, evaluate, settled, ...modules };
  const run = (op) => {
    try {
      const f = OPS[op && op.op];
      if (!f) throw new Error("Unknown op “" + (op && op.op) + "”.");
      return JSON.stringify(Object.assign({ ok: true }, f(op)));
    } catch (e) {
      return JSON.stringify({ ok: false, error: String((e && e.message) || e) });
    }
  };
  Object.defineProperty(window, "__sawitAgent", { value: Object.freeze({ run }), writable: false, configurable: false, enumerable: false });
})();
