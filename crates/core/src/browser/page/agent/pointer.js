  // ---- Pointer ----

  const pointer = (el, type, x, y, init) => {
    const Ctor = type.startsWith("pointer") && window.PointerEvent ? PointerEvent : MouseEvent;
    return el.dispatchEvent(
      new Ctor(type, {
        bubbles: !type.endsWith("enter") && !type.endsWith("leave"),
        cancelable: true,
        composed: true,
        view: window,
        clientX: x,
        clientY: y,
        pointerId: 1,
        pointerType: "mouse",
        isPrimary: true,
        ...init,
      }),
    );
  };
  const hitTest = (x, y) => {
    let hit = document.elementFromPoint(x, y);
    while (hit && hit.shadowRoot) {
      const inner = hit.shadowRoot.elementFromPoint(x, y);
      if (!inner || inner === hit) break;
      hit = inner;
    }
    return hit;
  };
  const composedContains = (outer, inner) => {
    for (let node = inner; node; node = node.parentNode || node.host) if (node === outer) return true;
    return false;
  };
  // Where a real click on `el` would land, after scrolling it into view.
  const aim = (el) => {
    el.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
    const rect = el.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) throw new Error(describe(el) + " has no size on screen.");
    const x = Math.min(Math.max(rect.left + rect.width / 2, 0), innerWidth - 1);
    const y = Math.min(Math.max(rect.top + rect.height / 2, 0), innerHeight - 1);
    const hit = hitTest(x, y);
    const label = hit && hit.closest && hit.closest("label");
    const ok = hit && (composedContains(el, hit) || composedContains(hit, el) || (label && label.control === el));
    if (!ok) throw new Error(describe(el) + " is covered by " + (hit ? describe(hit) : "nothing clickable") + ". Close or scroll past it, or click that element instead.");
    return { x, y, target: composedContains(el, hit) ? hit : el };
  };
  const focusable = (el) => {
    for (let node = el; node && node !== document.body; node = node.parentElement || (node.getRootNode && node.getRootNode().host)) {
      if (node.tabIndex >= 0 || node.isContentEditable) return node;
    }
    return null;
  };

  const click = (op) => {
    const el = required(op);
    if (el.disabled || el.getAttribute("aria-disabled") === "true") throw new Error(describe(el) + " is disabled.");
    const { x, y, target } = aim(el);
    const right = op.button === "right";
    const button = right ? 2 : 0;
    pointer(target, "pointerover", x, y, {});
    pointer(target, "pointerenter", x, y, {});
    pointer(target, "mouseover", x, y, {});
    pointer(target, "mouseenter", x, y, {});
    pointer(target, "pointermove", x, y, {});
    pointer(target, "mousemove", x, y, {});
    const times = op.double ? 2 : 1;
    for (let detail = 1; detail <= times; detail++) {
      pointer(target, "pointerdown", x, y, { button, buttons: right ? 2 : 1, detail });
      const focusIt = pointer(target, "mousedown", x, y, { button, buttons: right ? 2 : 1, detail });
      if (focusIt && detail === 1) {
        const f = focusable(target);
        if (f && f !== document.activeElement) f.focus({ preventScroll: true });
      }
      pointer(target, "pointerup", x, y, { button, detail });
      pointer(target, "mouseup", x, y, { button, detail });
      if (right) {
        pointer(target, "contextmenu", x, y, { button, detail });
        break;
      }
      pointer(target, "click", x, y, { button, detail });
    }
    if (op.double) pointer(target, "dblclick", x, y, { detail: 2 });
    return { clicked: describe(el), url: location.href };
  };

  const hover = (op) => {
    const el = required(op);
    const { x, y, target } = aim(el);
    for (const type of ["pointerover", "pointerenter", "mouseover", "mouseenter", "pointermove", "mousemove"]) pointer(target, type, x, y, {});
    return { hovered: describe(el), note: "Hover handlers ran; CSS :hover styles don't apply to simulated pointers." };
  };
