  // ---- Console and page errors ----

  const format = (value) => {
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.stack || String(value);
    if (value instanceof Element) return describe(value);
    try {
      const json = JSON.stringify(value);
      return json === undefined ? String(value) : json;
    } catch (_) {
      return String(value);
    }
  };
  const LEVELS = { log: "log", info: "info", warn: "warning", error: "error", debug: "debug", trace: "debug" };
  for (const name of Object.keys(LEVELS)) {
    const original = console[name];
    if (typeof original !== "function") continue;
    console[name] = function (...args) {
      send("console", { level: LEVELS[name], text: clip(args.map(format).join(" "), 4000), time_ms: now() });
      return original.apply(this, args);
    };
  }
  const failedResources = new Set();
  addEventListener(
    "error",
    (event) => {
      const target = event.target;
      if (target && target !== window && target instanceof Element) {
        const url = target.currentSrc || target.src || target.href;
        if (url) {
          failedResources.add(url);
          send("network", { method: "GET", url: String(url), kind: target.localName, error: "Failed to load", time_ms: now() });
        }
        return;
      }
      const text = (event.error && event.error.stack) || event.message || "Script error";
      send("console", { level: "error", text: clip(text, 4000), url: event.filename || undefined, line: event.lineno || undefined, time_ms: now() });
    },
    true,
  );
  addEventListener("unhandledrejection", (event) => {
    send("console", { level: "error", text: clip("Unhandled promise rejection: " + format(event.reason), 4000), time_ms: now() });
  });
