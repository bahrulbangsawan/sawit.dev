  // ---- Theme: Sawit's colours, which the app sends (`theme`) ----

  const theme = {
    bg: "#282c33",
    fg: "#dce0e5",
    muted: "#a9afbc",
    border: "#464b57",
    field: "#1f2227",
    accent: "#363c46",
    primary: "#74ade8",
    primaryFg: "#1b1d22",
    danger: "#d07277",
    ring: "#74ade8",
    font: "-apple-system, BlinkMacSystemFont, system-ui, sans-serif",
    mono: "ui-monospace, Menlo, monospace",
  };
  const MEASURE = "#ff375f";

  const STYLE = `
:host { all: initial; }
.root { position: absolute; inset: 0; overflow: hidden; font: 12px/1.4 var(--font); color: var(--fg);
  -webkit-font-smoothing: antialiased; letter-spacing: 0; text-transform: none; text-align: left; direction: ltr; }
.root * { box-sizing: border-box; }
.box { position: absolute; box-sizing: border-box; pointer-events: none; }
.label { position: absolute; pointer-events: none; display: flex; gap: 6px; align-items: baseline; padding: 3px 7px;
  border-radius: 6px; background: var(--bg); border: 1px solid var(--border); color: var(--fg); white-space: nowrap;
  font: 500 11px/16px var(--mono); box-shadow: 0 2px 8px rgba(0,0,0,.24); max-width: calc(100vw - 16px); overflow: hidden; }
.label .size { color: var(--muted); font-weight: 400; font-variant-numeric: tabular-nums; }
.mark { position: absolute; pointer-events: auto; min-width: 24px; height: 24px; padding: 0 6px; border-radius: 12px;
  border: 2px solid var(--bg); background: var(--primary); color: var(--primary-fg); font: 600 11px/20px var(--font);
  font-variant-numeric: tabular-nums; text-align: center; cursor: pointer; box-shadow: 0 1px 4px rgba(0,0,0,.3); margin: 0; }
.mark:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
.mark.open { background: var(--fg); color: var(--bg); }
.pop { position: absolute; pointer-events: auto; width: 320px; max-width: calc(100vw - 16px); display: flex;
  flex-direction: column; gap: 12px; padding: 12px; border-radius: 10px; background: var(--bg); color: var(--fg);
  border: 1px solid var(--border); box-shadow: 0 12px 32px rgba(0,0,0,.32), 0 2px 6px rgba(0,0,0,.2); }
.pop[hidden] { display: none; }
.head { display: flex; gap: 8px; align-items: flex-start; }
.badge { flex: none; min-width: 22px; height: 22px; padding: 0 6px; border-radius: 11px; background: var(--primary);
  color: var(--primary-fg); font: 600 11px/22px var(--font); text-align: center; font-variant-numeric: tabular-nums; }
.what { flex: 1; min-width: 0; }
.title { font: 500 12px/18px var(--mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sub { color: var(--muted); font-size: 11px; line-height: 16px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  font-variant-numeric: tabular-nums; }
.walk { flex: none; display: flex; gap: 4px; }
.walk[hidden] { display: none; }
button { font: 500 12px/1 var(--font); color: var(--fg); cursor: pointer; }
.icon { width: 24px; height: 24px; display: inline-flex; align-items: center; justify-content: center; padding: 0;
  border-radius: 6px; border: 1px solid var(--border); background: transparent; color: var(--muted); }
.icon:hover { color: var(--fg); background: var(--accent); }
.icon:disabled { opacity: .45; cursor: default; background: transparent; }
.kinds { display: flex; padding: 2px; gap: 2px; border-radius: 7px; background: var(--field); border: 1px solid var(--border); }
.kind { flex: 1; height: 24px; border: 0; border-radius: 5px; background: transparent; color: var(--muted); }
.kind:hover { color: var(--fg); }
.kind[aria-checked="true"] { background: var(--accent); color: var(--fg); box-shadow: 0 1px 2px rgba(0,0,0,.25); }
textarea { display: block; width: 100%; min-height: 64px; max-height: 160px; resize: none; margin: 0; padding: 8px 10px;
  border-radius: 7px; border: 1px solid var(--border); background: var(--field); color: var(--fg);
  font: 13px/1.45 var(--font); outline: none; }
textarea::placeholder { color: var(--muted); opacity: 1; }
textarea:focus { border-color: var(--ring); box-shadow: 0 0 0 2px color-mix(in srgb, var(--ring) 30%, transparent); }
.foot { display: flex; gap: 8px; align-items: center; }
.grow { flex: 1; }
.btn { height: 28px; padding: 0 10px; display: inline-flex; gap: 6px; align-items: center; border-radius: 7px;
  border: 1px solid var(--border); background: transparent; }
.btn:hover { background: var(--accent); }
.btn.primary { background: var(--primary); border-color: var(--primary); color: var(--primary-fg); }
.btn.primary:hover { filter: brightness(1.08); }
.btn.danger { color: var(--danger); border-color: transparent; padding: 0 6px; }
.btn.danger:hover { background: color-mix(in srgb, var(--danger) 14%, transparent); }
.btn[hidden] { display: none; }
kbd { font: 500 10px/1 var(--font); opacity: .7; }
button:focus-visible, .kind:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
.flash { position: absolute; pointer-events: none; border-radius: 2px; outline: 3px solid var(--primary); outline-offset: 2px; }
.root.shot > :not(.agents) { display: none; }
.agents, .agents * { pointer-events: none; }
.agent { position: absolute; left: 0; top: 0; z-index: 1; }
.agent.faded { opacity: .4; }
.agent.faded .doing, .agent.faded .typing { display: none; }
.agent .tip { position: absolute; left: 0; top: 0; width: 13px; height: 19px;
  filter: drop-shadow(1px 0 0 #fff) drop-shadow(-1px 0 0 #fff) drop-shadow(0 1px 0 #fff) drop-shadow(0 -1px 0 #fff) drop-shadow(0 1px 2px rgba(0,0,0,.35)); }
.agent .arrow { width: 100%; height: 100%; background: var(--c);
  clip-path: polygon(0 0, 0 16px, 4.4px 12.3px, 7.3px 18.6px, 9.9px 17.4px, 7.1px 11.2px, 12.6px 11.2px); }
.agent .pill { position: absolute; left: 12px; top: 17px; display: flex; gap: 6px; align-items: baseline; max-width: 280px;
  padding: 2px 8px; border-radius: 10px; background: var(--c); color: #fff; font: 600 11px/16px var(--font); white-space: nowrap;
  overflow: hidden; text-overflow: ellipsis; box-shadow: 0 0 0 1.5px #fff, 0 2px 6px rgba(0,0,0,.25); }
.agent .doing { font-weight: 400; overflow: hidden; text-overflow: ellipsis; }
.agent .typing { font-weight: 400; }
.agent.west .pill { left: auto; right: 2px; }
.agent.north .pill { top: auto; bottom: 4px; }
.ring { position: absolute; border-radius: 4px; outline: 2px solid var(--c); outline-offset: 2px;
  box-shadow: 0 0 0 4px rgba(255,255,255,.55); }
.ripple { position: absolute; width: 28px; height: 28px; margin: -14px 0 0 -14px; border-radius: 50%;
  border: 2px solid var(--c); box-shadow: 0 0 0 1px #fff; opacity: 0; }
.caret { position: absolute; width: 2px; border-radius: 1px; background: var(--c); box-shadow: 0 0 0 1px #fff; }
@media (prefers-reduced-motion: reduce) {
  .ripple { opacity: 1; }
}
@media (prefers-reduced-motion: no-preference) {
  .pop.in { animation: pop-in 140ms cubic-bezier(0.2, 0, 0, 1); }
  .flash { animation: flash 600ms ease-out forwards; }
  .agent { transition-property: transform, opacity; transition-duration: var(--glide, 120ms), 200ms;
    transition-timing-function: cubic-bezier(0.2, 0, 0, 1), ease-out; }
  .ripple { animation: ripple 420ms cubic-bezier(0.2, 0, 0, 1) var(--delay, 0ms) forwards; }
  .caret, .agent .typing { animation: blink 1s steps(1) infinite; }
}
@keyframes pop-in { from { opacity: 0; transform: translateY(var(--rise, 4px)); } to { opacity: 1; transform: none; } }
@keyframes flash { 0% { opacity: 1; } 30% { opacity: .2; } 60% { opacity: 1; } 100% { opacity: 0; } }
@keyframes ripple { from { opacity: 1; transform: scale(.4); } to { opacity: 0; transform: scale(1.6); } }
@keyframes blink { 50% { opacity: 0; } }
`;
  const UP = '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 13V3M4 7l4-4 4 4"/></svg>';
  const DOWN = '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 3v10M4 9l4 4 4-4"/></svg>';
  const TRASH = '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4"/></svg>';
  // Static markup: what the page wrote is set as text later, never parsed.
  const MARKUP =
    '<div class="root" part="root"><div class="layer"></div><div class="agents" aria-hidden="true"></div>' +
    '<div class="pop" role="dialog" aria-labelledby="pop-title" aria-describedby="pop-sub" hidden>' +
    '<div class="head"><span class="badge" id="pop-n"></span><div class="what"><div class="title" id="pop-title"></div>' +
    '<div class="sub" id="pop-sub"></div></div><div class="walk" id="pop-walk">' +
    '<button class="icon" id="pop-up" type="button" aria-label="Select the parent element" title="Parent element (⌥↑)">' + UP + "</button>" +
    '<button class="icon" id="pop-down" type="button" aria-label="Select the child element again" title="Child element (⌥↓)">' + DOWN + "</button>" +
    "</div></div>" +
    '<div class="kinds" role="radiogroup" aria-label="Kind">' +
    '<button class="kind" id="kind-fix" type="button" role="radio">Fix</button>' +
    '<button class="kind" id="kind-change" type="button" role="radio">Change</button>' +
    '<button class="kind" id="kind-question" type="button" role="radio">Question</button>' +
    '<button class="kind" id="kind-copy" type="button" role="radio" title="Ask the agent to reuse this element; adds its HTML">Copy</button>' +
    "</div>" +
    '<textarea id="pop-comment" rows="3" placeholder="What should change?" aria-label="Comment for the agent"></textarea>' +
    '<div class="foot"><button class="btn danger" id="pop-remove" type="button" aria-label="Remove this annotation">' + TRASH + "Remove</button>" +
    '<span class="grow"></span><button class="btn" id="pop-cancel" type="button">Cancel <kbd>Esc</kbd></button>' +
    '<button class="btn primary" id="pop-save" type="button">Save <kbd>↩</kbd></button></div>' +
    "</div></div>";
  const KINDS = ["fix", "change", "question", "copy"];
