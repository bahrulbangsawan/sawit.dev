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
