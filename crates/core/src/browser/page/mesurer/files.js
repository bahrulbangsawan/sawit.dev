  // ---- Files (lane 1: mesurer-capture) ----
  //
  // mesurer saves by clicking a download link, which a WKWebView doesn't follow. This world's
  // anchor `click`, for a `download` link to a blob: or data: address, posts the file instead
  // (`file.save`), never WebKit's download; the app saves it under `.nyawit/browser/captures/`.
  // Other links, and the page's own (another prototype), click as before. Blobs are kept from this
  // world's `URL.createObjectURL`, as mesurer revokes one right after the click.
  const BLOBS = new Map();
  if (typeof URL === "function" && typeof URL.createObjectURL === "function") {
    const create = URL.createObjectURL;
    const revoke = URL.revokeObjectURL;
    Object.defineProperty(URL, "createObjectURL", method(function createObjectURL(object) {
      const url = create.call(URL, object);
      if (typeof Blob === "function" && object instanceof Blob) BLOBS.set(url, object);
      return url;
    }));
    Object.defineProperty(URL, "revokeObjectURL", method(function revokeObjectURL(url) {
      BLOBS.delete(String(url));
      return revoke.call(URL, url);
    }));
  }
  // A data: address goes as it is, and Rust reads it (`mesurer::files::data_url`): no fetch a
  // page's CSP could stop.
  const saveDownload = async (name, href) => {
    const kept = BLOBS.get(href);
    if (!kept && /^data:/i.test(href)) return bridge("file.save", { name, href });
    const blob = kept || (await (await fetch(href)).blob());
    return bridge("file.save", { name, mime: blob.type || "", base64: await blobBase64(blob) });
  };
  if (typeof HTMLAnchorElement === "function") {
    const nativeClick = HTMLAnchorElement.prototype.click;
    Object.defineProperty(HTMLAnchorElement.prototype, "click", method(function click() {
      const href = String(this.href || "");
      if (!this.hasAttribute("download") || !/^(blob|data):/i.test(href)) return nativeClick.call(this);
      saveDownload(this.getAttribute("download") || "", href).catch((e) => log("warn", "mesurer's download wasn't saved:", e));
    }));
  }
