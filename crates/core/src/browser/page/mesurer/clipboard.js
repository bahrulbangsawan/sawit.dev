  // ---- Clipboard ----
  //
  // mesurer copies text (`writeText`) and region screenshots (`write` of a ClipboardItem holding a
  // promised PNG). WebKit wants a user gesture for both, and has no clipboard on a page that isn't
  // a secure context, so this world's go to the app (`clipboard.text`, `clipboard.image`). The
  // page's own `Clipboard.prototype` is another object. This world's `ClipboardItem` keeps what it
  // was made with, so a promised PNG is read once it is ready.
  const bytesToBase64 = (bytes) => {
    if (typeof bytes.toBase64 === "function") return bytes.toBase64();
    let text = "";
    for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(text);
  };
  const base64ToBytes = (text) => {
    if (typeof Uint8Array.fromBase64 === "function") return Uint8Array.fromBase64(text);
    const raw = atob(text);
    const bytes = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
    return bytes;
  };
  const blobBase64 = async (blob) => bytesToBase64(new Uint8Array(await blob.arrayBuffer()));
  const copyText = (text) => bridge("clipboard.text", { text: String(text) }).then(() => undefined);

  const ITEMS = new WeakMap();
  class ClipboardItemStandIn {
    constructor(items, options) {
      if (!items || typeof items !== "object") throw new TypeError("ClipboardItem takes an object of types.");
      ITEMS.set(this, { values: Object.assign({}, items), style: (options && options.presentationStyle) || "unspecified" });
    }
    get types() {
      return Object.freeze(Object.keys(ITEMS.get(this).values));
    }
    get presentationStyle() {
      return ITEMS.get(this).style;
    }
    getType(type) {
      const values = ITEMS.get(this).values;
      if (!(type in values)) return Promise.reject(new DOMException(`No ${type} in this item.`, "NotFoundError"));
      return Promise.resolve(values[type]).then((v) => (v instanceof Blob ? v : new Blob([String(v)], { type })));
    }
    static supports(type) {
      return type === "image/png" || type === "text/plain";
    }
  }
  Object.defineProperty(window, "ClipboardItem", { value: ClipboardItemStandIn, configurable: true, writable: true });

  // The first item's PNG to the pasteboard, else its text.
  const writeItems = async (items) => {
    const item = Array.from(items || [])[0];
    const types = item ? Array.from(item.types || []) : [];
    if (types.includes("image/png")) {
      const png = await item.getType("image/png");
      await bridge("clipboard.image", { png_base64: await blobBase64(png) });
    } else if (types.includes("text/plain")) {
      await copyText(await (await item.getType("text/plain")).text());
    } else {
      throw new DOMException("Sawit copies PNG images and text.", "NotAllowedError");
    }
  };
  const method = (value) => ({ value, configurable: true, writable: true });
  if (typeof Clipboard === "function" && Clipboard.prototype) {
    Object.defineProperty(Clipboard.prototype, "writeText", method(function writeText(text) {
      return copyText(text);
    }));
    Object.defineProperty(Clipboard.prototype, "write", method(function write(items) {
      return writeItems(items);
    }));
  }
  if (!navigator.clipboard) {
    // Not a secure context: mesurer still gets a clipboard.
    Object.defineProperty(navigator, "clipboard", {
      value: Object.freeze({ writeText: copyText, write: writeItems }),
      configurable: true,
    });
  }
