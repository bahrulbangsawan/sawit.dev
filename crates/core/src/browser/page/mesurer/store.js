  // ---- Persistence ----
  //
  // mesurer's `MesurerPersistence`, in Sawit's files rather than the page's storage: settings
  // app-wide, the workspace (`PagedWorkspaceStore`) per project and origin. `load()` is
  // synchronous: the snapshot the app read before mounting. Saves keep the value here and post it
  // (`store.*`); the app writes it, debounced. Changes made elsewhere come back through
  // `SawitMesurer.apply(snapshot, source)`.
  let stored = { settings: {}, workspace: null };
  const storeListeners = new Set();
  let storeErrors = null;

  const settle = (promise) => {
    promise.catch((e) => {
      log("warn", "mesurer's save failed:", e);
      if (storeErrors) storeErrors(e);
    });
  };
  const snapshotOf = () => ({ settings: stored.settings, workspace: stored.workspace });
  const preload = (snapshot) => {
    const settings = snapshot && snapshot.settings && typeof snapshot.settings === "object" ? snapshot.settings : {};
    const workspace = snapshot && snapshot.workspace && typeof snapshot.workspace === "object" ? snapshot.workspace : null;
    stored = { settings, workspace };
  };

  const persistence = Object.freeze({
    load: () => snapshotOf(),
    saveSettings(settings) {
      stored = { ...stored, settings: settings || {} };
      settle(bridge("store.settings", stored.settings));
    },
    saveWorkspace(workspace) {
      stored = { ...stored, workspace: workspace || null };
      settle(bridge("store.workspace", stored.workspace));
    },
    clearWorkspace() {
      stored = { ...stored, workspace: null };
      settle(bridge("store.clear", { what: "workspace" }));
    },
    clearSettings() {
      stored = { ...stored, settings: {} };
      settle(bridge("store.clear", { what: "settings" }));
    },
    subscribe(listener) {
      storeListeners.add(listener);
      return () => {
        storeListeners.delete(listener);
      };
    },
    setErrorHandler(handler) {
      storeErrors = typeof handler === "function" ? handler : null;
    },
  });

  // The app's snapshot replaces what is here and mesurer is told (`source`: which half changed,
  // both when left out): an agent's `set` or `clear`, a comment row removed in the panel or put
  // back by Undo. The files are written already and mesurer saves nothing back for an apply, so
  // its next save starts from these pages.
  //
  // A known race: mesurer saves 250 ms after its last change. An apply inside that window
  // replaces the page mesurer holds with the app's, which hasn't seen that change: the user's
  // last 250 ms are lost. Nothing here can see an unsaved change; the app's pushes are rare.
  const applySnapshot = (snapshot, source) => {
    preload(snapshot);
    const which = source && typeof source === "object"
      ? { settings: Boolean(source.settings), workspace: Boolean(source.workspace) }
      : { settings: true, workspace: true };
    for (const listener of [...storeListeners]) {
      try {
        listener(snapshotOf(), which);
      } catch (e) {
        log("error", "mesurer couldn't apply Sawit's snapshot:", e);
      }
    }
    return { applied: true, listeners: storeListeners.size };
  };

  // `layout.show`: mesurer's layout guides on screen after an agent set some. mesurer keeps
  // whether they show in unsaved state, so Sawit presses its Layout guides button (as L does,
  // which also opens their panel), unless they show already.
  const LAYOUT_SHOWN = "[data-mesurer-layout-guides]";
  HANDLERS["layout.show"] = async () => {
    if (!shadow) return { shown: false, reason: "mesurer isn't on the page" };
    if (shadow.querySelector(LAYOUT_SHOWN)) return { shown: true };
    const buttons = [...shadow.querySelectorAll("button")];
    const label = (b) => b.getAttribute("aria-label") || "";
    const button = buttons.find((b) => label(b).startsWith("Layout guides (")) || buttons.find((b) => label(b) === "Layout guides");
    if (!button) return { shown: false, reason: "mesurer's toolbar is minimized" };
    button.click();
    for (let i = 0; i < 20 && !shadow.querySelector(LAYOUT_SHOWN); i++) await new Promise((r) => setTimeout(r, 25));
    return { shown: Boolean(shadow.querySelector(LAYOUT_SHOWN)) };
  };
