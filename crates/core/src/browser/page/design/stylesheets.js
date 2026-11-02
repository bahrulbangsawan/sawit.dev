  // ---- Stylesheets ----

  const unquote = (v) => (v || "").trim().replace(/^(["'])(.*)\1$/, "$2");
  const mediaWidths = (text, into) => {
    const re = /\(\s*(?:min|max)-width\s*:\s*([\d.]+)(px|em|rem)\s*\)|\bwidth\s*[<>]=?\s*([\d.]+)(px|em|rem)/g;
    let m;
    while ((m = re.exec(text || ""))) {
      const n = Number(m[1] || m[3]);
      const unit = m[2] || m[4];
      const px = Math.round(unit === "px" ? n : n * 16);
      if (px > 0) into.add(px);
    }
  };
  const isRootSelector = (sel) => (sel || "").split(",").some((part) => /^\s*(:root|html)\s*$/i.test(part));
  const newRules = () => ({ faces: [], faceKeys: new Set(), variables: new Map(), breakpoints: new Set(), focusVisible: null, focus: null });
  // Reads one sheet's rules into `acc`; nested sheets (@import) a page can't read go to `blocked`.
  const readRules = (rules, base, acc, blocked) => {
    for (const rule of rules) {
      try {
        if (rule.type === CSSRule.IMPORT_RULE) {
          const inner = rule.styleSheet;
          if (!inner) continue;
          let list;
          try {
            list = inner.cssRules;
          } catch (_) {}
          if (!list) {
            if (blocked && inner.href) blocked.add(inner.href);
            continue;
          }
          readRules(list, inner.href || base, acc, blocked);
        } else if (rule.type === CSSRule.FONT_FACE_RULE) {
          const st = rule.style;
          const family = unquote(st.getPropertyValue("font-family"));
          if (!family) continue;
          const weight = st.getPropertyValue("font-weight").trim() || "normal";
          const fstyle = st.getPropertyValue("font-style").trim() || "normal";
          const key = family + "\n" + weight + "\n" + fstyle;
          if (acc.faceKeys.has(key)) continue;
          const src = [];
          const re = /url\(\s*(["']?)(.*?)\1\s*\)/g;
          let m;
          while ((m = re.exec(st.getPropertyValue("src")))) {
            if (m[2].startsWith("data:") && m[2].length > MAX_DATA_URL) continue;
            src.push(absUrl(m[2], base));
          }
          acc.faceKeys.add(key);
          acc.faces.push({ family, weight, style: fstyle, src });
        } else if (rule.type === CSSRule.MEDIA_RULE) {
          mediaWidths(rule.conditionText || (rule.media && rule.media.mediaText), acc.breakpoints);
          readRules(rule.cssRules, base, acc, blocked);
        } else if (rule.type === CSSRule.STYLE_RULE) {
          const sel = rule.selectorText || "";
          const st = rule.style;
          if (isRootSelector(sel)) {
            const root = getComputedStyle(document.documentElement);
            for (let i = 0; i < st.length && acc.variables.size < MAX_VARIABLES; i++) {
              const name = st[i];
              if (!name.startsWith("--") || acc.variables.has(name)) continue;
              const value = root.getPropertyValue(name).trim() || st.getPropertyValue(name).trim();
              if (value) acc.variables.set(name, value);
            }
          }
          if (/:focus(-visible)?(?![\w-])/.test(sel)) {
            const which = sel.includes(":focus-visible") ? "focusVisible" : "focus";
            if (!acc[which]) {
              const ring = normColor(st.getPropertyValue("outline-color"));
              if (ring && !transparent(ring)) acc[which] = ring;
            }
          }
          if (rule.cssRules && rule.cssRules.length) readRules(rule.cssRules, base, acc, blocked);
        } else if (rule.cssRules) {
          // @supports, @layer, @container and the like.
          readRules(rule.cssRules, base, acc, blocked);
        }
      } catch (_) {}
    }
  };
  const rulesOut = (acc) => ({
    font_faces: acc.faces,
    variables: [...acc.variables],
    breakpoints: [...acc.breakpoints].sort((a, b) => a - b),
    focus_ring: acc.focusVisible || acc.focus,
  });
  // The page's own sheets, with the ones it can't read (cross-origin) apart.
  const pageSheets = () => {
    const out = [];
    const blocked = new Set();
    const all = [...document.styleSheets];
    try {
      if (document.adoptedStyleSheets) all.push(...document.adoptedStyleSheets);
    } catch (_) {}
    for (const sheet of all) {
      try {
        const owner = sheet.ownerNode;
        if (owner && owner.closest && skipped(owner)) continue;
        let list;
        try {
          list = sheet.cssRules;
        } catch (_) {}
        if (!list) {
          if (sheet.href) blocked.add(sheet.href);
          continue;
        }
        out.push({ sheet, list, base: sheet.href || location.href });
      } catch (_) {}
    }
    return { readable: out, blocked };
  };
