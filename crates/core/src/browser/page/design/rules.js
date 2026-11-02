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
