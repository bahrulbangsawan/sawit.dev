  // ---- Keyboard and text ----

  const TEXT_INPUTS = new Set(["text", "search", "email", "url", "tel", "password", "number", ""]);
  const editable = (el) =>
    el.isContentEditable ||
    el.localName === "textarea" ||
    (el.localName === "input" && (TEXT_INPUTS.has(el.type) || ["date", "time", "datetime-local", "month", "week"].includes(el.type)));
  const setValue = (el, value) => {
    const proto = el.localName === "textarea" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
    el.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "insertText", data: value }));
  };
  const selectAll = (el) => {
    if (el.isContentEditable) {
      const range = document.createRange();
      range.selectNodeContents(el);
      const selection = getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    } else {
      try {
        el.select();
      } catch (_) {}
    }
  };
  const caretToEnd = (el) => {
    if (el.isContentEditable) {
      const range = document.createRange();
      range.selectNodeContents(el);
      range.collapse(false);
      const selection = getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    } else {
      try {
        el.setSelectionRange(el.value.length, el.value.length);
      } catch (_) {}
    }
  };
  const currentText = (el) => (el.isContentEditable ? el.textContent : el.value);

  const type = (op) => {
    const el = resolve(op) || document.activeElement;
    if (!el || el === document.body || el === document.documentElement) throw new Error("Nothing is focused. Give a ref or selector to type into.");
    if (!editable(el)) throw new Error(describe(el) + " doesn't take text.");
    if (el.disabled || el.readOnly) throw new Error(describe(el) + " is " + (el.disabled ? "disabled" : "read-only") + ".");
    el.scrollIntoView({ block: "center", behavior: "instant" });
    el.focus({ preventScroll: true });
    if (op.clear) {
      selectAll(el);
      if (!document.execCommand("delete") && !el.isContentEditable && el.value) setValue(el, "");
    } else {
      caretToEnd(el);
    }
    const before = currentText(el);
    let inserted = false;
    try {
      inserted = document.execCommand("insertText", false, op.text) && currentText(el) !== before;
    } catch (_) {}
    if (!inserted && op.text) {
      if (el.isContentEditable) el.textContent = before + op.text;
      else setValue(el, before + op.text);
    }
    if (!el.isContentEditable) el.dispatchEvent(new Event("change", { bubbles: true }));
    const result = { typed: op.text.length, into: describe(el), value: clip(valueOf(el) || "", 200) };
    if (op.submit) Object.assign(result, press({ key: "Enter" }));
    return result;
  };

  const KEY_CODES = {
    Enter: "Enter", Tab: "Tab", Escape: "Escape", Backspace: "Backspace", Delete: "Delete", " ": "Space",
    ArrowUp: "ArrowUp", ArrowDown: "ArrowDown", ArrowLeft: "ArrowLeft", ArrowRight: "ArrowRight",
    Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown",
  };
  const KEY_ALIASES = { esc: "Escape", return: "Enter", space: " ", up: "ArrowUp", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight", del: "Delete" };
  const tabbables = () =>
    [...document.querySelectorAll("a[href], button, input, select, textarea, [tabindex], [contenteditable]")].filter(
      (el) => el.tabIndex >= 0 && !el.disabled && shown(el) && el.getClientRects().length,
    );
  const press = (op) => {
    const parts = String(op.key).split("+");
    let key = parts.pop() || "+";
    if (key === "" && parts.length) key = "+";
    key = KEY_ALIASES[key.toLowerCase()] || (key.length === 1 ? key : key[0].toUpperCase() + key.slice(1));
    if (key.length > 1 && !KEY_CODES[key] && !/^F\d+$/.test(key)) throw new Error("Unknown key “" + op.key + "”. Use Enter, Tab, Escape, Backspace, Delete, Space, arrows, Home, End, PageUp, PageDown or one character.");
    const mods = new Set(parts.map((p) => p.toLowerCase()));
    const MODIFIERS = ["meta", "cmd", "command", "ctrl", "control", "alt", "option", "shift"];
    for (const mod of mods) {
      if (!MODIFIERS.includes(mod)) throw new Error("Unknown modifier “" + mod + "”. Use Meta, Ctrl, Alt or Shift.");
    }
    const meta = mods.has("meta") || mods.has("cmd") || mods.has("command");
    const ctrl = mods.has("ctrl") || mods.has("control");
    const alt = mods.has("alt") || mods.has("option");
    const shift = mods.has("shift");
    const target = document.activeElement || document.body;
    const code = KEY_CODES[key] || (/^[a-z]$/i.test(key) ? "Key" + key.toUpperCase() : /^\d$/.test(key) ? "Digit" + key : "");
    const init = { key, code, bubbles: true, cancelable: true, composed: true, metaKey: meta, ctrlKey: ctrl, altKey: alt, shiftKey: shift };
    const proceed = target.dispatchEvent(new KeyboardEvent("keydown", init));
    if (proceed) {
      const textField = editable(target);
      if (key.length === 1 && !meta && !ctrl) {
        if (target.dispatchEvent(new KeyboardEvent("keypress", init)) && textField) document.execCommand("insertText", false, key);
        else if (key === " " && (target.localName === "button" || target.type === "checkbox" || target.type === "radio")) target.click();
      } else if (key === "a" && meta) {
        if (textField) selectAll(target);
        else document.execCommand("selectAll");
      } else if (key === "Enter") {
        if (target.localName === "textarea" || target.isContentEditable) {
          document.execCommand("insertLineBreak");
        } else if (target.form && target.localName === "input") {
          target.form.requestSubmit();
        } else if (target.localName === "a" || target.localName === "button" || target.getAttribute("role") === "button") {
          target.click();
        }
      } else if (key === "Backspace" && textField) {
        document.execCommand("delete");
      } else if (key === "Delete" && textField) {
        document.execCommand("forwardDelete");
      } else if (key === "Tab") {
        const all = tabbables();
        const at = all.indexOf(target);
        const next = all[(at + (shift ? -1 : 1) + all.length) % all.length];
        if (next) next.focus();
      } else if (!textField && ["PageDown", "PageUp", "Home", "End", "ArrowDown", "ArrowUp", " "].includes(key)) {
        const page = innerHeight * 0.85;
        const by = { PageDown: page, PageUp: -page, ArrowDown: 40, ArrowUp: -40, " ": page }[key];
        if (key === "Home") scrollTo(scrollX, 0);
        else if (key === "End") scrollTo(scrollX, document.documentElement.scrollHeight);
        else scrollBy(0, by);
      }
    }
    target.dispatchEvent(new KeyboardEvent("keyup", init));
    return { pressed: op.key, focused: describe(document.activeElement || document.body), url: location.href };
  };

  const select = (op) => {
    const el = required(op);
    if (el.localName !== "select") throw new Error(describe(el) + " isn't a <select>; click it and pick the option instead.");
    const options = [...el.options];
    const wanted = op.values.map(String);
    const picked = wanted.map((w) => {
      const match =
        options.find((o) => o.value === w) ||
        options.find((o) => o.label.trim() === w.trim()) ||
        options.find((o) => o.label.trim().toLowerCase() === w.trim().toLowerCase());
      if (!match) throw new Error("No option “" + w + "”. Options: " + options.slice(0, 30).map((o) => JSON.stringify(o.label.trim())).join(", "));
      return match;
    });
    if (!el.multiple && picked.length > 1) throw new Error(describe(el) + " takes one option.");
    for (const o of options) o.selected = picked.includes(o);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
    return { selected: picked.map((o) => o.label.trim()) };
  };

  const scroll = (op) => {
    const el = resolve(op);
    const dx = Number(op.dx) || 0;
    const dy = Number(op.dy) || 0;
    if (el && !dx && !dy) {
      el.scrollIntoView({ block: "center", behavior: "instant" });
    } else {
      (el || document.scrollingElement || document.documentElement).scrollBy({ left: dx, top: dy, behavior: "instant" });
    }
    const page = document.scrollingElement || document.documentElement;
    return {
      scroll_x: Math.round(scrollX),
      scroll_y: Math.round(scrollY),
      page_height: page.scrollHeight,
      viewport_height: innerHeight,
      at_bottom: scrollY + innerHeight >= page.scrollHeight - 2,
    };
  };

  const query = (op) => {
    let found = true;
    if (op.text) found = found && !!document.body && document.body.innerText.toLowerCase().includes(String(op.text).toLowerCase());
    if (op.selector) {
      let el = null;
      try {
        el = deepQuery(op.selector);
      } catch (_) {
        throw new Error("“" + op.selector + "” isn't a valid CSS selector.");
      }
      found = found && !!el && shown(el);
    }
    return { found, url: location.href, title: document.title, ready: document.readyState };
  };

  const rect = (op) => {
    const el = required(op);
    el.scrollIntoView({ block: "center", behavior: "instant" });
    const r = el.getBoundingClientRect();
    const x = Math.max(r.left, 0);
    const y = Math.max(r.top, 0);
    return { x, y, width: Math.min(r.right, innerWidth) - x, height: Math.min(r.bottom, innerHeight) - y };
  };
