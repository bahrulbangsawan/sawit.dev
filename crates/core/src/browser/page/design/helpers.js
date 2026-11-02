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
