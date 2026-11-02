  // ---- Elements ----

  let refs = new Map();
  let nextRef = 0;
  addEventListener("pagehide", () => (refs = new Map()));

  const deepQuery = (selector, root = document) => {
    const found = root.querySelector(selector);
    if (found) return found;
    for (const el of root.querySelectorAll("*")) {
      if (el.shadowRoot) {
        const inner = deepQuery(selector, el.shadowRoot);
        if (inner) return inner;
      }
    }
    return null;
  };

  const resolve = (op) => {
    if (op.ref) {
      const el = refs.get(op.ref);
      if (!el) throw new Error("No element " + op.ref + ". Take a new browser_snapshot (refs reset on every snapshot and navigation).");
      if (!el.isConnected) throw new Error("Element " + op.ref + " is no longer on the page. Take a new browser_snapshot.");
      return el;
    }
    if (op.selector) {
      let el;
      try {
        el = deepQuery(op.selector);
      } catch (_) {
        throw new Error("“" + op.selector + "” isn't a valid CSS selector.");
      }
      if (!el) throw new Error("Nothing on the page matches “" + op.selector + "”.");
      return el;
    }
    return null;
  };
  const required = (op) => {
    const el = resolve(op);
    if (!el) throw new Error("Give a ref (from browser_snapshot) or a CSS selector.");
    return el;
  };

  const INPUT_ROLES = {
    button: "button", submit: "button", reset: "button", image: "button", file: "button",
    checkbox: "checkbox", radio: "radio", range: "slider", number: "spinbutton", search: "searchbox",
    color: "button", hidden: null,
  };
  const TAG_ROLES = {
    button: "button", textarea: "textbox", h1: "heading", h2: "heading", h3: "heading", h4: "heading",
    h5: "heading", h6: "heading", nav: "navigation", main: "main", aside: "complementary", form: "form",
    ul: "list", ol: "list", menu: "list", li: "listitem", table: "table", tr: "row", th: "columnheader",
    td: "cell", dialog: "dialog", summary: "button", option: "option", article: "article",
    fieldset: "group", progress: "progressbar", meter: "meter", hr: "separator", iframe: "iframe",
    p: "paragraph", label: "label", video: "video", audio: "audio", search: "search",
  };
  const role = (el) => {
    const explicit = (el.getAttribute("role") || "").trim().split(/\s+/)[0];
    if (explicit && explicit !== "presentation" && explicit !== "none") return explicit;
    const tag = el.localName;
    if (tag === "a" || tag === "area") return el.hasAttribute("href") ? "link" : null;
    if (tag === "input") return el.type in INPUT_ROLES ? INPUT_ROLES[el.type] : "textbox";
    if (tag === "select") return el.multiple || el.size > 1 ? "listbox" : "combobox";
    if (tag === "img") return el.getAttribute("alt") === "" ? null : "img";
    if (tag === "svg") return el.getAttribute("aria-label") ? "img" : null;
    if (tag === "section") return el.getAttribute("aria-label") || el.getAttribute("aria-labelledby") ? "region" : null;
    if (tag === "header") return el.closest("article, aside, main, nav, section") ? null : "banner";
    if (tag === "footer") return el.closest("article, aside, main, nav, section") ? null : "contentinfo";
    if (el.isContentEditable && !(el.parentElement && el.parentElement.isContentEditable)) return "textbox";
    return TAG_ROLES[tag] || null;
  };
  const NAME_FROM_CONTENT = new Set([
    "button", "link", "heading", "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "option",
    "cell", "columnheader", "rowheader", "treeitem", "switch", "tooltip", "label", "paragraph",
  ]);
  const LEAF = new Set(["button", "link", "heading", "option", "img", "textbox", "searchbox", "checkbox", "radio", "slider", "spinbutton", "combobox", "switch", "progressbar", "meter", "separator", "iframe", "paragraph", "label"]);
  const textOf = (el) => oneLine(el.innerText !== undefined ? el.innerText : el.textContent);
  const nameOf = (el, r) => {
    const by = el.getAttribute("aria-labelledby");
    if (by) {
      const text = oneLine(by.split(/\s+/).map((id) => (document.getElementById(id) || {}).textContent || "").join(" "));
      if (text) return text;
    }
    const label = el.getAttribute("aria-label");
    if (label && label.trim()) return oneLine(label);
    if (el.labels && el.labels.length) {
      const text = oneLine([...el.labels].map((l) => l.textContent).join(" "));
      if (text) return text;
    }
    if (el.localName === "img" || el.localName === "area" || (el.localName === "input" && el.type === "image")) {
      if (el.alt) return oneLine(el.alt);
    }
    if (el.localName === "input" && ["button", "submit", "reset"].includes(el.type)) {
      return el.value || (el.type === "submit" ? "Submit" : el.type === "reset" ? "Reset" : "");
    }
    if (el.localName === "fieldset") {
      const legend = el.querySelector(":scope > legend");
      if (legend) return textOf(legend);
    }
    if (NAME_FROM_CONTENT.has(r)) {
      const text = textOf(el);
      if (text) return text;
    }
    return oneLine(el.getAttribute("title") || el.getAttribute("placeholder") || "");
  };
  const describe = (el) => {
    if (!(el instanceof Element)) return String(el);
    const r = role(el);
    const name = r ? clip(nameOf(el, r), 60) : "";
    if (r) return r + (name ? " “" + name + "”" : "");
    const id = el.id ? "#" + el.id : "";
    const cls = typeof el.className === "string" && el.className.trim() ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".") : "";
    return el.localName + id + cls;
  };
  const shown = (el) => {
    if (el.getAttribute("aria-hidden") === "true") return false;
    if (el.checkVisibility) return el.checkVisibility({ visibilityProperty: true });
    const style = getComputedStyle(el);
    return style.display !== "none" && style.visibility !== "hidden";
  };
  const clickable = (el) => {
    if (el.hasAttribute("onclick") || (el.tabIndex >= 0 && el.hasAttribute("tabindex"))) return true;
    const cursor = getComputedStyle(el).cursor;
    return cursor === "pointer" && !(el.parentElement && getComputedStyle(el.parentElement).cursor === "pointer");
  };
  const valueOf = (el) => {
    if (el.localName === "select") return [...el.selectedOptions].map((o) => o.label).join(", ");
    if (el.isContentEditable) return textOf(el);
    if (el.type === "password") return el.value ? "•".repeat(Math.min(el.value.length, 8)) : "";
    return el.value;
  };

  const MAX_LINES = 2000;
  const snapshot = () => {
    refs = new Map();
    nextRef = 0;
    const lines = [];
    let truncated = false;
    const focused = document.activeElement;
    const children = (node) => {
      if (node.localName === "slot") return node.assignedNodes({ flatten: true });
      if (node.shadowRoot) return node.shadowRoot.childNodes;
      if (node.localName === "iframe") return [];
      return node.childNodes;
    };
    const line = (depth, text) => {
      if (lines.length >= MAX_LINES) {
        truncated = true;
        return false;
      }
      lines.push("  ".repeat(depth) + "- " + text);
      return true;
    };
    const visit = (node, depth) => {
      if (truncated) return;
      if (node.nodeType === Node.TEXT_NODE) {
        const text = oneLine(node.textContent);
        if (text) line(depth, "text: " + JSON.stringify(clip(text, 200)));
        return;
      }
      if (node.nodeType !== Node.ELEMENT_NODE && node.nodeType !== Node.DOCUMENT_FRAGMENT_NODE) return;
      const el = node;
      if (el.nodeType === Node.ELEMENT_NODE) {
        if (["script", "style", "noscript", "template", "head", "meta", "link", "title"].includes(el.localName)) return;
        if (el.hasAttribute("data-sawit-overlay")) return;
        const contents = getComputedStyle(el).display === "contents";
        if (!contents && !shown(el)) return;
        let r = role(el);
        if (!r && !contents && clickable(el)) r = "clickable";
        if (r) {
          const ref = "e" + ++nextRef;
          refs.set(ref, el);
          const name = r === "clickable" ? clip(textOf(el), 100) : clip(nameOf(el, r), 100);
          let text = r + (name ? " " + JSON.stringify(name) : "");
          if (r === "heading") text += " [level=" + (Number(el.getAttribute("aria-level")) || Number(el.localName.slice(1)) || 2) + "]";
          if (["textbox", "searchbox", "combobox", "listbox", "spinbutton", "slider"].includes(r)) {
            text += " value=" + JSON.stringify(clip(valueOf(el) || "", 200));
          }
          if (el.checked || el.getAttribute("aria-checked") === "true") text += " [checked]";
          if (el.getAttribute("aria-expanded")) text += el.getAttribute("aria-expanded") === "true" ? " [expanded]" : " [collapsed]";
          if (el.getAttribute("aria-selected") === "true" || (r === "option" && el.selected)) text += " [selected]";
          if (el.disabled || el.getAttribute("aria-disabled") === "true") text += " [disabled]";
          if (el.required) text += " [required]";
          if (el === focused) text += " [focused]";
          if (r === "link") {
            const href = el.getAttribute("href") || "";
            if (href && !href.startsWith("javascript:")) text += " /url: " + clip(href, 120);
          }
          text += " [ref=" + ref + "]";
          if (LEAF.has(r) || r === "clickable") {
            line(depth, text);
            // Fields and buttons rarely hold controls; links and clickable
            // cards sometimes do.
            if (["link", "clickable", "label", "paragraph"].includes(r)) visitControlsOnly(el, depth + 1);
            return;
          }
          const at = lines.length;
          if (!line(depth, text + ":")) return;
          for (const child of children(el)) visit(child, depth + 1);
          // Nothing inside: no colon.
          if (lines.length === at + 1) lines[at] = lines[at].slice(0, -1);
          return;
        }
      }
      for (const child of children(el)) visit(child, depth);
    };
    // Only interactive descendants, for elements whose text is their name.
    const visitControlsOnly = (el, depth) => {
      for (const child of el.querySelectorAll("a[href], button, input, select, textarea, [role=button], [role=link], [role=checkbox], [tabindex]")) {
        if (truncated || !shown(child)) continue;
        const r = role(child) || "clickable";
        const ref = "e" + ++nextRef;
        refs.set(ref, child);
        const name = clip(nameOf(child, r), 100);
        line(depth, r + (name ? " " + JSON.stringify(name) : "") + " [ref=" + ref + "]");
      }
    };
    if (document.body) visit(document.body, 0);
    const dialog = document.querySelector("dialog[open]");
    return {
      url: location.href,
      title: document.title,
      ready: document.readyState,
      refs: nextRef,
      truncated,
      modal: dialog ? describe(dialog) : undefined,
      snapshot: lines.join("\n") + (truncated ? "\n… (cut at " + MAX_LINES + " lines; scroll or use a selector)" : ""),
    };
  };
