// Sawit's page design module: reads the page's styles, stylesheets and assets
// for the Design panel. Spliced into the page agent after the overlay module
// (see browser_agent.js), so `resolve`, `clip`, `modules` and `helpers`
// (`helpers.selectorFor`) are in scope.
//
// Ops (their JSON is the contract in crates/core/src/site_design):
// - `scan` → `Scan`: colors, text styles, spacing, radii, shadows, sheets,
//   assets and component samples.
// - `rules` {sheets: [{href, text}]} → `SheetRules`: the same collectors over
//   sheets Sawit fetched because the page couldn't read them. They're kept for
//   `style`'s states.
// - `style` {ref | selector, html} → `StyleInfo`.
//
// Nothing here changes the page: it reads computed styles and sheets only.
{
  const MAX_ELEMENTS = 6000;
  // Hidden subtrees don't count against the budget, but walking them does cost.
  const MAX_WALKED = 60000;
  const MAX_COLORS = 500;
  const MAX_TEXTS = 400;
  const MAX_ASSETS = 400;
  const MAX_VARIABLES = 500;
  const MAX_ANSWER = 2 * 1024 * 1024;
  const MAX_SVG = 100 * 1024;
  const MAX_DATA_URL = 100 * 1024;
  const MAX_HTML = 30 * 1024;
  const MAX_INLINED = 200;
  const STATES = ["hover", "focus", "focus-visible", "active"];

  // Sheets Sawit fetched and the page parsed (`rules`), kept for `style`.
  let fetchedSheets = [];

  // ---- Small helpers ----

  const skipped = (el) => !!(el.closest && el.closest("[data-sawit-overlay]"));
  const absUrl = (url, base) => {
    try {
      return new URL(url, base || location.href).href;
    } catch (_) {
      return String(url);
    }
  };
  const num = (v) => {
    const n = parseFloat(v);
    return Number.isFinite(n) ? n : 0;
  };
  const round = (n, step) => Math.round(n / step) * step;
  const round3 = (n) => Math.round(n * 1000) / 1000;
  const classOf = (el) => (typeof el.className === "string" ? el.className : el.getAttribute("class") || "");
  const fileName = (url) => {
    try {
      const u = new URL(url, location.href);
      if (u.protocol === "data:" || u.protocol === "blob:") return "";
      const last = u.pathname.split("/").filter(Boolean).pop() || "";
      return decodeURIComponent(last);
    } catch (_) {
      return "";
    }
  };
  const isSvgUrl = (url) => {
    try {
      return /\.svg$/i.test(new URL(url, location.href).pathname) || /^data:image\/svg/i.test(url);
    } catch (_) {
      return false;
    }
  };
  // ---- Color math (pure: no DOM; browser.rs tests it in QuickJS) ----

  // A CSS color as WebKit computes it → sRGB [r, g, b, a] (0–255, alpha 0–1),
  // or null. Reads rgb()/rgba(), hex, oklch(), oklab(), lab(), lch() and
  // color(srgb | srgb-linear | display-p3); out-of-gamut values are clamped.
  const mat = (m, v) => m.map((row) => row[0] * v[0] + row[1] * v[1] + row[2] * v[2]);
  const XYZ_TO_SRGB = [
    [3.2409699419045226, -1.537383177570094, -0.4986107602930034],
    [-0.9692436362808796, 1.8759675015077202, 0.04155505740717559],
    [0.05563007969699366, -0.20397695888897652, 1.0569715142428786],
  ];
  const D50_TO_D65 = [
    [0.9554734527042182, -0.023098536874261423, 0.0632593086610217],
    [-0.028369706963208136, 1.0099954580058226, 0.021041398966943008],
    [0.012314001688319899, -0.020507696433477912, 1.3303659366080753],
  ];
  const P3_TO_XYZ = [
    [0.4865709486482162, 0.26566769316909306, 0.1982172852343625],
    [0.2289745640697488, 0.6917385218365064, 0.079286914093745],
    [0, 0.04511338185890264, 1.043944368900976],
  ];
  const toLinear = (v) => (Math.abs(v) <= 0.04045 ? v / 12.92 : Math.sign(v) * Math.pow((Math.abs(v) + 0.055) / 1.055, 2.4));
  const toGamma = (v) => (Math.abs(v) <= 0.0031308 ? v * 12.92 : Math.sign(v) * (1.055 * Math.pow(Math.abs(v), 1 / 2.4) - 0.055));
  const oklabToLinear = (L, a, b) => {
    const l = Math.pow(L + 0.3963377774 * a + 0.2158037573 * b, 3);
    const m = Math.pow(L - 0.1055613458 * a - 0.0638541728 * b, 3);
    const s = Math.pow(L - 0.0894841775 * a - 1.291485548 * b, 3);
    return [
      4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
      -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
      -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    ];
  };
  const labToLinear = (L, a, b) => {
    const k = 24389 / 27;
    const e = 216 / 24389;
    const fy = (L + 16) / 116;
    const fx = fy + a / 500;
    const fz = fy - b / 200;
    const x = fx * fx * fx > e ? fx * fx * fx : (116 * fx - 16) / k;
    const y = L > k * e ? fy * fy * fy : L / k;
    const z = fz * fz * fz > e ? fz * fz * fz : (116 * fz - 16) / k;
    return mat(XYZ_TO_SRGB, mat(D50_TO_D65, [x * 0.96422, y, z * 0.82521]));
  };
  // One component: a number, a percentage of `full`, an angle, or `none` (0).
  const part = (t, full) => {
    if (t === undefined || t === "none") return 0;
    const n = parseFloat(t);
    if (!Number.isFinite(n)) return NaN;
    if (t.endsWith("%")) return (n / 100) * full;
    if (t.endsWith("turn")) return n * 360;
    if (t.endsWith("rad")) return (n * 180) / Math.PI;
    if (t.endsWith("grad")) return n * 0.9;
    return n;
  };
  const readColor = (input) => {
    const c = String(input || "").trim().toLowerCase();
    if (!c) return null;
    if (c === "transparent") return [0, 0, 0, 0];
    const hex = /^#([0-9a-f]{3,8})$/.exec(c);
    if (hex) {
      let h = hex[1];
      if (h.length === 3 || h.length === 4) h = h.replace(/./g, "$&$&");
      if (h.length !== 6 && h.length !== 8) return null;
      const n = (i) => parseInt(h.slice(i, i + 2), 16);
      return [n(0), n(2), n(4), h.length === 8 ? n(6) / 255 : 1];
    }
    const fn = /^([a-z-]+)\(\s*([^()]*)\)$/.exec(c);
    if (!fn) return null;
    const [main, alphaText] = fn[2].split("/");
    const args = main.trim().split(/[\s,]+/).filter(Boolean);
    let alpha = alphaText === undefined ? 1 : part(alphaText.trim(), 1);
    let linear = null;
    let rgb = null;
    if (fn[1] === "rgb" || fn[1] === "rgba") {
      if (args.length < 3) return null;
      if (args.length === 4 && alphaText === undefined) alpha = part(args[3], 1);
      rgb = [part(args[0], 255), part(args[1], 255), part(args[2], 255)];
    } else if (fn[1] === "oklab") {
      linear = oklabToLinear(part(args[0], 1), part(args[1], 0.4), part(args[2], 0.4));
    } else if (fn[1] === "oklch") {
      const C = part(args[1], 0.4);
      const h = (part(args[2], 1) * Math.PI) / 180;
      linear = oklabToLinear(part(args[0], 1), C * Math.cos(h), C * Math.sin(h));
    } else if (fn[1] === "lab") {
      linear = labToLinear(part(args[0], 100), part(args[1], 125), part(args[2], 125));
    } else if (fn[1] === "lch") {
      const C = part(args[1], 150);
      const h = (part(args[2], 1) * Math.PI) / 180;
      linear = labToLinear(part(args[0], 100), C * Math.cos(h), C * Math.sin(h));
    } else if (fn[1] === "color") {
      const v = args.slice(1, 4).map((t) => part(t, 1));
      if (args[0] === "srgb") rgb = v.map((x) => x * 255);
      else if (args[0] === "srgb-linear") linear = v;
      else if (args[0] === "display-p3") linear = mat(XYZ_TO_SRGB, mat(P3_TO_XYZ, v.map(toLinear)));
      else return null;
    } else {
      return null;
    }
    if (linear) rgb = linear.map((x) => toGamma(x) * 255);
    if (!rgb || rgb.some((x) => !Number.isFinite(x)) || !Number.isFinite(alpha)) return null;
    const clamp = (x, hi) => Math.min(Math.max(x, 0), hi);
    return [clamp(rgb[0], 255), clamp(rgb[1], 255), clamp(rgb[2], 255), clamp(alpha, 1)];
  };
  // OKLCH chroma of sRGB [r, g, b] (0–255).
  const chromaOf = (p) => {
    const [r, g, b] = [toLinear(p[0] / 255), toLinear(p[1] / 255), toLinear(p[2] / 255)];
    const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
    const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
    const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
    const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
    return Math.sqrt(A * A + B * B);
  };

  // ---- End of color math ----

  // Any color the page computes → sRGB [r, g, b, a], falling back to the
  // browser's own reading (a detached canvas) for syntax not handled above.
  let canvasCtx = null;
  const canvas = () => {
    if (!canvasCtx) canvasCtx = document.createElement("canvas").getContext("2d");
    return canvasCtx;
  };
  const rgbaOf = (c) => {
    const read = readColor(c);
    if (read) return read;
    try {
      const ctx = canvas();
      if (!ctx) return null;
      ctx.fillStyle = "#010203";
      ctx.fillStyle = String(c);
      const out = String(ctx.fillStyle);
      if (out === "#010203") return null;
      return readColor(out);
    } catch (_) {
      return null;
    }
  };
  const transparent = (c) => {
    if (!c || c === "transparent") return true;
    const p = rgbaOf(c);
    return !!p && p[3] === 0;
  };
  // OKLCH chroma of any color, for "chromatic" (≥ 0.03, as in site_design).
  const chroma = (c) => {
    const p = rgbaOf(c);
    return !p || p[3] === 0 ? 0 : chromaOf(p);
  };
  // Every color written inside a computed value (box-shadow, gradients).
  const COLOR_IN_VALUE = /(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\([^()]*\)|#[0-9a-fA-F]{3,8}\b/g;
  const colorsIn = (value) => (value && value !== "none" ? value.match(COLOR_IN_VALUE) || [] : []);
  // A declared color (maybe a var()) as the browser normalises it, or null.
  const normColor = (value) => {
    let v = (value || "").trim();
    if (!v || /^(auto|none|inherit|initial|unset|revert|currentcolor|invert)$/i.test(v)) return null;
    try {
      const root = getComputedStyle(document.documentElement);
      for (let i = 0; i < 5 && v.includes("var("); i++) {
        v = v.replace(/var\(\s*(--[\w-]+)\s*(?:,\s*([^()]*))?\)/g, (_, name, fallback) => root.getPropertyValue(name).trim() || (fallback || "").trim());
      }
      if (!v || v.includes("var(")) return null;
      const ctx = canvas();
      if (!ctx) return v;
      ctx.fillStyle = "#010203";
      ctx.fillStyle = v;
      const out = String(ctx.fillStyle);
      if (out === "#010203" && !/^#010203$/i.test(v)) return null;
      if (/^#[0-9a-f]{6}$/i.test(out)) {
        const n = parseInt(out.slice(1), 16);
        return "rgb(" + (n >> 16) + ", " + ((n >> 8) & 255) + ", " + (n & 255) + ")";
      }
      return out;
    } catch (_) {
      return null;
    }
  };

  const buttonLike = (el) => {
    const tag = el.localName;
    if (tag === "button") return true;
    if (el.getAttribute("role") === "button") return true;
    if (tag === "input" && (el.type === "submit" || el.type === "button")) return true;
    return /\bbtn\b|button/i.test(classOf(el));
  };
  // The control text or a color sits in: the nearest link or button around it.
  const controlOf = (el) => (el.closest && el.closest("a, button, [role=button], input")) || el;

  // The element's corner radius: top-left, or the largest corner when they differ.
  const radiusOf = (s) => {
    const corners = [s.borderTopLeftRadius, s.borderTopRightRadius, s.borderBottomRightRadius, s.borderBottomLeftRadius];
    if (corners.every((c) => c === corners[0])) return corners[0];
    let best = corners[0];
    for (const c of corners) if (num(c) > num(best)) best = c;
    return best;
  };
  const zeroRadius = (r) => !r || /^0(px)?( 0(px)?)?$/.test(r);
  const sides = (s, prefix, suffix) => ["Top", "Right", "Bottom", "Left"].map((side) => num(s[prefix + side + suffix]));
  const shorthand = (v) => {
    const p = v.map((n) => round3(n) + "px");
    if (p[0] === p[1] && p[1] === p[2] && p[2] === p[3]) return p[0];
    if (p[0] === p[2] && p[1] === p[3]) return p[0] + " " + p[1];
    if (p[1] === p[3]) return p[0] + " " + p[1] + " " + p[2];
    return p.join(" ");
  };
  const hasBorder = (s) => sides(s, "border", "Width").some((w, i) => w > 0 && s["border" + ["Top", "Right", "Bottom", "Left"][i] + "Style"] !== "none");

  // ---- Fonts: the face the page actually renders ----

  const GENERICS = new Set([
    "serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui", "ui-serif", "ui-sans-serif",
    "ui-monospace", "ui-rounded", "math", "emoji", "fangsong", "-apple-system", "blinkmacsystemfont",
  ]);
  const splitStack = (stack) => {
    const out = [];
    let cur = "";
    let quote = "";
    for (const ch of stack || "") {
      if (quote) {
        if (ch === quote) quote = "";
        else cur += ch;
      } else if (ch === '"' || ch === "'") {
        quote = ch;
      } else if (ch === ",") {
        out.push(cur.trim());
        cur = "";
      } else {
        cur += ch;
      }
    }
    if (cur.trim()) out.push(cur.trim());
    return out.filter(Boolean);
  };
  // A family counts as rendered when the page loaded a face for it
  // (document.fonts), or, for an installed font, when text set in it measures
  // differently from the generic alone (against serif and monospace, so a font
  // that looks like one of them still shows). `document.fonts.check` alone
  // says yes to any family without an @font-face, installed or not.
  const PROBE = "mmmmmmmmmmlli1WQ@#&%ÆØ";
  const familyCache = new Map();
  let loadedFamilies = null;
  // Cleared at the start of each op: faces load and pages change between ops.
  const resetFonts = () => {
    familyCache.clear();
    loadedFamilies = null;
  };
  const loaded = () => {
    if (!loadedFamilies) {
      loadedFamilies = new Set();
      try {
        if (document.fonts) {
          document.fonts.forEach((face) => {
            if (face.status === "loaded") loadedFamilies.add(unquote(face.family).toLowerCase());
          });
        }
      } catch (_) {}
    }
    return loadedFamilies;
  };
  const rendered = (name) => {
    const key = name.toLowerCase();
    if (familyCache.has(key)) return familyCache.get(key);
    let yes = loaded().has(key);
    if (!yes) {
      try {
        const ctx = canvas();
        const quoted = '"' + name.replace(/["\\]/g, "\\$&") + '"';
        for (const generic of ["serif", "monospace"]) {
          ctx.font = "72px " + generic;
          const plain = ctx.measureText(PROBE).width;
          ctx.font = "72px " + quoted + ", " + generic;
          if (Math.abs(ctx.measureText(PROBE).width - plain) > 0.01) {
            yes = true;
            break;
          }
        }
      } catch (_) {}
    }
    familyCache.set(key, yes);
    return yes;
  };
  // The first entry of a font stack the page can render, or the generic it falls back to.
  const usedFamily = (stack) => {
    for (const name of splitStack(stack)) {
      if (GENERICS.has(name.toLowerCase())) return name;
      if (rendered(name)) return name;
    }
    return null;
  };

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

  // ---- HTML with computed styles inlined ----

  // [property, default] — a non-inherited property equal to its default is left out.
  const BOX_PROPS = [
    ["display", null], ["position", "static"], ["box-sizing", "content-box"],
    ["flex-direction", "row"], ["flex-wrap", "nowrap"], ["justify-content", "normal"], ["align-items", "normal"],
    ["gap", "normal"], ["grid-template-columns", "none"],
    ["padding", "0px"], ["margin", "0px"],
    ["border-top", null], ["border-right", null], ["border-bottom", null], ["border-left", null],
    ["border-radius", "0px"], ["background-color", "rgba(0, 0, 0, 0)"], ["background-image", "none"],
    ["box-shadow", "none"], ["opacity", "1"], ["text-decoration-line", "none"],
  ];
  // Inherited: written on the root, and on a descendant only where it differs from its parent.
  const TEXT_PROPS = [
    "color", "font-family", "font-size", "font-weight", "font-style", "line-height", "letter-spacing",
    "text-align", "text-transform", "white-space",
  ];
  const styleText = (el, parentStyle) => {
    const s = getComputedStyle(el);
    const parts = [];
    for (const [prop, dflt] of BOX_PROPS) {
      let v = s.getPropertyValue(prop);
      if (prop.startsWith("border-") && prop !== "border-radius") {
        const side = prop.slice(7);
        const w = s.getPropertyValue("border-" + side + "-width");
        const st = s.getPropertyValue("border-" + side + "-style");
        if (st === "none" || num(w) === 0) continue;
        v = w + " " + st + " " + s.getPropertyValue("border-" + side + "-color");
      } else if (prop === "padding" || prop === "margin") {
        const v4 = sides(s, prop, "");
        if (v4.every((n) => n === 0)) continue;
        v = shorthand(v4);
      } else if (prop === "border-radius") {
        if (zeroRadius(v) || !v) continue;
      }
      if (!v || v === dflt) continue;
      parts.push(prop + ": " + v);
    }
    for (const prop of TEXT_PROPS) {
      const v = s.getPropertyValue(prop);
      if (!v) continue;
      if (parentStyle && parentStyle.getPropertyValue(prop) === v) continue;
      parts.push(prop + ": " + v);
    }
    return { text: parts.join("; ") + (parts.length ? ";" : ""), style: s };
  };
  const strip = (root) => {
    for (const s of [...root.querySelectorAll("script")]) s.remove();
    for (const el of [root, ...root.querySelectorAll("*")]) {
      for (const attr of [...el.attributes]) {
        if (/^on/i.test(attr.name)) el.removeAttribute(attr.name);
      }
    }
  };
  // outerHTML of a copy, with computed styles inlined on the first MAX_INLINED
  // elements, no scripts or on* handlers, at most MAX_HTML. The copy lives in
  // an inert document: no image loads, no custom element constructors.
  let inertDoc = null;
  const copyOf = (el) => {
    if (!inertDoc) inertDoc = document.implementation.createHTMLDocument("");
    return inertDoc.importNode(el, true);
  };
  const inlinedHtml = (el) => {
    try {
      const copy = copyOf(el);
      const originals = [el, ...el.querySelectorAll("*")];
      const copies = [copy, ...copy.querySelectorAll("*")];
      const styles = new Map();
      const n = Math.min(originals.length, copies.length, MAX_INLINED);
      for (let i = 0; i < n; i++) {
        try {
          const orig = originals[i];
          const parent = i === 0 ? null : styles.get(orig.parentElement) || null;
          const { text, style: s } = styleText(orig, parent);
          styles.set(orig, s);
          if (text) copies[i].setAttribute("style", text);
          else copies[i].removeAttribute("style");
        } catch (_) {}
      }
      strip(copy);
      let html = copy.outerHTML || "";
      if (html.length > MAX_HTML) {
        // Too big with every element styled: the root styled, children as they are.
        const plain = copyOf(el);
        if (copy.hasAttribute("style")) plain.setAttribute("style", copy.getAttribute("style"));
        strip(plain);
        html = plain.outerHTML || "";
      }
      if (html.length > MAX_HTML) html = html.slice(0, MAX_HTML) + "<!-- cut at " + MAX_HTML + " characters -->";
      return html;
    } catch (_) {
      return "";
    }
  };

  // ---- scan ----

  const ASSET_LABEL_ATTRS = ["alt", "aria-label", "title"];
  const scan = () => {
    resetFonts();
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
    const addColor = (value, role, ctx) => {
      if (transparent(value)) return;
      const c = bump(colors, value + "\n" + role, () => ({ value, role, count: 0, on_button: 0, on_link: 0 }));
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
    const componentsOf = (el, s, rect) => {
      const tag = el.localName;
      if (buttonLike(el)) {
        const bg = s.backgroundColor;
        const filled = !transparent(bg);
        if (filled || hasBorder(s)) {
          const key = [bg, s.color, radiusOf(s), shorthand(sides(s, "padding", ""))].join("|");
          const c = bump(buttons, key, () => ({ el, count: 0, chromatic: filled && chroma(bg) >= 0.03 }));
          c.count++;
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
            componentsOf(el, s, rect);
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
    const sheetRules = rulesOut(acc);

    // Body background: the body's, else the root's.
    let bodyBackground = null;
    try {
      const b = document.body && getComputedStyle(document.body).backgroundColor;
      const h = getComputedStyle(document.documentElement).backgroundColor;
      bodyBackground = !transparent(b) ? b : !transparent(h) ? h : null;
    } catch (_) {}

    let manifest = null;
    const manifestLink = document.querySelector("link[rel~=manifest][href]");
    if (manifestLink) manifest = absUrl(manifestLink.getAttribute("href"));

    // Components: one sample each.
    const top = (map, ok) => {
      let best = null;
      for (const [key, c] of map) if ((!ok || ok(c, key)) && (!best || c.count > best[1].count)) best = [key, c];
      return best;
    };
    const components = [];
    const sample = (kind, el) => {
      if (!el) return;
      try {
        const s = getComputedStyle(el);
        components.push({
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
    const primary = top(buttons, (c) => c.chromatic);
    if (primary) {
      sample("button", primary[1].el);
      const secondary = top(buttons, (_, key) => key !== primary[0]);
      if (secondary) sample("button-secondary", secondary[1].el);
    }
    sample("input", input);
    const card = top(cards);
    if (card) sample("card", card[1].el);
    const visible = (el) => {
      if (!el || skipped(el)) return false;
      const r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    };
    const nav = [...document.querySelectorAll("header nav")].find(visible) || [...document.querySelectorAll("nav")].find(visible);
    sample("nav", nav || null);
    const link = top(links);
    if (link) sample("link", link[1].el);

    const byCount = (map) => [...map].sort((a, b) => b[1] - a[1]);
    const out = {
      url: location.href,
      title: document.title,
      viewport: [innerWidth, innerHeight],
      truncated,
      colors: [...colors.values()].sort((a, b) => b.count - a.count).slice(0, MAX_COLORS),
      texts: [...texts.values()].sort((a, b) => b.chars - a.chars).slice(0, MAX_TEXTS),
      spacing: byCount(spacing),
      radii: byCount(radii),
      shadows: byCount(shadows),
      body_background: bodyBackground,
      focus_ring: sheetRules.focus_ring,
      font_faces: sheetRules.font_faces,
      variables: sheetRules.variables,
      breakpoints: sheetRules.breakpoints,
      assets,
      components,
      blocked_sheets: [...blocked],
      manifest,
    };
    // Over budget: halve the heaviest list until it fits.
    const LISTS = ["assets", "components", "texts", "colors", "variables", "font_faces", "shadows", "radii", "spacing"];
    const encoder = new TextEncoder();
    const bytes = (v) => encoder.encode(JSON.stringify(v)).length;
    let size = bytes(out);
    for (let i = 0; i < 40 && size > MAX_ANSWER; i++) {
      let heaviest = null;
      let weight = 0;
      for (const name of LISTS) {
        const w = out[name].length ? bytes(out[name]) : 0;
        if (w > weight) {
          weight = w;
          heaviest = name;
        }
      }
      if (!heaviest) break;
      out[heaviest] = out[heaviest].slice(0, Math.floor(out[heaviest].length / 2));
      out.truncated = true;
      size = bytes(out);
    }
    return out;
  };

  // ---- rules ----

  const rules = (op) => {
    const acc = newRules();
    const parsed = [];
    for (const item of (op && op.sheets) || []) {
      try {
        const href = String(item.href || location.href);
        const text = String(item.text || "").replace(/@import\s[^;]*;?/gi, "");
        const sheet = new CSSStyleSheet();
        sheet.replaceSync(text);
        parsed.push({ sheet, list: sheet.cssRules, base: href });
        readRules(sheet.cssRules, href, acc, null);
      } catch (_) {}
    }
    fetchedSheets = parsed;
    return rulesOut(acc);
  };

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
}
