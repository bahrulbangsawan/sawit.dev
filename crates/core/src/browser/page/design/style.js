  // ---- style ----

  // Top-level commas only (not those inside :is(a, b)).
  const splitSelectors = (text) => {
    const out = [];
    let depth = 0;
    let cur = "";
    for (const ch of text || "") {
      if (ch === "(" || ch === "[") depth++;
      else if (ch === ")" || ch === "]") depth--;
      if (ch === "," && depth === 0) {
        out.push(cur.trim());
        cur = "";
      } else {
        cur += ch;
      }
    }
    if (cur.trim()) out.push(cur.trim());
    return out;
  };
  const STATE_RE = /:(hover|focus-visible|focus|active)(?![\w-])/g;
  // A longhand as the rule wrote it: `background: #0077ed` comes back as nine
  // longhands, so the shortest shorthand that serializes stands in for them.
  const declared = (st, prop) => {
    if (!prop.startsWith("--")) {
      const parts = prop.split("-");
      for (let n = 1; n < parts.length; n++) {
        const name = parts.slice(0, n).join("-");
        const value = st.getPropertyValue(name).trim();
        if (value) return [name, value];
      }
    }
    return [prop, st.getPropertyValue(prop).trim()];
  };
  const statesOf = (el) => {
    const found = new Map(STATES.map((s) => [s, new Map()]));
    const visit = (list) => {
      for (const rule of list) {
        try {
          if (rule.type === CSSRule.MEDIA_RULE) {
            const media = rule.conditionText || (rule.media && rule.media.mediaText) || "all";
            if (matchMedia(media).matches) visit(rule.cssRules);
          } else if (rule.type === CSSRule.IMPORT_RULE) {
            if (rule.styleSheet) visit(rule.styleSheet.cssRules);
          } else if (rule.type === CSSRule.STYLE_RULE) {
            const sel = rule.selectorText || "";
            if (sel.includes(":")) {
              for (const part of splitSelectors(sel)) {
                const states = new Set();
                let m;
                STATE_RE.lastIndex = 0;
                while ((m = STATE_RE.exec(part))) states.add(m[1]);
                if (!states.size) continue;
                let base = part.replace(STATE_RE, "").trim();
                if (!base || /[>+~]$/.test(base)) base += "*";
                let hit = false;
                try {
                  hit = el.matches(base);
                } catch (_) {}
                if (!hit) continue;
                const st = rule.style;
                for (const state of states) {
                  const into = found.get(state);
                  for (let i = 0; i < st.length; i++) {
                    const [name, value] = declared(st, st[i]);
                    into.set(name, value);
                  }
                }
              }
            }
            if (rule.cssRules && rule.cssRules.length) visit(rule.cssRules);
          } else if (rule.cssRules) {
            visit(rule.cssRules);
          }
        } catch (_) {}
      }
    };
    for (const { list } of pageSheets().readable) visit(list);
    for (const { list } of fetchedSheets) visit(list);
    return STATES.filter((s) => found.get(s).size).map((s) => [s, [...found.get(s)]]);
  };

  // The opaque color behind `el`, as rgb(): walks up through transparent and
  // translucent backgrounds, compositing the translucent ones over what's
  // below, on white.
  const effectiveBackground = (el) => {
    const layers = [];
    let base = null;
    for (let node = el; node && node.nodeType === Node.ELEMENT_NODE; node = node.parentElement || (node.getRootNode && node.getRootNode().host) || null) {
      const c = getComputedStyle(node).backgroundColor;
      if (transparent(c)) continue;
      // Read in any syntax (oklch, lab, color(display-p3 …)); one the browser can't read is passed over.
      const p = rgbaOf(c);
      if (!p) continue;
      if (p[3] >= 1) {
        base = p;
        break;
      }
      layers.push(p);
    }
    let [r, g, b] = base || [255, 255, 255];
    for (let i = layers.length - 1; i >= 0; i--) {
      const [lr, lg, lb, a] = layers[i];
      r = lr * a + r * (1 - a);
      g = lg * a + g * (1 - a);
      b = lb * a + b * (1 - a);
    }
    return "rgb(" + Math.round(r) + ", " + Math.round(g) + ", " + Math.round(b) + ")";
  };

  const labelOf = (el) => {
    const cls = classOf(el).trim() ? "." + classOf(el).trim().split(/\s+/).slice(0, 2).join(".") : "";
    return el.localName + (el.id ? "#" + el.id : "") + cls;
  };
  const style = (op) => {
    const el = resolve(op);
    if (!el) throw new Error("Give a ref (from browser_snapshot) or a CSS selector.");
    resetFonts();
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    const r2 = (n) => Math.round(n * 100) / 100;
    const display = s.display;
    const flex = display.includes("flex");
    const grid = display.includes("grid");
    const orNull = (v) => (v && v !== "none" ? v : null);
    let states = [];
    try {
      states = statesOf(el);
    } catch (_) {}
    let effective = "rgb(255, 255, 255)";
    try {
      effective = effectiveBackground(el);
    } catch (_) {}
    return {
      selector: helpers.selectorFor(el),
      label: labelOf(el),
      rect: { x: r2(r.x), y: r2(r.y), width: r2(r.width), height: r2(r.height) },
      box: {
        margin: sides(s, "margin", ""),
        border: sides(s, "border", "Width"),
        padding: sides(s, "padding", ""),
      },
      typography: {
        family: s.fontFamily,
        used_family: usedFamily(s.fontFamily),
        size: num(s.fontSize),
        weight: Math.round(num(s.fontWeight)) || 400,
        line_height: s.lineHeight === "normal" ? null : round3(num(s.lineHeight)),
        letter_spacing: s.letterSpacing === "normal" ? null : round3(num(s.letterSpacing)),
        color: s.color,
        align: s.textAlign,
        transform: s.textTransform,
      },
      background: { color: s.backgroundColor, image: orNull(s.backgroundImage) },
      border: {
        color: s.borderTopColor,
        style: s.borderTopStyle,
        width: sides(s, "border", "Width"),
        radius: s.borderRadius || radiusOf(s),
      },
      effects: { shadow: orNull(s.boxShadow), opacity: num(s.opacity), transition: s.transition || null },
      layout: {
        display,
        position: s.position,
        gap: flex || grid ? s.gap || null : null,
        flex_direction: flex ? s.flexDirection : null,
        justify: flex || grid ? s.justifyContent : null,
        align: flex || grid ? s.alignItems : null,
        grid_template_columns: grid ? orNull(s.gridTemplateColumns) : null,
      },
      states,
      effective_background: effective,
      html: op.html ? inlinedHtml(el) : null,
    };
  };

  Object.assign(modules, { scan, rules, style });
