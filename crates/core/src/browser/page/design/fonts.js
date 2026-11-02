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
