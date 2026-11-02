  // ---- URL changes without a load ----

  const notifyUrl = () => send("url", location.href);
  for (const name of ["pushState", "replaceState"]) {
    const original = history[name];
    history[name] = function () {
      const result = original.apply(this, arguments);
      notifyUrl();
      return result;
    };
  }
  addEventListener("popstate", notifyUrl);
  addEventListener("hashchange", notifyUrl);
