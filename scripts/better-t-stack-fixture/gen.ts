// Runs upstream's builder logic (create-better-t-stack a9bd456ccc9d, apps/web/src/lib) on
// golden stacks, random stacks and random click sequences, and prints what it answers as JSON:
// the fixture Sawit's Rust port is tested against.
import { DEFAULT_STACK, PRESET_TEMPLATES, TECH_OPTIONS, type StackState } from "@/lib/constant";
import { sanitizeAddons, sanitizeExamples } from "@/lib/sanitize-stack-addons";
import { applyStackUpdate, resolveStackCompatibility } from "@/lib/stack-compatibility";
import {
  CATEGORY_ORDER,
  formatProjectName,
  generateStackCommand,
  getDesktopBuildNote,
  getSelectedTechs,
} from "@/lib/stack-utils";
import { getDisabledReason, isOptionCompatible, validateProjectName } from "@/lib/stack-validation";

function mulberry32(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = mulberry32(20261101);
const pick = <T>(xs: readonly T[]) => xs[Math.floor(rand() * xs.length)];

const withName = (stack: StackState) => ({
  ...stack,
  projectName: formatProjectName(stack.projectName),
});

// Upstream's getTechSelectionUpdate and getSelectedTechRemovalUpdate (use-stack-builder.ts),
// with StackUpdateSchema's parse left out: every id here is one of TECH_OPTIONS'.
function toggleSingle(values: readonly string[], id: string) {
  return values.includes(id) ? ["none"] : [id];
}
function toggleMulti(values: readonly string[], id: string) {
  return values.includes(id) ? values.filter((value) => value !== id) : [...values, id];
}
function selection(stack: StackState, category: string, techId: string): Partial<StackState> {
  const effective = resolveStackCompatibility(stack).stack as any;
  if (!isOptionCompatible(effective, category as any, techId)) return {};
  const current = effective[category];
  if (category === "webFrontend") return { webFrontend: toggleSingle(effective.webFrontend, techId) as any };
  if (category === "nativeFrontend")
    return { nativeFrontend: toggleSingle(effective.nativeFrontend, techId) as any };
  if (category === "addons")
    return { addons: sanitizeAddons(toggleMulti(effective.addons, techId)).sort() as any };
  if (category === "examples")
    return { examples: sanitizeExamples(toggleMulti(effective.examples, techId)).sort() as any };
  if (current !== techId) return { [category]: techId } as any;
  if (category === "git") return { git: techId === "true" ? "false" : "true" };
  if (category === "install") return { install: techId === "true" ? "false" : "true" };
  return {};
}
function removal(stack: StackState, category: string, techId: string): Partial<StackState> {
  const effective = resolveStackCompatibility(stack).stack as any;
  const value = effective[category];
  const options = (TECH_OPTIONS as any)[category] || [];
  const hasNone = options.some((o: any) => o.id === "none");
  const forceNone = category === "addons" || category === "examples";
  if (Array.isArray(value)) {
    const next = value.filter((id: string) => id !== techId);
    return { [category]: next.length === 0 && (hasNone || forceNone) ? ["none"] : next } as any;
  }
  if (value === techId && hasNone) return { [category]: "none" } as any;
  return {};
}

function disabledMap(stack: StackState) {
  const out: Record<string, Record<string, string>> = {};
  for (const category of CATEGORY_ORDER) {
    for (const option of (TECH_OPTIONS as any)[category]) {
      const reason = getDisabledReason(stack, category, option.id);
      if (reason !== null) (out[category] ??= {})[option.id] = reason;
    }
  }
  return out;
}

const NAMES = ["my-better-t-app", "my app", "it's mine", "café", "ok_name.v2", "a/b", "", "Shop  Front"];

function randomStack(): StackState {
  const entries = CATEGORY_ORDER.map((category) => {
    const options = (TECH_OPTIONS as any)[category] as { id: string }[];
    if (category === "addons" || category === "examples") {
      const count = Math.floor(rand() * Math.min(options.length, 4));
      // A Fisher-Yates shuffle: upstream's `sort(() => 0.5 - Math.random())` gives another
      // order from run to run, even with a seeded random.
      const shuffled = [...options].filter(({ id }) => id !== "none");
      for (let i = shuffled.length - 1; i > 0; i--) {
        const j = Math.floor(rand() * (i + 1));
        [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
      }
      const ids = shuffled.slice(0, count).map(({ id }) => id);
      return [category, ids.length ? ids : ["none"]];
    }
    const id = pick(options).id;
    return [category, category === "webFrontend" || category === "nativeFrontend" ? [id] : id];
  });
  return { ...(Object.fromEntries(entries) as any), projectName: pick(NAMES), yolo: "false" };
}


// One stack as one line: the categories in Sawit's field order, lists joined by commas, then
// the project name.
const enc = (s: any) =>
  [
    s.webFrontend.join(","), s.nativeFrontend.join(","), s.backend, s.runtime, s.api, s.database,
    s.orm, s.dbSetup, s.webDeploy, s.serverDeploy, s.auth, s.payments, s.packageManager,
    s.addons.join(","), s.examples.join(","), s.git, s.install, s.projectName ?? "",
  ].join("|");
const encChanges = (cs: any[]) => cs.map((c) => `${c.category}: ${c.message}`);
const encDisabled = (d: Record<string, Record<string, string>>) =>
  Object.entries(d).flatMap(([c, m]) => Object.entries(m).map(([id, r]) => `${c}/${id}: ${r}`));

const golden: any[] = [];
const stacks: Array<[string, StackState]> = [
  ["default", DEFAULT_STACK],
  ...PRESET_TEMPLATES.map((p) => [p.id, p.stack] as [string, StackState]),
];
for (const [name, stack] of stacks) {
  for (const pm of ["bun", "pnpm", "npm"] as const) {
    const resolved = resolveStackCompatibility({ ...stack, packageManager: pm });
    golden.push({
      name,
      packageManager: pm,
      changes: encChanges(resolved.changes),
      stack: enc(resolved.stack),
      command: generateStackCommand(withName(resolved.stack)),
    });
  }
}

const cases: any[] = [];
for (let i = 0; i < 60; i++) {
  const input = randomStack();
  const resolved = resolveStackCompatibility(input);
  cases.push({
    input: enc(input),
    resolved: enc(resolved.stack),
    changes: encChanges(resolved.changes),
    disabled: encDisabled(disabledMap(resolved.stack)),
    selected: getSelectedTechs(resolved.stack).map((t) => `${t.category}/${t.id}`),
    note: getDesktopBuildNote(resolved.stack),
    nameError: validateProjectName(formatProjectName(resolved.stack.projectName)) ?? null,
    command: generateStackCommand(withName(resolved.stack)),
  });
}

const walks: any[] = [];
for (let w = 0; w < 40; w++) {
  const startPm = pick(["bun", "pnpm", "npm"]);
  let stack: StackState = { ...DEFAULT_STACK, packageManager: startPm as any };
  const steps: any[] = [];
  for (let s = 0; s < 10; s++) {
    const selected = getSelectedTechs(resolveStackCompatibility(stack).stack);
    let op: string, category: string, id: string;
    if (rand() < 0.2 && selected.length) {
      const tech = pick(selected);
      [op, category, id] = ["remove", tech.category, tech.id];
    } else {
      category = pick(CATEGORY_ORDER);
      id = pick((TECH_OPTIONS as any)[category]).id;
      op = "select";
    }
    const update = op === "select" ? selection(stack, category, id) : removal(stack, category, id);
    const result = applyStackUpdate(stack, update);
    stack = result.stack;
    steps.push({ op, category, id, after: enc(stack), changes: encChanges(result.changes) });
  }
  walks.push({ packageManager: startPm, steps });
}


// Upstream's CLI compatibility cases (apps/cli/test/compatibility.test.ts) that the builder can
// express, on the test's base stack: what the builder makes of each.
const base: any = {
  projectName: "compat", webFrontend: ["tanstack-router"], nativeFrontend: ["none"], backend: "hono",
  runtime: "bun", database: "sqlite", orm: "drizzle", dbSetup: "none", api: "trpc", auth: "none",
  payments: "none", packageManager: "bun", addons: ["none"], examples: ["none"], webDeploy: "none",
  serverDeploy: "none", git: "false", install: "false", yolo: "false",
};
const rejected: Array<[string, any]> = [
  ...["nuxt", "svelte", "solid", "astro"].map((f) => [`${f}-trpc`, { webFrontend: [f] }] as [string, any]),
  ["multiple-web", { webFrontend: ["next", "svelte"] }],
  ["multiple-native", { nativeFrontend: ["native-bare", "native-uniwind"] }],
  ["workers-express", { backend: "express", runtime: "workers" }],
  ["mongoose-postgres", { database: "postgres", orm: "mongoose" }],
  ["database-without-orm", { orm: "none" }],
  ["neon-sqlite", { dbSetup: "neon" }],
  ["d1-without-deploy", { runtime: "workers", dbSetup: "d1" }],
  ["polar-clerk", { auth: "clerk", payments: "polar" }],
  ["clerk-mixed-nuxt", { webFrontend: ["nuxt"], nativeFrontend: ["native-bare"], api: "orpc", auth: "clerk" }],
  ["convex-database", { backend: "convex", runtime: "none", api: "none" }],
  ["todo-without-db", { database: "none", orm: "none", examples: ["todo"] }],
  ["todo-without-api", { api: "none", examples: ["todo"] }],
  ["solid-ai", { webFrontend: ["solid"], api: "orpc", examples: ["ai"] }],
  ["task-runner-conflict", { addons: ["turborepo", "nx"] }],
  ["observability-conflict", { addons: ["evlog", "axiom"] }],
  ["native-web-deploy", { webFrontend: ["none"], nativeFrontend: ["native-bare"], webDeploy: "vercel" }],
  ["vercel-bun-runtime", { serverDeploy: "vercel" }],
  ["docker-workers", { runtime: "workers", serverDeploy: "docker" }],
  ["drizzle-mongodb", { database: "mongodb" }],
  ["turso-postgres", { database: "postgres", dbSetup: "turso" }],
  ["prisma-postgres-mysql", { database: "mysql", dbSetup: "prisma-postgres" }],
  ["supabase-sqlite", { dbSetup: "supabase" }],
  ["atlas-postgres", { database: "postgres", dbSetup: "mongodb-atlas" }],
  ["planetscale-sqlite", { dbSetup: "planetscale" }],
  ["docker-sqlite", { dbSetup: "docker" }],
  ["workers-mongodb", { runtime: "workers", database: "mongodb", orm: "prisma" }],
  ["self-runtime", { webFrontend: ["next"], backend: "self-next" }],
  ["frontend-only-api", { backend: "none", runtime: "none", database: "none", orm: "none" }],
  ["convex-mixed-better-auth", {
    webFrontend: ["nuxt"], nativeFrontend: ["native-bare"], backend: "convex", runtime: "none",
    database: "none", orm: "none", api: "none", auth: "better-auth",
  }],
  ["desktop-fullstack", { webFrontend: ["next"], backend: "self-next", runtime: "none", addons: ["tauri"] }],
  ["d1-postgres", { database: "postgres", dbSetup: "d1", runtime: "workers", serverDeploy: "cloudflare" }],
  ["orm-without-database", { database: "none" }],
  ["server-without-runtime", { runtime: "none" }],
  ["workers-without-deployment", { runtime: "workers" }],
  ["cloudflare-bun-runtime", { serverDeploy: "cloudflare" }],
  ["prisma-workers-runtime", { runtime: "workers", serverDeploy: "prisma" }],
  ["next-cloudflare-manual-prisma-postgres", {
    webFrontend: ["next"], backend: "self-next", runtime: "none", database: "postgres", orm: "prisma",
    webDeploy: "cloudflare",
  }],
  ["unsupported-pwa-frontend", { webFrontend: ["nuxt"], api: "orpc", addons: ["pwa"] }],
];
const accepted: Array<[string, any]> = [
  ["self-d1", {
    webFrontend: ["next"], backend: "self-next", runtime: "none", dbSetup: "d1", webDeploy: "cloudflare",
    auth: "better-auth",
  }],
  ["server-only-clerk", { webFrontend: ["none"], auth: "clerk" }],
  ["mixed-clerk", { webFrontend: ["tanstack-router"], nativeFrontend: ["native-uniwind"], auth: "clerk" }],
];
const compat = [...rejected.map((c) => [...c, "rejected"]), ...accepted.map((c) => [...c, "accepted"])].map(
  ([name, options, kind]: any) => {
    const input = { ...base, ...options };
    const resolved = resolveStackCompatibility(input);
    return {
      name,
      kind,
      input: enc(input),
      resolved: enc(resolved.stack),
      changes: encChanges(resolved.changes),
      command: generateStackCommand(withName(resolved.stack)),
    };
  },
);

console.log(JSON.stringify({ commit: "a9bd456ccc9d", cli: "3.49.0", golden, compat, cases, walks }, null, 1));
