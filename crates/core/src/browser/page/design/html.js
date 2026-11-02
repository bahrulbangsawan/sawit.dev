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
