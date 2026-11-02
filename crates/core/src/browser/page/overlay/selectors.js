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
  // `elements_at {points}`: the element under each point (`{x, y}`, CSS px from the page's top
  // left: mesurer keeps its marks where they are on the page) as Sawit's notes name one, its
  // selector and label; `null` for a point outside the viewport, or over no element of the page's
  // own. Sawit's overlay and mesurer's host are looked through: only the page's elements count.
  const elementsAt = (op) => {
    const points = Array.isArray(op.points) ? op.points.slice(0, 500) : [];
    const sx = window.scrollX || 0;
    const sy = window.scrollY || 0;
    const at = (p) => {
      const x = Number(p && p.x) - sx;
      const y = Number(p && p.y) - sy;
      if (!Number.isFinite(x) || !Number.isFinite(y) || x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return null;
      const under = document.elementsFromPoint(x, y);
      for (let i = 0; i < under.length; i++) {
        const el = under[i];
        if (el.localName === HOST_TAG || isMesurer(el)) continue;
        if (el === document.documentElement || el === document.body) return null;
        return { selector: selectorFor(el), label: short(el) };
      }
      return null;
    };
    return { elements: points.map(at) };
  };
