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
