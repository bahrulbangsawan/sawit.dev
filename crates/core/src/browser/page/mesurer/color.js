  // ---- Colour sampling (lane 1: mesurer-capture) ----
  //
  // WebKit has no `EyeDropper`: this world's opens macOS's colour sampler (`color.sample`,
  // NSColorSampler) and resolves with {sRGBHex}; it rejects with an AbortError when the user
  // cancels (Esc), or when its signal aborts (`color.cancel`).
  const abortError = () => new DOMException("The colour sampling was cancelled.", "AbortError");
  class EyeDropperStandIn {
    open(options) {
      const signal = options && options.signal;
      return new Promise((resolve, reject) => {
        if (signal && signal.aborted) return reject(abortError());
        let done = false;
        const settle = (fn, value) => {
          if (done) return;
          done = true;
          if (signal) signal.removeEventListener("abort", onAbort);
          fn(value);
        };
        const onAbort = () => {
          settle(reject, abortError());
          bridge("color.cancel").catch(() => {});
        };
        if (signal) signal.addEventListener("abort", onAbort);
        bridge("color.sample").then(
          (picked) => (picked && typeof picked.sRGBHex === "string" ? settle(resolve, { sRGBHex: picked.sRGBHex }) : settle(reject, abortError())),
          (e) => settle(reject, e),
        );
      });
    }
  }
  Object.defineProperty(window, "EyeDropper", method(EyeDropperStandIn));
