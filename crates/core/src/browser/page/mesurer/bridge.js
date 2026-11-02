  // ---- Bridge ----
  //
  // World → app: `bridge(kind, payload)` posts `JSON.stringify({kind, payload, url})` to the
  // `sawitMesurer` handler and resolves with the app's JSON reply (`null` for none), or rejects
  // with its message. `url` files a message that lands after a navigation under its own page. The
  // kinds and their payloads are `sawit_core::browser::mesurer::FromMesurer`: ready, log,
  // store.{settings,workspace,clear}, clipboard.{text,image}, file.save, capture.{visible,frame},
  // color.{sample,cancel} (lane 1), comments.{copied,resolved} (lane 2), ui.state, link.open
  // (lane 3).
  //
  // App → world: `SawitMesurer` (mount.js). Lane files add mesurer props to `PROPS`, and what
  // `SawitMesurer.call(name, args)` runs to `HANDLERS`. The handler is taken now, before anything
  // else runs in this world.
  const HANDLER = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.sawitMesurer;
  const bridge = (kind, payload) => {
    if (!HANDLER) return Promise.reject(new Error("Sawit's bridge isn't there"));
    let message;
    try {
      message = JSON.stringify({ kind, payload: payload === undefined ? {} : payload, url: location.href });
    } catch (e) {
      return Promise.reject(e instanceof Error ? e : new Error(String(e)));
    }
    return HANDLER.postMessage(message).then(
      (reply) => (typeof reply === "string" && reply !== "" ? JSON.parse(reply) : null),
      (e) => {
        throw e instanceof Error ? e : new Error(String((e && e.message) || e));
      },
    );
  };
  // A line for Sawit's log; never throws, never waits.
  const log = (level, ...parts) => {
    const text = parts
      .map((p) => (p instanceof Error ? `${p.name}: ${p.message}` : typeof p === "string" ? p : safeJson(p)))
      .join(" ");
    bridge("log", { level, text }).catch(() => {});
  };
  const safeJson = (value) => {
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  };
  // Props the lanes give mesurer (`captureVisibleTab`, …), merged under the app's own.
  const PROPS = Object.create(null);
  // What `SawitMesurer.call(name, args)` runs; each answers a JSON-able value or a promise of one.
  const HANDLERS = Object.create(null);
