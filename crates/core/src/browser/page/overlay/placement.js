  // ---- Placement ----

  // Where a floating box of `size` goes beside `anchor` (both in viewport px) inside a
  // viewport of `view`: below it, else above it when that fits and below doesn't (flip), at
  // the anchor's left edge moved inside the viewport (shift). An anchor with room on neither
  // side (one taller than the viewport) gets the box inside its visible part.
  const place = (anchor, size, view, gap = 8, margin = 8) => {
    const clamp = (v, lo, hi) => Math.max(lo, Math.min(v, Math.max(lo, hi)));
    const below = view.height - anchor.bottom - gap - margin;
    const above = anchor.top - gap - margin;
    let side;
    let top;
    if (size.height <= below) {
      side = "below";
      top = anchor.bottom + gap;
    } else if (size.height <= above) {
      side = "above";
      top = anchor.top - gap - size.height;
    } else {
      side = "inside";
      top = Math.max(anchor.top, 0) + margin;
    }
    top = clamp(top, margin, view.height - size.height - margin);
    const left = clamp(anchor.left, margin, view.width - size.width - margin);
    return { left: Math.round(left), top: Math.round(top), side };
  };
  helpers.place = place;
  // The overlay's own scale (`--k`): 1 over the scale the device preview draws the page at, so
  // the popover, labels, markers and agents' pointers keep their size on screen at 44 % as at
  // 100 %. 1 when the page is drawn at its own size.
  let uiK = 1;
  const uiScale = (scale) => {
    const s = Number(scale);
    return s > 0 && s < 1 ? Math.min(1 / s, 8) : 1;
  };
  // The popover's width before `uiK` scales it: 320 px on screen, or the viewport's on-screen
  // width less 16 when that is narrower.
  const popWidth = (viewWidth, k) => Math.round(Math.max(160, Math.min(320, viewWidth / k - 16)));
  const overlaps = (a, b) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
  helpers.uiScale = uiScale;
  helpers.popWidth = popWidth;
  helpers.overlaps = overlaps;
