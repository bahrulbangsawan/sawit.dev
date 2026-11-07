// Convert to Layers (B15): what a screen's page looks like once rendered, read inside its own web
// view and answered as JSON (a string). Facts only: each element's box (CSS px from the page's
// top-left), the computed styles a layer can carry, its text, and its pictures; Rust decides
// which become frames, text, images and auto layout (layers/build.rs).
(() => {
  const MAX = 2500, MAX_IMAGE = 3 * 1024 * 1024;
  const sx = window.scrollX, sy = window.scrollY;
  let count = 0;
  const skipped = { pseudo: 0, backgrounds: 0, images: 0, over: 0 };
  const PHRASING = /^(a|abbr|b|bdi|bdo|br|cite|code|data|dfn|em|i|kbd|mark|q|s|samp|small|span|strong|sub|sup|time|u|var|wbr|label)$/;
  const MEDIA = /^(img|svg|canvas|video|picture)$/;
  const FIELD = /^(input|textarea|select)$/;
  const px = (v) => { const n = parseFloat(v); return Number.isFinite(n) ? n : 0; };
  const clear = (c) => !c || c === 'transparent' || /rgba\(.*,\s*0\)$/.test(c);
  const box = (r) => [r.left + sx, r.top + sy, r.width, r.height];
  const textStyle = (cs) => ({
    c: cs.color, ff: cs.fontFamily, fs: px(cs.fontSize), fw: px(cs.fontWeight) || 400,
    lh: cs.lineHeight === 'normal' ? null : px(cs.lineHeight),
    ls: cs.letterSpacing === 'normal' ? 0 : px(cs.letterSpacing),
    ta: cs.textAlign, tt: cs.textTransform, td: cs.textDecorationLine, it: cs.fontStyle === 'italic',
  });
  const style = (el, cs, r) => {
    const s = textStyle(cs);
    if (!clear(cs.backgroundColor)) s.bg = cs.backgroundColor;
    if (cs.backgroundImage && cs.backgroundImage !== 'none') skipped.backgrounds++;
    const bd = [cs.borderTopWidth, cs.borderRightWidth, cs.borderBottomWidth, cs.borderLeftWidth].map(px);
    const sides = [cs.borderTopStyle, cs.borderRightStyle, cs.borderBottomStyle, cs.borderLeftStyle];
    const colors = [cs.borderTopColor, cs.borderRightColor, cs.borderBottomColor, cs.borderLeftColor];
    const shown = bd.map((w, i) => (sides[i] === 'none' || sides[i] === 'hidden' || clear(colors[i])) ? 0 : w);
    if (shown.some((w) => w > 0)) { s.bd = shown; s.bc = colors[shown.findIndex((w) => w > 0)]; }
    const radius = (v) => { const n = px(v); return /%$/.test(v) ? n / 100 * Math.min(r.width, r.height) : n; };
    const rd = [cs.borderTopLeftRadius, cs.borderTopRightRadius, cs.borderBottomRightRadius, cs.borderBottomLeftRadius].map(radius);
    if (rd.some((v) => v > 0)) s.rd = rd;
    if (+cs.opacity < 1) s.op = +cs.opacity;
    if (cs.boxShadow && cs.boxShadow !== 'none') s.sh = cs.boxShadow;
    s.d = cs.display; s.pos = cs.position;
    if (/flex/.test(cs.display)) {
      s.fd = cs.flexDirection; s.wr = cs.flexWrap !== 'nowrap'; s.jc = cs.justifyContent; s.ai = cs.alignItems;
    }
    if (/flex|grid/.test(cs.display)) s.g = [cs.rowGap, cs.columnGap].map((v) => v === 'normal' ? 0 : px(v));
    s.p = [cs.paddingTop, cs.paddingRight, cs.paddingBottom, cs.paddingLeft].map(px);
    s.bw = bd;
    const parent = el.parentElement && getComputedStyle(el.parentElement);
    if (parent && /flex/.test(parent.display)) { s.grow = px(cs.flexGrow); s.self = cs.alignSelf; }
    if (cs.overflow !== 'visible' || cs.overflowX !== 'visible') s.clip = true;
    return s;
  };
  // The text a node shows: its text nodes and <br>s, white space collapsed as CSS does.
  const textOf = (node, pre) => {
    let out = '';
    for (const c of node.childNodes) {
      if (c.nodeType === 3) out += pre ? c.textContent : c.textContent.replace(/[ \t\r\n\f]+/g, ' ');
      else if (c.nodeType === 1 && c.tagName === 'BR') out += '\n';
      else if (c.nodeType === 1 && getComputedStyle(c).display !== 'none') out += textOf(c, pre);
    }
    return out;
  };
  const tidy = (t) => t.split('\n').map((l) => l.replace(/ +/g, ' ').trim()).join('\n').trim();
  const rangeBox = (nodes) => {
    const range = document.createRange();
    let l = Infinity, t = Infinity, r = -Infinity, b = -Infinity;
    for (const n of nodes) {
      range.selectNodeContents(n);
      for (const q of range.getClientRects()) {
        if (q.width < 0.5 || q.height < 0.5) continue;
        l = Math.min(l, q.left); t = Math.min(t, q.top); r = Math.max(r, q.right); b = Math.max(b, q.bottom);
      }
    }
    return l === Infinity ? null : [l + sx, t + sy, r - l, b - t];
  };
  const phrasingOnly = (el) => [...el.childNodes].every((c) => c.nodeType === 3 || c.nodeType === 8 ||
    (c.nodeType === 1 && PHRASING.test(c.tagName.toLowerCase()) && !/flex|grid|block/.test(getComputedStyle(c).display) &&
      clear(getComputedStyle(c).backgroundColor) && px(getComputedStyle(c).borderTopWidth) === 0 && phrasingOnly(c)));
  const nameOf = (el) => {
    const label = el.getAttribute('aria-label') || el.getAttribute('alt') || el.id || '';
    if (label) return label.slice(0, 60);
    const cls = (el.getAttribute('class') || '').trim().split(/\s+/)[0];
    return cls ? cls.slice(0, 60) : null;
  };
  const picture = (el) => {
    try {
      const w = el.naturalWidth || el.videoWidth || el.width, h = el.naturalHeight || el.videoHeight || el.height;
      if (!w || !h) return null;
      const c = document.createElement('canvas');
      c.width = Math.min(w, 2048); c.height = Math.round(h * c.width / w);
      c.getContext('2d').drawImage(el, 0, 0, c.width, c.height);
      const url = c.toDataURL('image/png');
      return url.length > MAX_IMAGE ? null : url;
    } catch (_) { return null; }
  };
  const svgOf = (el, cs, r) => {
    const copy = el.cloneNode(true);
    copy.setAttribute('width', String(r.width)); copy.setAttribute('height', String(r.height));
    if (!copy.getAttribute('xmlns')) copy.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
    if (!copy.getAttribute('viewBox') && el.viewBox && el.viewBox.baseVal && el.viewBox.baseVal.width)
      copy.setAttribute('viewBox', `0 0 ${el.viewBox.baseVal.width} ${el.viewBox.baseVal.height}`);
    // currentColor and inherited paint resolve to what the page drew.
    const kids = [copy, ...copy.querySelectorAll('*')], real = [el, ...el.querySelectorAll('*')];
    // Each shape gets the paint the page's CSS gave it, which the copy alone would lose.
    kids.forEach((k, i) => {
      const s = real[i] && getComputedStyle(real[i]);
      if (!s || !(i === 0 || /^(path|circle|rect|line|polyline|polygon|ellipse|g|text)$/.test(k.tagName))) return;
      for (const a of ['fill', 'stroke']) {
        const v = s[a];
        k.setAttribute(a, !v || /currentcolor/i.test(v) ? s.color : v);
      }
      if (s.strokeWidth) k.setAttribute('stroke-width', s.strokeWidth);
      for (const a of ['strokeLinecap', 'strokeLinejoin'])
        if (s[a]) k.setAttribute(a === 'strokeLinecap' ? 'stroke-linecap' : 'stroke-linejoin', s[a]);
    });
    return new XMLSerializer().serializeToString(copy).replace(/currentColor/g, cs.color);
  };
  const walk = (el) => {
    if (count >= MAX) { skipped.over++; return null; }
    const cs = getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden' || +cs.opacity === 0) return null;
    const tag = el.tagName.toLowerCase();
    if (/^(script|style|template|noscript|head|meta|link)$/.test(tag)) return null;
    const r = el.getBoundingClientRect();
    for (const p of ['::before', '::after']) {
      const c = getComputedStyle(el, p).content;
      if (c && c !== 'none' && c !== 'normal' && c !== '""') skipped.pseudo++;
    }
    count++;
    const item = { t: tag, r: box(r), s: style(el, cs, r), k: [] };
    const name = nameOf(el);
    if (name) item.nm = name;
    if (tag === 'svg') { item.svg = svgOf(el, cs, r); return item; }
    if (MEDIA.test(tag)) {
      const target = tag === 'picture' ? el.querySelector('img') : el;
      item.img = target ? picture(target) : null;
      if (!item.img) skipped.images++;
      return item;
    }
    if (FIELD.test(tag)) {
      const value = tag === 'select' ? (el.selectedOptions[0] || {}).text : el.value;
      const shown = value || el.getAttribute('placeholder') || '';
      if (shown) {
        const ts = textStyle(cs);
        if (!value) ts.c = getComputedStyle(el, '::placeholder').color || ts.c;
        const p = item.s.p, b = item.s.bw;
        item.k.push({ t: '#text', tx: shown, s: ts, r: [item.r[0] + p[3] + b[3], item.r[1] + p[0] + b[0],
          Math.max(1, item.r[2] - p[1] - p[3] - b[1] - b[3]), Math.max(1, item.r[3] - p[0] - p[2] - b[0] - b[2])] });
      }
      return item;
    }
    const pre = /pre/.test(cs.whiteSpace);
    if (el.childNodes.length && phrasingOnly(el)) {
      const text = tidy(textOf(el, pre));
      if (text) {
        const at = rangeBox([el]) || item.r;
        item.k.push({ t: '#text', tx: text, s: textStyle(cs), r: at });
      }
      return item;
    }
    for (const c of el.childNodes) {
      if (c.nodeType === 3) {
        const text = tidy(pre ? c.textContent : c.textContent.replace(/\s+/g, ' '));
        const at = text && rangeBox([c]);
        if (at) item.k.push({ t: '#text', tx: text, s: textStyle(cs), r: at });
      } else if (c.nodeType === 1) {
        const kid = walk(c);
        if (kid) item.k.push(kid);
      }
    }
    return item;
  };
  const root = document.body ? walk(document.body) : null;
  const doc = getComputedStyle(document.documentElement);
  const body = document.body ? getComputedStyle(document.body) : doc;
  const bg = !clear(body.backgroundColor) ? body.backgroundColor : !clear(doc.backgroundColor) ? doc.backgroundColor : 'rgb(255, 255, 255)';
  const h = Math.max(innerHeight, document.documentElement.scrollHeight, document.body ? document.body.scrollHeight : 0);
  return JSON.stringify({ w: innerWidth, h, bg, title: document.title || '', root, skipped, count });
})()
