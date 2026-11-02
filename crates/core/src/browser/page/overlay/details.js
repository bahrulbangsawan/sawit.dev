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
