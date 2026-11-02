  // ---- Agents: the cursors of the agents that drive the page ----
  //
  // Each agent Sawit drives the page for has a pointer in its colour and a pill with its name and
  // what it does, in the overlay's own layer. They take no event (`pointer-events: none`, so
  // `elementFromPoint` passes them by and a click lands where it would), change no layout (the
  // host is fixed over the viewport), are hidden from assistive technology (the app announces
  // what agents do), and hide with the rest of the overlay for a screenshot. Sawit sends the
  // cursor's op just before the action's, so the pointer glides to the target as the action
  // happens; it adds no more than the glide to the action. Only Sawit's call, signed with the
  // token, draws one: a page can't draw a cursor that claims to be an agent's.

  const AGENT_FADE = 5000;
  const AGENT_GONE = 30000;
  // How long the target's ring and a click's ripple stay after the pointer arrives.
  const AGENT_SETTLE = 900;
  const AGENT_COLOR = /^#[0-9a-f]{6}$/i;
  // `[{ id, node, name, doing, typing, x, y, fade, gone }]`, read by index.
  let agentCursors = [];
  // What an agent's op does, in words, by the action's name: `doing|done`.
  const AGENT_VERBS = {
    click: "clicking|clicked",
    hover: "pointing at|pointed at",
    type: "typing in|typed in",
    select: "choosing|chose",
    scroll: "scrolling|scrolled",
    press: "pressing|pressed",
  };
  // The element as the agent's caption names it: its accessible name, else its role or tag.
  const agentTargetName = (el) => {
    if (!el) return "";
    let name = "";
    let r = null;
    try {
      r = role(el);
      name = oneLine(nameOf(el, r) || "");
    } catch (_) {}
    return clip(name || r || el.localName || "", 40);
  };
  // What the agent does with `op` on `el`, in words: "clicking Sign up", "typing in Email",
  // "choosing Large in Size", "pressing Enter", "scrolling".
  const agentAction = (action, el, op) => {
    const verb = (AGENT_VERBS[action] || "working on|worked on").split("|");
    const name = agentTargetName(el);
    let object = name;
    if (action === "press") object = clip(String(op.key || "a key"), 24);
    else if (action === "select") {
      const values = clip((op.values || []).map(String).join(", "), 30);
      object = values && name ? values + " in " + name : values || name;
    } else if (action === "scroll") object = el ? name : "";
    const words = (v) => (object ? v + " " + object : v.replace(/ (in|at|on)$/, ""));
    return { doing: words(verb[0]), done: words(verb[1]) };
  };
  helpers.agentAction = agentAction;
  // Where the pointer goes for `el`: its centre in the viewport, kept inside the viewport.
  const agentAim = (el) => {
    const r = el.getBoundingClientRect();
    const x = Math.min(Math.max(r.left + r.width / 2, 0), innerWidth - 1);
    const y = Math.min(Math.max(r.top + r.height / 2, 0), innerHeight - 1);
    return { x: Math.round(x), y: Math.round(y), r };
  };
  // Where the agent's caret goes in a text field before its text lands: after the text the field
  // holds, in its content box, as tall as a line.
  const agentCaret = (el, r) => {
    let left = r.left + 4;
    let top = r.top + 4;
    let height = Math.max(r.height - 8, 10);
    try {
      const s = getComputedStyle(el);
      const n = (v) => parseFloat(v) || 0;
      const size = n(s.fontSize) || 13;
      const padL = n(s.paddingLeft) + n(s.borderLeftWidth);
      const text = el.isContentEditable ? el.textContent || "" : String(el.value || "");
      let width = text.length * size * 0.55;
      try {
        const ctx = make("canvas").getContext("2d");
        ctx.font = s.font || size + "px sans-serif";
        width = ctx.measureText(text).width;
      } catch (_) {}
      height = Math.min(Math.max(size * 1.3, 10), r.height - 4);
      left = r.left + padL + Math.min(width, Math.max(r.width - padL * 2, 0));
      top = r.top + (r.height - height) / 2;
    } catch (_) {}
    return { left, top, height };
  };
  // After `css`, which sets the whole inline style, the colour of what an agent draws.
  const agentPaint = (node, color) => prop(node, "--c", AGENT_COLOR.test(color || "") ? color : "#2563eb");
  // Drops a node of the agents' layer after `ms`.
  const dropLater = (node, ms) => setTimeout(() => drop(node), ms);
  const agentOf = (id) => {
    for (let i = 0; i < agentCursors.length; i++) if (agentCursors[i].id === id) return agentCursors[i];
    return null;
  };
  const forgetAgent = (a) => {
    clearTimeout(a.fade);
    clearTimeout(a.gone);
    drop(a.node);
    const kept = [];
    for (let i = 0; i < agentCursors.length; i++) if (agentCursors[i] !== a) kept[kept.length] = agentCursors[i];
    agentCursors = kept;
  };
  // The cursor of agent `who` (`{id, name, color}`), made the first time it acts.
  const agentCursor = (who) => {
    let a = agentOf(who.id);
    if (a) return a;
    const node = make("div");
    attr(node, "class", "agent");
    const tip = make("div");
    attr(tip, "class", "tip");
    const arrow = make("div");
    attr(arrow, "class", "arrow");
    put(tip, arrow);
    const pill = make("div");
    attr(pill, "class", "pill");
    const name = make("span");
    attr(name, "class", "name");
    const typing = make("span");
    attr(typing, "class", "typing");
    const doing = make("span");
    attr(doing, "class", "doing");
    put(pill, name);
    put(pill, typing);
    put(pill, doing);
    put(node, tip);
    put(node, pill);
    put(agentsLayer, node);
    a = { id: who.id, node, name, doing, typing, x: null, y: null, fade: 0, gone: 0 };
    agentCursors[agentCursors.length] = a;
    return a;
  };
  // An agent acts: its pointer glides to the target (`lead` ms, Sawit's head start for the
  // action; jumps under reduced motion or the first time), the target is ringed in its colour,
  // a click ripples where the pointer lands, and a field it types into shows its caret first.
  // The cursor fades after a few seconds idle and goes after 30 s.
  const presence = (op) => {
    if (op.token !== TOKEN) return { shown: false };
    if (op.action === "clear") {
      while (agentCursors.length) forgetAgent(agentCursors[0]);
      return { shown: false };
    }
    const who = op.agent || {};
    if (typeof who.id !== "string" || !who.id) throw new Error("An agent's cursor needs the agent's id.");
    let el = null;
    try {
      el = op.action === "press" ? document.activeElement : resolve(op);
    } catch (_) {}
    if (el && (el === document.body || el === document.documentElement)) el = null;
    // The action scrolls its element into view first; the cursor goes where it will be.
    if (el && el.scrollIntoView && (op.action === "click" || op.action === "hover")) {
      el.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
    } else if (el && el.scrollIntoView && (op.action === "type" || (op.action === "scroll" && !op.dx && !op.dy))) {
      el.scrollIntoView({ block: "center", behavior: "instant" });
    }
    const words = agentAction(op.action, el, op);
    if (!ensureHost(true) || !agentsLayer) return { shown: false, doing: words.doing, done: words.done };
    const a = agentCursor(who);
    const lead = Math.max(0, Math.min(Number(op.lead) || 0, 150));
    const aim = el ? agentAim(el) : { x: a.x === null ? Math.round(innerWidth / 2) : a.x, y: a.y === null ? Math.round(innerHeight / 2) : a.y, r: null };
    const first = a.x === null;
    agentPaint(a.node, who.color);
    prop(a.node, "--glide", (first ? 0 : lead) + "ms");
    prop(a.node, "transform", "translate(" + aim.x + "px," + aim.y + "px)");
    const side = (aim.x > innerWidth - 220 ? " west" : "") + (aim.y > innerHeight - 48 ? " north" : "");
    attr(a.node, "class", "agent" + side);
    write(a.name, String(who.name || "Agent").slice(0, 40));
    write(a.doing, words.doing);
    write(a.typing, op.action === "type" ? "▍" : "");
    a.x = aim.x;
    a.y = aim.y;
    const settle = (first ? 0 : lead) + AGENT_SETTLE;
    if (aim.r) {
      const ring = make("div");
      attr(ring, "class", "ring");
      css(ring, "left:" + px(aim.r.left) + ";top:" + px(aim.r.top) + ";width:" + px(aim.r.width) + ";height:" + px(aim.r.height));
      agentPaint(ring, who.color);
      put(agentsLayer, ring);
      dropLater(ring, settle);
    }
    if (op.action === "click" && aim.r) {
      const ripple = make("div");
      attr(ripple, "class", "ripple");
      css(ripple, "left:" + px(aim.x) + ";top:" + px(aim.y) + ";--delay:" + (first ? 0 : lead) + "ms");
      agentPaint(ripple, who.color);
      put(agentsLayer, ripple);
      dropLater(ripple, settle);
    }
    if (op.action === "type" && aim.r) {
      const at = agentCaret(el, aim.r);
      const caret = make("div");
      attr(caret, "class", "caret");
      css(caret, "left:" + px(at.left) + ";top:" + px(at.top) + ";height:" + px(at.height));
      agentPaint(caret, who.color);
      put(agentsLayer, caret);
      dropLater(caret, settle);
    }
    clearTimeout(a.fade);
    clearTimeout(a.gone);
    a.fade = setTimeout(() => attr(a.node, "class", "agent faded" + side), AGENT_FADE);
    a.gone = setTimeout(() => forgetAgent(a), AGENT_GONE);
    return { shown: true, doing: words.doing, done: words.done, x: aim.x, y: aim.y };
  };
