  const HOST_TAG = "sawit-overlay";

  // The listener methods as they are before any page script runs (this module runs with the
  // agent, at document start), called through the agent's own `apply`, with options that
  // have no prototype a page could give a `signal` or `once`. The picker's listeners go only
  // through these, and only by index: a page script that got one (by patching the methods,
  // the array iterator that `for…of` and destructuring use, or `Array.prototype`) could call
  // it with an `isTrusted: true` object of its own and pick for the user. Nor can a page
  // take one from the `caller` of a function a listener calls (a getter on the event, a
  // replaced `preventDefault`): the agent is strict-mode code, so `caller` is null there.
  const addListener = EventTarget.prototype.addEventListener;
  const removeListener = EventTarget.prototype.removeEventListener;
  const listen = (type, fn, options) => apply(addListener, window, [type, fn, options]);
  const unlisten = (type, fn, options) => apply(removeListener, window, [type, fn, options]);
  const CAPTURE = { __proto__: null, capture: true };
  const SCROLLS = { __proto__: null, capture: true, passive: true };
  const PASSIVE = { __proto__: null, passive: true };
  const ONCE = { __proto__: null, once: true };

  // ---- Natives, taken at document start ----
  //
  // The popover holds what the user types for the agent, which reaches it as the user's own
  // words. So no page script may get hold of a node of the overlay (it could read or rewrite
  // the comment), nor read a key typed into it. Every DOM call on the overlay's nodes goes
  // through these, never through a method or getter a page could have replaced by the time
  // the overlay is drawn; arrays that hold its nodes are read and written by index. Where a
  // native is missing (a test's stand-in page), the plain property is used.
  const protoOf = (name) => {
    const C = window[name];
    return typeof C === "function" && C.prototype ? C.prototype : null;
  };
  const method = (p, name) => (p && typeof p[name] === "function" ? p[name] : null);
  const getter = (p, name) => {
    const d = p ? Object.getOwnPropertyDescriptor(p, name) : null;
    return d && d.get ? d.get : null;
  };
  const setter = (p, name) => {
    const d = p ? Object.getOwnPropertyDescriptor(p, name) : null;
    return d && d.set ? d.set : null;
  };
  const EV = protoOf("Event");
  const KEV = protoOf("KeyboardEvent");
  const MEV = protoOf("MouseEvent");
  const UIEV = protoOf("UIEvent");
  const NODE = protoOf("Node");
  const ELEM = protoOf("Element");
  const HTML = protoOf("HTMLElement");
  const DOC = protoOf("Document");
  const FRAG = protoOf("DocumentFragment");
  const SHADOW = protoOf("ShadowRoot");
  const AREA = protoOf("HTMLTextAreaElement");
  const CSSD = protoOf("CSSStyleDeclaration");
  const SHEET = window.CSSStyleSheet;
  const N = {
    target: getter(EV, "target"),
    type: getter(EV, "type"),
    stop: method(EV, "stopImmediatePropagation"),
    prevent: method(EV, "preventDefault"),
    key: getter(KEV, "key"),
    composing: getter(KEV, "isComposing"),
    meta: getter(KEV, "metaKey"),
    shift: getter(KEV, "shiftKey"),
    ctrl: getter(KEV, "ctrlKey"),
    keyAlt: getter(KEV, "altKey"),
    mouseAlt: getter(MEV, "altKey"),
    x: getter(MEV, "clientX"),
    y: getter(MEV, "clientY"),
    detail: getter(UIEV, "detail"),
    createElement: method(DOC, "createElement"),
    attachShadow: method(ELEM, "attachShadow"),
    setAttribute: method(ELEM, "setAttribute"),
    removeAttribute: method(ELEM, "removeAttribute"),
    box: method(ELEM, "getBoundingClientRect"),
    scrollHeight: getter(ELEM, "scrollHeight"),
    remove: method(ELEM, "remove"),
    appendChild: method(NODE, "appendChild"),
    text: setter(NODE, "textContent"),
    connected: getter(NODE, "isConnected"),
    style: getter(HTML, "style"),
    focus: method(HTML, "focus"),
    blur: method(HTML, "blur"),
    query: method(FRAG, "querySelector"),
    inner: setter(SHADOW, "innerHTML"),
    active: getter(SHADOW, "activeElement"),
    sheets: setter(SHADOW, "adoptedStyleSheets"),
    value: getter(AREA, "value"),
    setValue: setter(AREA, "value"),
    cssText: setter(CSSD, "cssText"),
    setProperty: method(CSSD, "setProperty"),
    replaceSync: SHEET && method(SHEET.prototype, "replaceSync"),
  };
  // An event's field, through its native getter.
  const read = (get, e, name) => (get ? apply(get, e, []) : e[name]);
  const stopNow = (e) => (N.stop ? apply(N.stop, e, []) : e.stopImmediatePropagation());
  const preventNow = (e) => (N.prevent ? apply(N.prevent, e, []) : e.preventDefault());
  // DOM calls on the overlay's own nodes.
  const make = (tag) => apply(N.createElement, document, [tag]);
  const put = (parent, child) => apply(N.appendChild, parent, [child]);
  const drop = (node) => apply(N.remove, node, []);
  const attr = (node, name, value) =>
    value === null ? apply(N.removeAttribute, node, [name]) : apply(N.setAttribute, node, [name, value]);
  const write = (node, text) => apply(N.text, node, [text]);
  const css = (node, text) => apply(N.cssText, apply(N.style, node, []), [text]);
  const prop = (node, name, value) => apply(N.setProperty, apply(N.style, node, []), [name, value]);
  const boxOf = (node) => apply(N.box, node, []);
  const find = (root, selector) => apply(N.query, root, [selector]);
  const focusOn = (node) => apply(N.focus, node, [{ __proto__: null, preventScroll: true }]);
