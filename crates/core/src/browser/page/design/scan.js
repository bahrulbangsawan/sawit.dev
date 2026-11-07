  // ---- scan ----

  const ASSET_LABEL_ATTRS = ["alt", "aria-label", "title"];
  // What the last scan left for `scan_samples`: its button looks' elements, and the input,
  // card, nav and link it picked.
  let toSample = null;
  const scan = () => {
    resetFonts();
    const readings = newReadings();
    const note = readings.note;
    const colors = new Map();
    const texts = new Map();
    const spacing = new Map();
    const radii = new Map();
    const shadows = new Map();
    const assets = [];
    const assetByKey = new Map();
    const buttons = new Map();
    const cards = new Map();
    const links = new Map();
    let truncated = false;
    let input = null;
    const hasMain = !!document.querySelector("main, [role=main]");

    const bump = (map, key, make) => {
      let entry = map.get(key);
      if (!entry) {
        entry = make();
        map.set(key, entry);
      }
      return entry;
    };
    // Every colour, transparent ones too: Rust drops those (site_design::page::plan).
    const addColor = (value, role, ctx) => {
      if (!value) return;
      const c = bump(colors, value + "\n" + role, () => ({ value: note(value), role, count: 0, on_button: 0, on_link: 0 }));
      c.count++;
      if (ctx && buttonLike(ctx)) c.on_button++;
      else if (ctx && ctx.localName === "a") c.on_link++;
    };
    const count = (map, key) => map.set(key, (map.get(key) || 0) + 1);

    // Assets.
    // The header: `header` or `[role=banner]`; on a page with neither, the first
    // `nav` outside main, aside, article and footer (apple.com's #globalnav).
    const topNav = document.querySelector("header, [role=banner]")
      ? null
      : [...document.querySelectorAll("nav")].find((n) => !n.closest("main, aside, article, footer")) || null;
    const isLogo = (el, url) => {
      const head = el.closest("header, [role=banner]") || (topNav && topNav.contains(el));
      const home = el.closest("a[href]");
      if (head && home) {
        try {
          const u = new URL(home.getAttribute("href"), location.href);
          if (u.origin === location.origin && (u.pathname === "/" || u.pathname === "")) return true;
        } catch (_) {}
      }
      return /logo/i.test(classOf(el) + " " + (el.getAttribute("alt") || "") + " " + (url ? fileName(url) : ""));
    };
    const labelFor = (el, url) => {
      for (const a of ASSET_LABEL_ATTRS) {
        const v = el && el.getAttribute(a);
        if (v && v.trim()) return clip(v.trim().replace(/\s+/g, " "), 80);
      }
      if (el && el.localName === "svg") {
        const t = el.querySelector("title");
        if (t && t.textContent.trim()) return clip(t.textContent.trim(), 80);
      }
      return (url && fileName(url)) || (el ? el.localName : "asset");
    };
    const addAsset = (kind, url, svg, el, extra) => {
      if (url && url.startsWith("data:") && url.length > MAX_DATA_URL) return;
      const key = kind === "lottie" || kind === "icon" || kind === "video" ? kind + "\n" + (url || svg) : (url || svg);
      let a = assetByKey.get(key);
      if (!a) {
        if (assets.length >= MAX_ASSETS) return;
        const logo = !!el && isLogo(el, url);
        a = {
          kind,
          url: url || null,
          svg: svg || null,
          label: logo ? "logo" : (extra && extra.label) || labelFor(el, url),
          width: extra && extra.width ? Math.round(extra.width) : null,
          height: extra && extra.height ? Math.round(extra.height) : null,
          used_on: [],
          logo,
        };
        assetByKey.set(key, a);
        assets.push(a);
      }
      if (el && a.used_on.length < 3) {
        try {
          const sel = helpers.selectorFor(el);
          if (!a.used_on.includes(sel)) a.used_on.push(sel);
        } catch (_) {}
      }
    };
    const assetsOf = (el, s, rect) => {
      const tag = el.localName;
      if (tag === "img") {
        const url = el.currentSrc || el.src;
        if (url) addAsset(isSvgUrl(url) ? "svg" : "image", url, null, el, { width: el.naturalWidth, height: el.naturalHeight });
      } else if (tag === "svg" && !el.ownerSVGElement) {
        const markup = el.outerHTML || "";
        if (markup && markup.length <= MAX_SVG) {
          const icon = rect.width <= 48 && rect.height <= 48;
          addAsset(icon ? "icon" : "svg", null, markup, el, { width: rect.width, height: rect.height });
        }
      } else if (tag === "video") {
        const source = el.querySelector("source[src]");
        const url = el.currentSrc || el.src || (source ? absUrl(source.getAttribute("src")) : "");
        const label = labelFor(el, url);
        if (url) addAsset("video", url, null, el, { width: el.videoWidth, height: el.videoHeight, label });
        if (el.poster) addAsset("image", el.poster, null, el, { label: label + " poster" });
      } else if ((tag === "lottie-player" || tag === "dotlottie-player") && el.getAttribute("src")) {
        addAsset("lottie", absUrl(el.getAttribute("src")), null, el, null);
      }
      const bg = s.backgroundImage;
      if (bg && bg !== "none" && bg.includes("url(")) {
        const re = /url\(\s*(["']?)(.*?)\1\s*\)/g;
        let m;
        while ((m = re.exec(bg))) {
          const url = absUrl(m[2]);
          addAsset(isSvgUrl(url) ? "svg" : "image", url, null, el, null);
        }
      }
    };

    // Components.
    const addCandidate = (map, key, el) => {
      const c = bump(map, key, () => ({ el, count: 0 }));
      c.count++;
    };
    // A button look counts the elements that are filled or have a border; which are filled
    // waits on Rust's reading of the background, so both counts go, with where each starts.
    const componentsOf = (el, s, rect, at) => {
      const tag = el.localName;
      if (buttonLike(el)) {
        const bg = s.backgroundColor;
        const key = [bg, s.color, radiusOf(s), shorthand(sides(s, "padding", ""))].join("|");
        const c = bump(buttons, key, () => ({ background: note(bg), count: 0, first: at, bordered: 0, first_bordered: null, el, elBordered: null }));
        c.count++;
        if (hasBorder(s)) {
          if (!c.bordered) {
            c.first_bordered = at;
            c.elBordered = el;
          }
          c.bordered++;
        }
      } else if (tag === "a" && (!hasMain || el.closest("main, [role=main]"))) {
        addCandidate(links, [s.color, s.textDecorationLine, s.fontWeight, s.fontSize].join("|"), el);
      }
      if (!input && (tag === "textarea" || (tag === "input" && /^(text|email|search|url|tel|password|number|)$/.test(el.type)))) input = el;
      if (rect.width >= 120 && rect.width <= 800 && num(radiusOf(s)) >= 4 && (s.boxShadow !== "none" || hasBorder(s))) {
        if (el.querySelector("h1, h2, h3, h4, h5, h6, [role=heading], img")) {
          addCandidate(cards, [s.backgroundColor, radiusOf(s), s.boxShadow, hasBorder(s) ? s.borderTopColor : ""].join("|"), el);
        }
      }
    };

    // The walk: visible elements in document order.
    const SHAPES = new Set(["path", "circle", "rect", "ellipse", "line", "polyline", "polygon", "text", "use", "tspan"]);
    const root = document.body;
    if (root) {
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
      const pastSubtree = () => {
        while (!walker.nextSibling()) {
          if (!walker.parentNode() || walker.currentNode === root) return null;
        }
        return walker.currentNode;
      };
      let visited = 0;
      let walked = 0;
      let el = walker.currentNode;
      while (el) {
        if (++walked > MAX_WALKED || visited >= MAX_ELEMENTS) {
          truncated = true;
          break;
        }
        let s = null;
        try {
          if (el.hasAttribute("data-sawit-overlay")) {
            el = pastSubtree();
            continue;
          }
          s = getComputedStyle(el);
          if (s.display === "none") {
            el = pastSubtree();
            continue;
          }
        } catch (_) {
          el = walker.nextNode();
          continue;
        }
        try {
          const rect = el.getBoundingClientRect();
          if (rect.width > 0 && rect.height > 0 && s.visibility !== "hidden") {
            visited++;
            const tag = el.localName;
            const ctx = controlOf(el);
            // Text.
            let chars = 0;
            for (const node of el.childNodes) {
              if (node.nodeType === Node.TEXT_NODE) chars += node.textContent.replace(/\s+/g, " ").trim().length;
            }
            if (chars > 0) {
              addColor(s.color, "text", ctx);
              const bucket = buttonLike(ctx) ? "button" : tag;
              const inMain = !!el.closest("main, [role=main]");
              const size = num(s.fontSize);
              const weight = Math.round(num(s.fontWeight)) || 400;
              const lh = s.lineHeight === "normal" ? null : round3(num(s.lineHeight));
              const ls = s.letterSpacing === "normal" ? null : round3(num(s.letterSpacing));
              const key = [bucket, s.fontFamily, size, weight, lh, ls, s.color, inMain].join("\n");
              const t = bump(texts, key, () => ({
                tag: bucket,
                in_main: inMain,
                family: s.fontFamily,
                used_family: usedFamily(s.fontFamily),
                size,
                weight,
                line_height: lh,
                letter_spacing: ls,
                color: s.color,
                chars: 0,
              }));
              t.chars += chars;
            }
            // Colors.
            addColor(s.backgroundColor, "background", el);
            const borderColors = new Set();
            ["Top", "Right", "Bottom", "Left"].forEach((side) => {
              if (num(s["border" + side + "Width"]) > 0 && s["border" + side + "Style"] !== "none") borderColors.add(s["border" + side + "Color"]);
            });
            for (const c of borderColors) addColor(c, "border", el);
            if (el instanceof SVGElement && SHAPES.has(tag)) {
              if (s.fill && s.fill !== "none" && !s.fill.startsWith("url(")) addColor(s.fill, "fill", el);
              if (s.stroke && s.stroke !== "none" && !s.stroke.startsWith("url(")) addColor(s.stroke, "stroke", el);
            }
            for (const c of new Set(colorsIn(s.boxShadow))) addColor(c, "shadow", el);
            if (s.backgroundImage && s.backgroundImage.includes("gradient(")) {
              for (const c of new Set(colorsIn(s.backgroundImage))) addColor(c, "gradient", el);
            }
            // Spacing, radii, shadows.
            for (const v of [...sides(s, "margin", ""), ...sides(s, "padding", ""), num(s.rowGap), num(s.columnGap)]) {
              if (v > 0) count(spacing, round(v, 0.5));
            }
            const radius = radiusOf(s);
            if (!zeroRadius(radius)) count(radii, radius);
            if (s.boxShadow && s.boxShadow !== "none") count(shadows, s.boxShadow);
            assetsOf(el, s, rect);
            componentsOf(el, s, rect, visited);
          }
        } catch (_) {}
        el = walker.nextNode();
      }
    }

    // Icons and Lottie outside the walk.
    for (const link of document.querySelectorAll("link[rel~=icon], link[rel~=apple-touch-icon], link[rel~=mask-icon], link[rel~=apple-touch-icon-precomposed]")) {
      try {
        const href = link.getAttribute("href");
        if (!href) continue;
        const size = /^(\d+)x(\d+)$/i.exec((link.getAttribute("sizes") || "").trim());
        const rel = (link.getAttribute("rel") || "icon").toLowerCase();
        addAsset("icon", absUrl(href), null, null, {
          label: rel.includes("apple") ? "apple-touch-icon" : rel.includes("mask") ? "mask-icon" : "favicon",
          width: size ? Number(size[1]) : null,
          height: size ? Number(size[2]) : null,
        });
      } catch (_) {}
    }
    try {
      for (const player of document.querySelectorAll("lottie-player[src], dotlottie-player[src]")) {
        if (!skipped(player)) addAsset("lottie", absUrl(player.getAttribute("src")), null, player, null);
      }
      for (const entry of performance.getEntriesByType("resource")) {
        const url = entry.name || "";
        let path = url;
        try {
          path = new URL(url).pathname;
        } catch (_) {}
        if (/\.lottie$/i.test(path) || (/\.json$/i.test(path) && /lottie/i.test(url))) {
          addAsset("lottie", url, null, null, { label: fileName(url) || "animation" });
        }
      }
    } catch (_) {}

    // Sheets.
    const acc = newRules();
    const { readable, blocked } = pageSheets();
    for (const { list, base } of readable) {
      try {
        readRules(list, base, acc, blocked);
      } catch (_) {}
    }
    const sheetRules = rulesOut(acc, note);

    // Body background: the body's, else the root's (Rust picks).
    let bodyBackgrounds = [];
    try {
      const b = document.body && getComputedStyle(document.body).backgroundColor;
      const h = getComputedStyle(document.documentElement).backgroundColor;
      bodyBackgrounds = [note(b) || null, note(h) || null];
    } catch (_) {}

    let manifest = null;
    const manifestLink = document.querySelector("link[rel~=manifest][href]");
    if (manifestLink) manifest = absUrl(manifestLink.getAttribute("href"));

    // Components: Rust picks the buttons, `scan_samples` samples them with these.
    const top = (map) => {
      let best = null;
      for (const [key, c] of map) if (!best || c.count > best[1].count) best = [key, c];
      return best;
    };
    const visible = (el) => {
      if (!el || skipped(el)) return false;
      const r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    };
    const card = top(cards);
    const link = top(links);
    toSample = {
      buttons: [...buttons.values()],
      input,
      card: card ? card[1].el : null,
      nav: [...document.querySelectorAll("header nav")].find(visible) || [...document.querySelectorAll("nav")].find(visible) || null,
      link: link ? link[1].el : null,
    };

    const byCount = (map) => [...map].sort((a, b) => b[1] - a[1]);
    return {
      url: location.href,
      title: document.title,
      viewport: [innerWidth, innerHeight],
      truncated,
      colors: [...colors.values()],
      texts: [...texts.values()].sort((a, b) => b.chars - a.chars).slice(0, MAX_TEXTS),
      spacing: byCount(spacing),
      radii: byCount(radii),
      shadows: byCount(shadows),
      body_backgrounds: bodyBackgrounds,
      focus_rings: sheetRules.focus_rings,
      font_faces: sheetRules.font_faces,
      variables: sheetRules.variables,
      breakpoints: sheetRules.breakpoints,
      assets,
      buttons: [...buttons.values()].map((b) => ({ background: b.background, count: b.count, first: b.first, bordered: b.bordered, first_bordered: b.first_bordered })),
      blocked_sheets: [...blocked],
      manifest,
      color_fallbacks: readings.fallbacks,
    };
  };
  const sample = (into, kind, el) => {
    if (!el) return;
    try {
      const s = getComputedStyle(el);
      into.push({
        kind,
        selector: helpers.selectorFor(el),
        html: inlinedHtml(el),
        style: {
          background: s.backgroundColor,
          color: s.color,
          radius: radiusOf(s),
          padding: shorthand(sides(s, "padding", "")),
          font_family: s.fontFamily,
          font_size: num(s.fontSize),
          font_weight: Math.round(num(s.fontWeight)) || 400,
          height: round3(el.getBoundingClientRect().height),
          border: s.borderTopWidth + " " + s.borderTopStyle + " " + s.borderTopColor,
        },
      });
    } catch (_) {}
  };
  // {button, secondary}: the button looks Rust picked, each `{id, bordered}` (its index in the
  // scan's `buttons`, and whether its first element with a border stands for it), or null.
  const scanSamples = (op) => {
    const from = toSample;
    if (!from) throw new Error("Scan the page first.");
    const button = (pick) => {
      const look = pick && from.buttons[pick.id];
      return look ? (pick.bordered ? look.elBordered : look.el) : null;
    };
    const components = [];
    sample(components, "button", button(op.button));
    sample(components, "button-secondary", button(op.secondary));
    sample(components, "input", from.input);
    sample(components, "card", from.card);
    sample(components, "nav", from.nav);
    sample(components, "link", from.link);
    return { components };
  };
