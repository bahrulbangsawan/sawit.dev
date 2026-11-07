  // ---- Colours ----

  // Rust reads the colours the page computes (site_design::computed): rgb()/rgba(), hex,
  // oklch(), oklab(), lab(), lch() and color(srgb | srgb-linear | display-p3). For any other
  // syntax an answer sends the browser's own reading with it (a detached canvas's fillStyle),
  // in `color_fallbacks`.
  let canvasCtx = null;
  const canvas = () => {
    if (!canvasCtx) canvasCtx = document.createElement("canvas").getContext("2d");
    return canvasCtx;
  };
  const READ_IN_RUST = /^(?:#[0-9a-f]{3,8}|transparent|(?:rgba?|oklab|oklch|lab|lch)\([^()]*\)|color\(\s*(?:srgb|srgb-linear|display-p3)\s[^()]*\))$/;
  const browserReading = (c) => {
    try {
      const ctx = canvas();
      if (!ctx) return null;
      ctx.fillStyle = "#010203";
      ctx.fillStyle = String(c);
      const out = String(ctx.fillStyle);
      return out === "#010203" ? null : out;
    } catch (_) {
      return null;
    }
  };
  // The colours one answer sends: `note(c)` passes `c` through, keeping the browser's reading
  // of it in `fallbacks` when Rust can't read its syntax.
  const newReadings = () => {
    const fallbacks = Object.create(null);
    const seen = new Set();
    const note = (c) => {
      if (typeof c !== "string" || seen.has(c)) return c;
      seen.add(c);
      if (!READ_IN_RUST.test(c.trim().toLowerCase())) fallbacks[c] = browserReading(c);
      return c;
    };
    return { note, fallbacks };
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
