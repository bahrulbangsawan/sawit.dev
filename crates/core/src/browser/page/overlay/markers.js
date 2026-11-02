  // ---- Markers ----

  const markerAction = (node) => {
    for (let i = 0; i < marks.length; i++) if (marks[i].node === node) return marks[i];
    return null;
  };
  // The markers for `items`: kept where the number stays, so a focused one keeps the keyboard.
  const syncMarkers = () => {
    if (!ensureHost()) return;
    stale = false;
    const kept = [];
    for (let i = 0; i < marks.length; i++) {
      let still = null;
      for (let j = 0; j < items.length; j++) if (items[j].n === marks[i].item.n) still = items[j];
      if (still) {
        marks[i].item = still;
        kept[kept.length] = marks[i];
      } else drop(marks[i].node);
    }
    for (let j = 0; j < items.length; j++) {
      let has = false;
      for (let i = 0; i < kept.length; i++) if (kept[i].item.n === items[j].n) has = true;
      if (has) continue;
      const node = make("button");
      attr(node, "type", "button");
      attr(node, "class", "mark");
      write(node, String(items[j].n));
      put(rootNode, node);
      kept[kept.length] = { node, item: items[j] };
    }
    for (let i = 0; i < kept.length; i++) {
      const it = kept[i].item;
      const what = it.comment ? clip(oneLine(it.comment), 80) : it.label || "";
      attr(kept[i].node, "aria-label", "Annotation " + it.n + (what ? ": " + what : "") + ". Enter opens it, Delete removes it.");
      attr(kept[i].node, "title", "Annotation " + it.n + (what ? ": " + what : ""));
    }
    marks = kept;
  };
