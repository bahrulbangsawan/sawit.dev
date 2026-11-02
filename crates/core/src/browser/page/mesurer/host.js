  // ---- Host ----
  //
  // mesurer renders in a shadow root on `<div id="mesurer-extension-host">` (its core looks the
  // host up by that id), on `<html>`, as its extension mounts it (apps/extension/src/host.ts):
  // the same locked styles, re-applied and re-attached when the page moves or restyles it, one
  // step under Sawit's own overlay (z-index 2147483646), so Sawit's popover, markers and agents'
  // cursors stay on top. `data-sawit-overlay` keeps it out of Sawit's snapshots, picking and
  // design scans; `data-sawit-mesurer` tells Sawit's picker that mesurer has the pointer.
  //
  // The shadow root is open, as the extension's is. A closed one can't work with mesurer's
  // keyboard gate: the gate runs in the page's world (gate.js), where a closed root's insides are
  // out of `composedPath()` and `host.shadowRoot` is null, so it would take every key typed into a
  // mesurer field for the page's and stop it. What an open root leaves to a page script is
  // mesurer's DOM (reading or changing what it shows); this world's objects, the bridge and what
  // the app receives stay out of its reach.
  const HOST_ID = "mesurer-extension-host";
  const ROOT_ID = "mesurer-extension-root";
  const HOST_STYLES = [
    ["all", "initial"], ["display", "block"], ["position", "fixed"], ["inset", "0px"],
    ["top", "0px"], ["right", "0px"], ["bottom", "0px"], ["left", "0px"],
    ["width", "100vw"], ["height", "100vh"], ["max-width", "none"], ["max-height", "none"],
    ["min-width", "0px"], ["min-height", "0px"], ["margin", "0px"], ["padding", "0px"],
    ["border", "0px"], ["outline", "none"], ["overflow", "visible"], ["opacity", "1"],
    ["visibility", "visible"], ["pointer-events", "none"], ["z-index", "2147483646"],
    ["isolation", "isolate"], ["transform", "none"], ["translate", "none"], ["rotate", "none"],
    ["scale", "none"], ["filter", "none"], ["backdrop-filter", "none"],
    ["mix-blend-mode", "normal"], ["clip-path", "none"], ["mask-image", "none"], ["zoom", "1"],
    ["contain", "none"], ["background", "transparent"], ["box-shadow", "none"],
    ["color-scheme", "light"], ["color", "#0f172a"],
    ["font-family", "ui-sans-serif, system-ui, sans-serif"], ["font-size", "16px"],
    ["font-weight", "400"], ["line-height", "1.5"], ["letter-spacing", "normal"],
    ["direction", "ltr"],
  ];
  // What `hide("chrome")` hides: the toolbar, panels, menus and cards; drawings, guides,
  // measurements and comment pins stay.
  const CHROME_CSS = `
    .mesurer-toolbar-container, [data-mesurer-settings-panel], [data-mesurer-layout-guides-panel],
    [data-mesurer-inspect-info-card], [data-mesurer-text-card], [data-mesurer-pen-card],
    [data-mesurer-recording-card], [data-mesurer-comment-hover-card], [data-mesurer-comment-popover],
    [data-mesurer-comment-overflow-menu], [data-mesurer-comment-delete-confirmation],
    [data-mesurer-guide-menu], [data-mesurer-group-controls], [data-mesurer-motion-player],
    [data-mesurer-format-select], [role="menu"], [role="listbox"], [role="dialog"], [role="tooltip"] {
      visibility: hidden !important;
    }`;

  let host = null;
  let shadow = null;
  let container = null;
  let observer = null;
  let chromeSheet = null;
  let hiddenMode = "none";
  let relocking = false;

  const lockHost = () => {
    if (!host) return;
    host.removeAttribute("hidden");
    host.removeAttribute("inert");
    for (const [property, value] of HOST_STYLES) host.style.setProperty(property, value, "important");
    host.style.setProperty("display", hiddenMode === "all" ? "none" : "block", "important");
  };
  const attachHost = () => {
    const root = document.documentElement;
    if (host && root && host.parentNode !== root) root.appendChild(host);
  };
  const observeRoots = () => {
    if (!observer || !host) return;
    observer.disconnect();
    observer.observe(document, { childList: true });
    if (document.documentElement) observer.observe(document.documentElement, { childList: true });
    if (document.body) observer.observe(document.body, { childList: true });
    observer.observe(host, { attributes: true, attributeFilter: ["style", "class", "hidden", "inert"] });
    if (shadow) observer.observe(shadow, { childList: true });
  };
  // The page replaced `<html>`'s children, restyled the host or emptied the shadow root: put it
  // all back. Once per microtask, as its own changes call this again.
  const relock = () => {
    if (relocking || !host) return;
    relocking = true;
    try {
      lockHost();
      attachHost();
      if (shadow && container && container.parentNode !== shadow) shadow.appendChild(container);
      observeRoots();
    } finally {
      queueMicrotask(() => {
        relocking = false;
      });
    }
  };

  // The host, its shadow root and the container mesurer renders in; made once per page.
  const buildHost = () => {
    if (host) {
      relock();
      return { host, shadow, container };
    }
    host = document.createElement("div");
    host.id = HOST_ID;
    host.setAttribute("data-sawit-overlay", "");
    host.setAttribute("data-sawit-mesurer", "");
    shadow = host.attachShadow({ mode: "open" });
    container = document.createElement("div");
    container.id = ROOT_ID;
    shadow.appendChild(container);
    try {
      chromeSheet = new CSSStyleSheet();
      chromeSheet.replaceSync(CHROME_CSS);
      chromeSheet.disabled = true;
      shadow.adoptedStyleSheets = [...shadow.adoptedStyleSheets, chromeSheet];
    } catch (e) {
      chromeSheet = null;
      log("warn", "mesurer's chrome sheet couldn't be made:", e);
    }
    lockHost();
    attachHost();
    observer = new MutationObserver(relock);
    observeRoots();
    if (document.getElementById(HOST_ID) !== host) log("warn", `another #${HOST_ID} is on the page`);
    return { host, shadow, container };
  };
  const removeHost = () => {
    if (observer) observer.disconnect();
    observer = null;
    if (host) host.remove();
    host = null;
    shadow = null;
    container = null;
    chromeSheet = null;
    hiddenMode = "none";
  };

  // X-ray outlines the page's own elements, from a `<style>` mesurer puts in the document
  // (`#mesurer-xray-styles`): hidden with the rest by `hide("all")`.
  const XRAY_STYLE_ID = "mesurer-xray-styles";
  const syncXray = () => {
    const style = document.getElementById(XRAY_STYLE_ID);
    if (!style) return;
    if (hiddenMode === "all") style.setAttribute("media", "not all");
    else if (style.getAttribute("media") === "not all") style.removeAttribute("media");
  };

  // `all`: nothing of mesurer shows (agents' and notes' screenshots); `chrome`: its drawings
  // show, its toolbar, panels, menus and cards don't; `none`: everything shows.
  const hide = (mode) => {
    if (mode !== "all" && mode !== "chrome" && mode !== "none") throw new Error(`Unknown hide mode “${mode}”.`);
    hiddenMode = mode;
    if (chromeSheet) chromeSheet.disabled = mode !== "chrome";
    lockHost();
    syncXray();
    return { hidden: mode, mounted: Boolean(host) };
  };
