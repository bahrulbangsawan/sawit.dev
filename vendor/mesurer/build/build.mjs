// Builds Sawit's mesurer bundle from mesurer's source (MESURER_SRC, a clone at the pinned commit
// with its packages installed and `generate:styles` run): bundle/mesurer.js (entry.tsx: React,
// mesurer and its stylesheet, one IIFE) and bundle/gate.js (gate.ts, for the page's world), with
// SHA256SUMS and BUILD.json beside them. Run it through rebuild.sh.
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const VERSION = "0.2.7";
const COMMIT = "5c2fdaefde54591329d94b179af6ef5f32e15676";
const COPYRIGHT = "Copyright (c) 2026 Julien Thibeaut";

const here = path.dirname(fileURLToPath(import.meta.url));
const src = process.env.MESURER_SRC;
if (!src) throw new Error("MESURER_SRC names mesurer's clone");
const out = process.env.MESURER_OUT ?? path.join(here, "..", "bundle");
const pkg = path.join(src, "packages", "mesurer");
const modules = path.join(pkg, "node_modules");
// The esbuild mesurer's own build (tsup) runs, at the lockfile's version: pnpm puts it beside
// tsup's real folder.
const requireFrom = (name) => createRequire(path.join(realpathSync(path.join(modules, name)), "package.json"));
const esbuild = requireFrom("tsup")("esbuild");
const versionOf = (dir) => JSON.parse(readFileSync(path.join(dir, "package.json"), "utf8")).version;
const react = versionOf(path.join(modules, "react"));
const reactDom = versionOf(path.join(modules, "react-dom"));

// The MIT text as mesurer ships it, checked to be the one the banner names.
const license = readFileSync(path.join(pkg, "LICENSE"), "utf8");
const [title, , copyright, , ...permission] = license.trimEnd().split("\n");
if (title !== "MIT License" || copyright !== COPYRIGHT) {
  throw new Error(`mesurer's LICENSE isn't the MIT text with "${COPYRIGHT}"`);
}
const banner = (extra) =>
  [
    `/*! mesurer ${VERSION} (ibelick/mesurer@${COMMIT}) | MIT License | ${COPYRIGHT}`,
    " *",
    ...permission.map((line) => (line ? ` * ${line}` : " *")),
    ...(extra ? [" *", ` * ${extra}`] : []),
    " */",
  ].join("\n");

const common = {
  bundle: true,
  format: "iife",
  platform: "browser",
  target: "safari16",
  jsx: "automatic",
  minify: true,
  legalComments: "eof",
  write: false,
  metafile: true,
  logLevel: "warning",
  nodePaths: [modules],
  alias: {
    "mesurer-src": pkg,
    "mesurer-ext": path.join(src, "apps", "extension", "src"),
  },
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
    // Reviews are left out, as in mesurer's npm package and extension: they need snapdom and a
    // hosted viewer. `__MESURER_EXTENSION__` stays unset, which keeps the in-page recording
    // editor and its encoders.
    "globalThis.__MESURER_NO_REVIEW__": "true",
    __MESURER_SNAPDOM_CAPTURE__: "false",
    __SAWIT_MESURER_VERSION__: JSON.stringify(VERSION),
    __SAWIT_MESURER_COMMIT__: JSON.stringify(COMMIT),
  },
};

// The packages a build took code from, by name, from its metafile.
const packagesIn = (metafile) => {
  const names = new Set();
  for (const input of Object.keys(metafile.inputs)) {
    const at = input.lastIndexOf("node_modules/");
    if (at < 0) continue;
    const [scope, name] = input.slice(at + "node_modules/".length).split("/");
    names.add(scope.startsWith("@") ? `${scope}/${name}` : scope);
  }
  return [...names].sort();
};

const build = async (entry, file, extra) => {
  const result = await esbuild.build({
    ...common,
    entryPoints: [path.join(here, entry)],
    outfile: path.join(out, file),
    banner: { js: banner(extra) },
  });
  const bytes = Buffer.from(result.outputFiles[0].contents);
  writeFileSync(path.join(out, file), bytes);
  return {
    file,
    bytes: bytes.length,
    gzip: gzipSync(bytes, { level: 9 }).length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    packages: packagesIn(result.metafile),
  };
};

mkdirSync(out, { recursive: true });
const built = [
  await build(
    "entry.tsx",
    "mesurer.js",
    `Bundled with React and React DOM ${react} (MIT License, Copyright (c) Meta Platforms, Inc. and affiliates) and Tailwind CSS output (MIT License, Copyright (c) Tailwind Labs, Inc.).`,
  ),
  await build("gate.ts", "gate.js"),
];
if (built[0].packages.includes("@zumer/snapdom")) throw new Error("snapdom reached the bundle");
writeFileSync(
  path.join(out, "SHA256SUMS"),
  built.map((b) => `${b.sha256}  ${b.file}\n`).join(""),
);
const info = {
  mesurer: { version: VERSION, commit: COMMIT, repository: "https://github.com/ibelick/mesurer" },
  tools: {
    node: process.versions.node,
    pnpm: process.env.PNPM_VERSION ?? null,
    esbuild: esbuild.version,
    react,
    "react-dom": reactDom,
    scheduler: versionOf(
      path.dirname(
        requireFrom("react-dom").resolve("scheduler/package.json"),
      ),
    ),
    tailwindcss: versionOf(path.join(modules, "tailwindcss")),
  },
  files: Object.fromEntries(built.map(({ file, ...rest }) => [file, rest])),
};
writeFileSync(path.join(out, "BUILD.json"), `${JSON.stringify(info, null, 2)}\n`);
console.log(built.map((b) => `${b.file}: ${b.bytes} bytes, ${b.gzip} gzip`).join("\n"));
