  // ---- Drawing ----

  const drawHighlight = (el, label) => {
    if (!el || !el.isConnected) return;
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    const n = (name) => parseFloat(s[name]) || 0;
    const m = { t: n("marginTop"), r: n("marginRight"), b: n("marginBottom"), l: n("marginLeft") };
    const bw = { t: n("borderTopWidth"), r: n("borderRightWidth"), b: n("borderBottomWidth"), l: n("borderLeftWidth") };
    const p = { t: n("paddingTop"), r: n("paddingRight"), b: n("paddingBottom"), l: n("paddingLeft") };
    const at = (x, y, w, h) => "left:" + px(x) + ";top:" + px(y) + ";width:" + px(Math.max(w, 0)) + ";height:" + px(Math.max(h, 0)) + ";";
    box(at(r.left - m.l, r.top - m.t, r.width + m.l + m.r, r.height + m.t + m.b) + "background:rgba(246,178,107,.3)");
    const px0 = r.left + bw.l;
    const py0 = r.top + bw.t;
    const pw = r.width - bw.l - bw.r;
    const ph = r.height - bw.t - bw.b;
    box(at(px0, py0, pw, ph) + "background:rgba(147,196,125,.3)");
    box(at(r.left, r.top, r.width, r.height) + "outline:2px solid var(--primary);outline-offset:-1px");
    if (!label) return;
    const chip = make("div");
    attr(chip, "class", "label");
    const name = make("span");
    write(name, short(el));
    const size = make("span");
    attr(size, "class", "size");
    write(size, sizeOf(el));
    put(chip, name);
    put(chip, size);
    css(chip, "left:" + px(Math.max(Math.min(r.left, innerWidth - 120), 4)) + ";top:" + px(r.top < 28 ? Math.min(r.bottom + 6, innerHeight - 26) : r.top - 26));
    put(layer, chip);
    drawn[drawn.length] = chip;
  };
  // Each marker sits on its element's top-left corner, kept inside the viewport.
  const placeMarkers = () => {
    for (let i = 0; i < marks.length; i++) {
      const m = marks[i];
      let el = null;
      try {
        el = resolve({ selector: m.item.selector });
      } catch (_) {}
      if (!el) {
        prop(m.node, "display", "none");
        continue;
      }
      const r = el.getBoundingClientRect();
      const out = r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth;
      prop(m.node, "display", out ? "none" : "block");
      prop(m.node, "left", px(Math.max(2, Math.min(r.left - 12, innerWidth - 28))));
      prop(m.node, "top", px(Math.max(2, Math.min(r.top - 12, innerHeight - 28))));
      attr(m.node, "class", pop && pop.n === m.item.n ? "mark open" : "mark");
    }
  };
  const placePopover = () => {
    if (!pop || !ui) return;
    if (!pop.el.isConnected) return close();
    const r = pop.el.getBoundingClientRect();
    const b = boxOf(ui.pop);
    const at = place(r, { width: b.width || 320, height: b.height || 200 }, { width: innerWidth, height: innerHeight });
    prop(ui.pop, "left", at.left + "px");
    prop(ui.pop, "top", at.top + "px");
    prop(ui.pop, "--rise", at.side === "above" ? "-4px" : "4px");
  };
  const drawMeasure = () => {
    if (mode !== "inspect" || !altHeld || !pinned || !pinned.isConnected || !current || current === pinned) return;
    const a = pinned.getBoundingClientRect();
    const b = current.getBoundingClientRect();
    box("left:" + px(a.left) + ";top:" + px(a.top) + ";width:" + px(a.width) + ";height:" + px(a.height) +
      ";outline:1px dashed " + MEASURE + ";outline-offset:-1px");
    const tag = (x, y, d) => {
      box("left:" + px(x) + ";top:" + px(y) + ";transform:translate(-50%,-50%);padding:1px 4px;border-radius:3px;background:" +
        MEASURE + ";color:#fff;font:10px/13px system-ui,sans-serif;white-space:nowrap", Math.round(d * 10) / 10 + "px");
    };
    const hLine = (x1, x2, y) => {
      if (x2 - x1 < 0.5) return;
      box("left:" + px(x1) + ";top:" + px(y) + ";width:" + px(x2 - x1) + ";height:1px;background:" + MEASURE);
      tag((x1 + x2) / 2, y, x2 - x1);
    };
    const vLine = (y1, y2, x) => {
      if (y2 - y1 < 0.5) return;
      box("left:" + px(x) + ";top:" + px(y1) + ";width:1px;height:" + px(y2 - y1) + ";background:" + MEASURE);
      tag(x, (y1 + y2) / 2, y2 - y1);
    };
    // Where a line between the boxes runs: the middle of their overlap, else the picked box's middle.
    const midY = Math.max(a.top, b.top) < Math.min(a.bottom, b.bottom) ? (Math.max(a.top, b.top) + Math.min(a.bottom, b.bottom)) / 2 : a.top + a.height / 2;
    const midX = Math.max(a.left, b.left) < Math.min(a.right, b.right) ? (Math.max(a.left, b.left) + Math.min(a.right, b.right)) / 2 : a.left + a.width / 2;
    // Horizontal: the gap between them, else (overlapping) the distance between matching edges.
    if (b.left >= a.right) hLine(a.right, b.left, midY);
    else if (a.left >= b.right) hLine(b.right, a.left, midY);
    else {
      hLine(Math.min(a.left, b.left), Math.max(a.left, b.left), midY);
      hLine(Math.min(a.right, b.right), Math.max(a.right, b.right), midY);
    }
    if (b.top >= a.bottom) vLine(a.bottom, b.top, midX);
    else if (a.top >= b.bottom) vLine(b.bottom, a.top, midX);
    else {
      vLine(Math.min(a.top, b.top), Math.max(a.top, b.top), midX);
      vLine(Math.min(a.bottom, b.bottom), Math.max(a.bottom, b.bottom), midX);
    }
  };
  const render = () => {
    if (!host && items.length === 0 && mode === "off" && !pop) return;
    if (!ensureHost()) return;
    for (let i = 0; i < drawn.length; i++) drop(drawn[i]);
    drawn = [];
    if (stale) syncMarkers();
    placeMarkers();
    if (pop) drawHighlight(pop.el, false);
    else if (mode !== "off") drawHighlight(current, true);
    drawMeasure();
    placePopover();
  };
  const schedule = () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      render();
    });
  };
  let timer = 0;
  const watch = () => {
    const want = items.length > 0 || mode !== "off" || !!pop;
    if (want === watching) return;
    watching = want;
    const act = want ? listen : unlisten;
    act("scroll", schedule, SCROLLS);
    act("resize", schedule, PASSIVE);
    if (want) timer = setInterval(schedule, 500);
    else clearInterval(timer);
  };

  if (document.readyState === "complete") {
    setTimeout(() => {
      ready = true;
      render();
    }, 1000);
  } else {
    listen("load", () => {
      setTimeout(() => {
        ready = true;
        render();
      }, 1000);
    }, ONCE);
  }
