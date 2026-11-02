  // Page modules (see browser.rs `page_script`) add ops and share helpers.
  const modules = {};
  const helpers = {};
  /*__SAWIT_MODULES__*/

  const OPS = { snapshot, click, hover, type, press, select, scroll, query, rect, evaluate, settled, ...modules };
  const run = (op) => {
    try {
      const f = OPS[op && op.op];
      if (!f) throw new Error("Unknown op “" + (op && op.op) + "”.");
      return JSON.stringify(Object.assign({ ok: true }, f(op)));
    } catch (e) {
      return JSON.stringify({ ok: false, error: String((e && e.message) || e) });
    }
  };
  Object.defineProperty(window, "__sawitAgent", { value: Object.freeze({ run }), writable: false, configurable: false, enumerable: false });
