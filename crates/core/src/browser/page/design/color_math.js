  // ---- Color math (pure: no DOM; browser.rs tests it in QuickJS) ----

  // A CSS color as WebKit computes it → sRGB [r, g, b, a] (0–255, alpha 0–1),
  // or null. Reads rgb()/rgba(), hex, oklch(), oklab(), lab(), lch() and
  // color(srgb | srgb-linear | display-p3); out-of-gamut values are clamped.
  const mat = (m, v) => m.map((row) => row[0] * v[0] + row[1] * v[1] + row[2] * v[2]);
  const XYZ_TO_SRGB = [
    [3.2409699419045226, -1.537383177570094, -0.4986107602930034],
    [-0.9692436362808796, 1.8759675015077202, 0.04155505740717559],
    [0.05563007969699366, -0.20397695888897652, 1.0569715142428786],
  ];
  const D50_TO_D65 = [
    [0.9554734527042182, -0.023098536874261423, 0.0632593086610217],
    [-0.028369706963208136, 1.0099954580058226, 0.021041398966943008],
    [0.012314001688319899, -0.020507696433477912, 1.3303659366080753],
  ];
  const P3_TO_XYZ = [
    [0.4865709486482162, 0.26566769316909306, 0.1982172852343625],
    [0.2289745640697488, 0.6917385218365064, 0.079286914093745],
    [0, 0.04511338185890264, 1.043944368900976],
  ];
  const toLinear = (v) => (Math.abs(v) <= 0.04045 ? v / 12.92 : Math.sign(v) * Math.pow((Math.abs(v) + 0.055) / 1.055, 2.4));
  const toGamma = (v) => (Math.abs(v) <= 0.0031308 ? v * 12.92 : Math.sign(v) * (1.055 * Math.pow(Math.abs(v), 1 / 2.4) - 0.055));
  const oklabToLinear = (L, a, b) => {
    const l = Math.pow(L + 0.3963377774 * a + 0.2158037573 * b, 3);
    const m = Math.pow(L - 0.1055613458 * a - 0.0638541728 * b, 3);
    const s = Math.pow(L - 0.0894841775 * a - 1.291485548 * b, 3);
    return [
      4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
      -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
      -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    ];
  };
  const labToLinear = (L, a, b) => {
    const k = 24389 / 27;
    const e = 216 / 24389;
    const fy = (L + 16) / 116;
    const fx = fy + a / 500;
    const fz = fy - b / 200;
    const x = fx * fx * fx > e ? fx * fx * fx : (116 * fx - 16) / k;
    const y = L > k * e ? fy * fy * fy : L / k;
    const z = fz * fz * fz > e ? fz * fz * fz : (116 * fz - 16) / k;
    return mat(XYZ_TO_SRGB, mat(D50_TO_D65, [x * 0.96422, y, z * 0.82521]));
  };
  // One component: a number, a percentage of `full`, an angle, or `none` (0).
  const part = (t, full) => {
    if (t === undefined || t === "none") return 0;
    const n = parseFloat(t);
    if (!Number.isFinite(n)) return NaN;
    if (t.endsWith("%")) return (n / 100) * full;
    if (t.endsWith("turn")) return n * 360;
    if (t.endsWith("rad")) return (n * 180) / Math.PI;
    if (t.endsWith("grad")) return n * 0.9;
    return n;
  };
  const readColor = (input) => {
    const c = String(input || "").trim().toLowerCase();
    if (!c) return null;
    if (c === "transparent") return [0, 0, 0, 0];
    const hex = /^#([0-9a-f]{3,8})$/.exec(c);
    if (hex) {
      let h = hex[1];
      if (h.length === 3 || h.length === 4) h = h.replace(/./g, "$&$&");
      if (h.length !== 6 && h.length !== 8) return null;
      const n = (i) => parseInt(h.slice(i, i + 2), 16);
      return [n(0), n(2), n(4), h.length === 8 ? n(6) / 255 : 1];
    }
    const fn = /^([a-z-]+)\(\s*([^()]*)\)$/.exec(c);
    if (!fn) return null;
    const [main, alphaText] = fn[2].split("/");
    const args = main.trim().split(/[\s,]+/).filter(Boolean);
    let alpha = alphaText === undefined ? 1 : part(alphaText.trim(), 1);
    let linear = null;
    let rgb = null;
    if (fn[1] === "rgb" || fn[1] === "rgba") {
      if (args.length < 3) return null;
      if (args.length === 4 && alphaText === undefined) alpha = part(args[3], 1);
      rgb = [part(args[0], 255), part(args[1], 255), part(args[2], 255)];
    } else if (fn[1] === "oklab") {
      linear = oklabToLinear(part(args[0], 1), part(args[1], 0.4), part(args[2], 0.4));
    } else if (fn[1] === "oklch") {
      const C = part(args[1], 0.4);
      const h = (part(args[2], 1) * Math.PI) / 180;
      linear = oklabToLinear(part(args[0], 1), C * Math.cos(h), C * Math.sin(h));
    } else if (fn[1] === "lab") {
      linear = labToLinear(part(args[0], 100), part(args[1], 125), part(args[2], 125));
    } else if (fn[1] === "lch") {
      const C = part(args[1], 150);
      const h = (part(args[2], 1) * Math.PI) / 180;
      linear = labToLinear(part(args[0], 100), C * Math.cos(h), C * Math.sin(h));
    } else if (fn[1] === "color") {
      const v = args.slice(1, 4).map((t) => part(t, 1));
      if (args[0] === "srgb") rgb = v.map((x) => x * 255);
      else if (args[0] === "srgb-linear") linear = v;
      else if (args[0] === "display-p3") linear = mat(XYZ_TO_SRGB, mat(P3_TO_XYZ, v.map(toLinear)));
      else return null;
    } else {
      return null;
    }
    if (linear) rgb = linear.map((x) => toGamma(x) * 255);
    if (!rgb || rgb.some((x) => !Number.isFinite(x)) || !Number.isFinite(alpha)) return null;
    const clamp = (x, hi) => Math.min(Math.max(x, 0), hi);
    return [clamp(rgb[0], 255), clamp(rgb[1], 255), clamp(rgb[2], 255), clamp(alpha, 1)];
  };
  // OKLCH chroma of sRGB [r, g, b] (0–255).
  const chromaOf = (p) => {
    const [r, g, b] = [toLinear(p[0] / 255), toLinear(p[1] / 255), toLinear(p[2] / 255)];
    const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
    const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
    const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
    const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
    return Math.sqrt(A * A + B * B);
  };

  // ---- End of color math ----
