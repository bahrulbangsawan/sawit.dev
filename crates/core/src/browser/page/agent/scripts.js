  // ---- Scripts ----

  const settledResults = new Map();
  let nextPromise = 0;
  const toJson = (value) => {
    if (value === undefined) return null;
    const seen = new WeakSet();
    const text = JSON.stringify(value, (_, v) => {
      if (v instanceof Element) return "[" + describe(v) + "]";
      if (typeof v === "function") return "[function]";
      if (typeof v === "bigint") return v.toString();
      if (v && typeof v === "object") {
        if (seen.has(v)) return "[circular]";
        seen.add(v);
      }
      return v;
    });
    if (text === undefined) return String(value);
    return text.length > 20000 ? clip(text, 20000) : JSON.parse(text);
  };
  // `op.fn` is the agent's script as Sawit's call compiled it (`evaluate_call` in
  // browser.rs): compiling it here would be an `eval`, which a page's CSP may forbid.
  const evaluate = (op) => {
    if (typeof op.fn !== "function") throw new Error("Nothing to evaluate.");
    const value = apply(op.fn, window, []);
    if (value && typeof value.then === "function") {
      const id = ++nextPromise;
      settledResults.set(id, { done: false });
      value.then(
        (v) => settledResults.set(id, { done: true, ok: true, value: v }),
        (e) => settledResults.set(id, { done: true, ok: false, error: String((e && e.message) || e) }),
      );
      return { pending: id };
    }
    return { value: toJson(value) };
  };
  const settled = (op) => {
    const state = settledResults.get(op.id);
    if (!state) throw new Error("That promise belongs to a page that has since gone; run the script again.");
    if (!state.done) return { done: false };
    settledResults.delete(op.id);
    if (!state.ok) throw new Error(state.error);
    return { done: true, value: toJson(state.value) };
  };
