// Sawit's entry into mesurer: the styled <Mesurer> (its stylesheet adopted into the shadow root it
// renders in), behind one global the glue calls. It makes no host and installs no keyboard gate:
// Sawit's glue (crates/core/src/browser/page/mesurer/) builds the host and its shadow root in the
// `sawit-mesurer` content world, and the gate (gate.ts) runs in the page's own world.
//
// mesurer itself is used as it is, at the commit rebuild.sh pins: no file of it is changed.
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import Mesurer from "mesurer-src/mesurer";
import type { MesurerProps } from "mesurer-src/mesurer-client";

declare const __SAWIT_MESURER_VERSION__: string;
declare const __SAWIT_MESURER_COMMIT__: string;

type Props = Omit<MesurerProps, "portalTarget">;

let root: Root | null = null;
let target: ShadowRoot | null = null;
let current: Props = {};

const draw = () => {
  if (!root || !target) return;
  const portalTarget = target;
  const props = current;
  // Synchronously, so the glue's `ready` follows a tree that is there.
  flushSync(() => root?.render(<Mesurer {...props} portalTarget={portalTarget} />));
};

const bundle = Object.freeze({
  version: __SAWIT_MESURER_VERSION__,
  commit: __SAWIT_MESURER_COMMIT__,
  /** Renders mesurer into `container`, inside `shadowRoot`; a second call starts over. */
  render(container: HTMLElement, shadowRoot: ShadowRoot, props: Props) {
    if (root) root.unmount();
    target = shadowRoot;
    current = { ...(props ?? {}) };
    root = createRoot(container);
    draw();
  },
  /** Renders again with `props` over the ones it has. */
  update(props: Props) {
    current = { ...current, ...(props ?? {}) };
    draw();
  },
  /** Unmounts mesurer: its own cleanup saves the workspace through `persistence` first. */
  unmount() {
    const was = root;
    root = null;
    target = null;
    current = {};
    was?.unmount();
  },
  isMounted: () => root !== null,
});

(globalThis as typeof globalThis & { __sawitMesurerBundle?: typeof bundle }).__sawitMesurerBundle =
  bundle;
