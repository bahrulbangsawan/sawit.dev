  const handlers = window.webkit && window.webkit.messageHandlers;
  const ipc = handlers && handlers.ipc;
  if (window.__sawitAgent || !ipc || typeof ipc.postMessage !== "function") return;
  const TOKEN = "__SAWIT_TOKEN__";
  // The handler's own `postMessage` and `Reflect.apply`, as they are before any page script
  // runs. wry's `window.ipc` shim looks the handler's method up on every call, so a page
  // that patched it (on the handler's prototype) would read every signed message.
  const postMessage = ipc.postMessage;
  const apply = Reflect.apply;
  const post = (message) => apply(postMessage, ipc, [message]);
  const send = (kind, data) => {
    try {
      post(TOKEN + "\n" + kind + "\n" + JSON.stringify(data));
    } catch (_) {}
  };
  // The user's press in the page gives it AppKit's keyboard, which the app follows: the ⌘ keys it
  // sees first (⌘C, ⌘V…) then go to the page, not to the field that had the keyboard before. A
  // script's press (the agent's own clicks, or the page's) moves neither keyboard: it isn't sent.
  addEventListener("pointerdown", (e) => { if (e.isTrusted) send("pressed", true); }, { capture: true, passive: true });
  const now = () => performance.now();
  const clip = (text, max) =>
    text.length > max ? text.slice(0, max) + "… (" + text.length + " chars)" : text;
  const oneLine = (text) => (text || "").replace(/\s+/g, " ").trim();
  const absolute = (url) => {
    try {
      return new URL(url, location.href).href;
    } catch (_) {
      return String(url);
    }
  };
