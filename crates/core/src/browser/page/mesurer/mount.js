  // ---- Mount ----
  //
  // `SawitMesurer`, what the app calls in this world (`callAsyncJavaScript`, each answer made a
  // JSON string by the caller):
  //
  //   mount({props, snapshot})   builds the host and renders mesurer (idempotent); `snapshot` is
  //                              {settings, workspace} as read from Sawit's files; posts `ready`
  //                              once mesurer's tree is in the shadow root
  //   unmount()                  unmounts (mesurer saves its workspace first) and removes the host
  //   update(props)              renders again with these props over the ones it has
  //   apply(snapshot, source)    Sawit's files changed: mesurer reloads settings and workspace
  //   hide(mode)                 "all", "chrome" or "none" (see host.js)
  //   isMounted()
  //   call(name, args)           runs `HANDLERS[name](args)`, which the lanes fill
  //
  // Props, merged in this order: Sawit's (persistence in Sawit's files, workspace kept across
  // reloads, no reviews); on a first visit (no saved workspace), mesurer open with no tool, so a
  // click still makes a Sawit note; the lanes' `PROPS`; then the app's `props` (its theme).
  const BUNDLE = globalThis.__sawitMesurerBundle;
  const TREE_WAIT_MS = 5000;
  let mountedNow = false;

  // Resolves with how long mesurer's tree took to appear in `root` (`null` once it is unmounted).
  const treeIn = (root, since) =>
    new Promise((resolve) => {
      const look = () => {
        if (!mountedNow) return resolve(null);
        const ms = performance.now() - since;
        const there = Boolean(root.querySelector("[data-mesurer-root]"));
        if (there || ms >= TREE_WAIT_MS) resolve({ there, ms: Math.round(ms * 10) / 10 });
        else setTimeout(look, 16);
      };
      look();
    });

  const mount = async (args) => {
    if (!BUNDLE) throw new Error("mesurer's bundle isn't in this world.");
    const options = args && typeof args === "object" ? args : {};
    if (mountedNow) return { mounted: true, version: BUNDLE.version, again: true };
    const since = performance.now();
    preload(options.snapshot);
    const { shadow: root, container: box } = buildHost();
    const props = Object.assign(
      { persistence, persistSession: true, persistOnReload: true, features: { review: false } },
      stored.workspace ? null : { initialState: { enabled: true, toolMode: "none" } },
      PROPS,
      options.props && typeof options.props === "object" ? options.props : null,
    );
    try {
      BUNDLE.render(box, root, props);
    } catch (e) {
      removeHost();
      throw e;
    }
    mountedNow = true;
    treeIn(root, since).then((tree) => {
      if (!tree) return;
      if (!tree.there) log("warn", `mesurer's tree didn't appear within ${TREE_WAIT_MS} ms`);
      bridge("ready", { version: BUNDLE.version, url: location.href, mount_ms: tree.ms }).catch(() => {});
    });
    return { mounted: true, version: BUNDLE.version };
  };

  const unmount = () => {
    const was = mountedNow;
    mountedNow = false;
    try {
      if (was) BUNDLE.unmount();
    } finally {
      removeHost();
    }
    return { mounted: false, was };
  };

  const call = async (name, args) => {
    const handler = HANDLERS[name];
    if (typeof handler !== "function") throw new Error(`Sawit's mesurer has no “${name}”.`);
    return handler(args === undefined ? {} : args);
  };

  if (!("SawitMesurer" in globalThis)) {
    Object.defineProperty(globalThis, "SawitMesurer", {
      value: Object.freeze({
        version: BUNDLE ? BUNDLE.version : null,
        commit: BUNDLE ? BUNDLE.commit : null,
        mount,
        unmount,
        update: (props) => {
          if (mountedNow) BUNDLE.update(props && typeof props === "object" ? props : {});
          return { mounted: mountedNow };
        },
        apply: (snapshot, source) => applySnapshot(snapshot, source),
        hide: (mode) => hide(mode),
        isMounted: () => mountedNow,
        call,
      }),
      configurable: false,
      enumerable: false,
      writable: false,
    });
  }
