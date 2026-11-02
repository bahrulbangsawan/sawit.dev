  // ---- Network ----

  const TEXTUAL = /json|text|xml|javascript|graphql|x-www-form-urlencoded/i;
  // What is typed into the page's password fields, as a request carries it: as typed, in a form's
  // encoding, URL-encoded and inside a JSON string, longest first. A request is logged with
  // "[redacted]" there, whatever the field's name; Sawit then hides the values of fields named
  // like secrets (password, token, api_key, …). Values shorter than 4 characters are left: they
  // would hide the same letters everywhere else.
  const typedPasswords = () => {
    const found = [];
    try {
      for (const field of document.querySelectorAll('input[type="password"]')) {
        const value = field.value;
        if (typeof value !== "string" || value.length < 4) continue;
        const forms = [value, encodeURIComponent(value), JSON.stringify(value).slice(1, -1)];
        if (typeof URLSearchParams === "function") forms.push(new URLSearchParams([["", value]]).toString().slice(1));
        for (const form of forms) if (!found.includes(form)) found.push(form);
      }
    } catch (_) {}
    return found.sort((a, b) => b.length - a.length);
  };
  const hideTyped = (text) => {
    for (const value of typedPasswords()) text = text.split(value).join("[redacted]");
    return text;
  };
  const bodyText = (body) => {
    if (body == null) return undefined;
    if (typeof body === "string") return clip(hideTyped(body), 2048);
    if (body instanceof URLSearchParams) return clip(hideTyped(body.toString()), 2048);
    if (body instanceof FormData) {
      const parts = [];
      for (const [key, value] of body.entries()) parts.push(key + "=" + (typeof value === "string" ? value : "[file " + value.name + "]"));
      return clip(hideTyped(parts.join("&")), 2048);
    }
    return "[" + (body.constructor ? body.constructor.name : "binary") + "]";
  };
  const originalFetch = window.fetch;
  if (originalFetch) {
    window.fetch = function (input, init) {
      const start = now();
      const request = input instanceof Request ? input : null;
      const method = String((init && init.method) || (request && request.method) || "GET").toUpperCase();
      const url = hideTyped(absolute(request ? request.url : String(input)));
      const entry = { method, url, kind: "fetch", request_body: bodyText(init && init.body), time_ms: start };
      const promise = originalFetch.apply(this, arguments);
      promise.then(
        (response) => {
          entry.status = response.status;
          const done = (body) => {
            if (body !== undefined) entry.response_body = clip(body, 4096);
            entry.duration_ms = now() - start;
            send("network", entry);
          };
          const type = response.headers.get("content-type") || "";
          const size = Number(response.headers.get("content-length") || 0);
          if (!TEXTUAL.test(type) || size > 1 << 20 || /event-stream/.test(type)) return done();
          // A body that never ends (a stream) is reported without it.
          const timer = setTimeout(() => done(), 3000);
          response
            .clone()
            .text()
            .then(
              (body) => {
                clearTimeout(timer);
                done(body);
              },
              () => {
                clearTimeout(timer);
                done();
              },
            );
        },
        (error) => {
          entry.error = String((error && error.message) || error);
          entry.duration_ms = now() - start;
          send("network", entry);
        },
      );
      return promise;
    };
  }
  const xhrs = new WeakMap();
  const xhrOpen = XMLHttpRequest.prototype.open;
  const xhrSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (method, url) {
    // A call on something that isn't a request gets the browser's own error: in strict mode
    // `this` isn't made an object, and a WeakMap would throw first.
    try {
      xhrs.set(this, { method: String(method).toUpperCase(), url: hideTyped(absolute(String(url))) });
    } catch (_) {}
    return xhrOpen.apply(this, arguments);
  };
  XMLHttpRequest.prototype.send = function (body) {
    const info = xhrs.get(this);
    if (info) {
      const start = now();
      const request_body = bodyText(body);
      this.addEventListener("loadend", () => {
        const entry = { method: info.method, url: info.url, kind: "xhr", request_body, time_ms: start, duration_ms: now() - start };
        if (this.status) {
          entry.status = this.status;
          if (this.responseType === "" || this.responseType === "text") entry.response_body = clip(this.responseText || "", 4096);
        } else {
          entry.error = "Request failed";
        }
        send("network", entry);
      });
    }
    return xhrSend.apply(this, arguments);
  };
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        if (e.initiatorType === "fetch" || e.initiatorType === "xmlhttprequest" || failedResources.has(e.name)) continue;
        send("network", { method: "GET", url: e.name, kind: e.initiatorType || "other", status: e.responseStatus || undefined, duration_ms: e.duration, time_ms: e.startTime });
      }
    }).observe({ type: "resource", buffered: true });
  } catch (_) {}
