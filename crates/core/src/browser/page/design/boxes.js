  const buttonLike = (el) => {
    const tag = el.localName;
    if (tag === "button") return true;
    if (el.getAttribute("role") === "button") return true;
    if (tag === "input" && (el.type === "submit" || el.type === "button")) return true;
    return /\bbtn\b|button/i.test(classOf(el));
  };
  // The control text or a color sits in: the nearest link or button around it.
  const controlOf = (el) => (el.closest && el.closest("a, button, [role=button], input")) || el;

  // The element's corner radius: top-left, or the largest corner when they differ.
  const radiusOf = (s) => {
    const corners = [s.borderTopLeftRadius, s.borderTopRightRadius, s.borderBottomRightRadius, s.borderBottomLeftRadius];
    if (corners.every((c) => c === corners[0])) return corners[0];
    let best = corners[0];
    for (const c of corners) if (num(c) > num(best)) best = c;
    return best;
  };
  const zeroRadius = (r) => !r || /^0(px)?( 0(px)?)?$/.test(r);
  const sides = (s, prefix, suffix) => ["Top", "Right", "Bottom", "Left"].map((side) => num(s[prefix + side + suffix]));
  const shorthand = (v) => {
    const p = v.map((n) => round3(n) + "px");
    if (p[0] === p[1] && p[1] === p[2] && p[2] === p[3]) return p[0];
    if (p[0] === p[2] && p[1] === p[3]) return p[0] + " " + p[1];
    if (p[1] === p[3]) return p[0] + " " + p[1] + " " + p[2];
    return p.join(" ");
  };
  const hasBorder = (s) => sides(s, "border", "Width").some((w, i) => w > 0 && s["border" + ["Top", "Right", "Bottom", "Left"][i] + "Style"] !== "none");
