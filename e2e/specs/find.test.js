// ⌘F: the same find bar in a code file, a Markdown preview, a PDF, the browser's page, the canvas
// and the terminal. It finds and counts ("1 of 2"), steps with Return, and Escape closes it and
// gives the keyboard back to the view it was in.

import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { activeTab, openCanvas, terminalText, waitForTab, windowState } from "../lib/sawit.js";

const FIELD = { role: "textfield", name: "Find" };

/**
 * ⌘F, the query typed, and the count: `{first} of {total}` (the terminal starts from its newest
 * match, the others from the first), the next after Return, and back with Shift-Return.
 */
async function findAndStep(ctx, query, total, { press = (k) => ctx.win.press(k), first = 1 } = {}) {
  const { win } = ctx;
  await press("cmd+f");
  await win.waitFor(FIELD);
  await win.typeKeys(query);
  await win.waitFor({ role: "status", name: `${first} of ${total}` }, { timeoutMs: 10_000 });
  await win.press("return");
  await win.waitFor({ role: "status", name: `${(first % total) + 1} of ${total}` });
  await win.press("shift+return");
  await win.waitFor({ role: "status", name: `${first} of ${total}` });
  await win.press("escape");
  await win.waitForGone(FIELD);
}

/** A one-page PDF whose text layer reads "needle one" and "and needle two". */
function pdfWithText() {
  const stream = "BT /F1 24 Tf 72 700 Td (needle one) Tj 0 -40 Td (and needle two) Tj ET";
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
    `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`,
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
  ];
  let out = "%PDF-1.4\n";
  const offsets = [];
  objects.forEach((body, i) => {
    offsets.push(out.length);
    out += `${i + 1} 0 obj\n${body}\nendobj\n`;
  });
  const xref = out.length;
  out += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const o of offsets) out += `${String(o).padStart(10, "0")} 00000 n \n`;
  out += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return out;
}

sawitTest("find: ⌘F in a code file finds, counts, steps, and Escape gives the editor its keyboard", async (ctx) => {
  const { win, world } = ctx;
  const file = path.join(world.project, "notes.txt");
  await win.click({ role: "treeitem", name: "notes.txt", within: "Files" });
  await until(async () => (await activeTab(ctx))?.path === file, { what: "notes.txt in a tab" });
  const editor = { role: "textfield", name: "notes.txt" };
  await win.focus(editor);
  await findAndStep(ctx, "line", 2);
  // The keyboard is back in the editor: what is typed lands in the file's text.
  await win.press("cmd+down");
  await win.typeKeys("x");
  await win.waitFor({ ...editor, value: /x$/ });
});

sawitTest("find: ⌘F in a Markdown preview searches what the preview shows", async (ctx) => {
  const { win, world } = ctx;
  await writeFile(path.join(world.project, "doc.md"), "# Title\n\nA **needle** here, and a needle there.\n");
  await ctx.call({ cmd: "open", path: path.join(world.project, "doc.md") });
  await until(async () => (await activeTab(ctx))?.title === "doc.md", { what: "doc.md in a tab" });
  // The preview alone.
  await win.click({ role: "radiobutton", name: /Preview/ }).catch(() => win.click({ role: "button", name: /Preview/ }));
  await findAndStep(ctx, "needle", 2);
});

sawitTest("find: ⌘F in a PDF searches its text layer", async (ctx) => {
  const { win, world } = ctx;
  const file = path.join(world.project, "doc.pdf");
  await writeFile(file, pdfWithText(), "latin1");
  await ctx.call({ cmd: "open", path: file });
  await until(async () => (await activeTab(ctx))?.title === "doc.pdf", { what: "doc.pdf in a tab" });
  await win.click({ role: "radiobutton", name: "doc.pdf", within: "Tabs" });
  await findAndStep(ctx, "needle", 2);
});

sawitTest("find: ⌘F reaches the find bar while the browser's page has the keyboard", { site: true }, async (ctx) => {
  const { win, site } = ctx;
  await ctx.call({ cmd: "open_page", page: "browser" });
  await win.setValue({ role: "textfield", name: "Search or enter address" }, site.url);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.pressInPage("return");
  await until(
    async () => {
      const b = (await windowState(ctx)).browser;
      return b?.url === site.url && !b.loading ? b : null;
    },
    { timeoutMs: 20_000, what: "the fixture site to load" },
  );
  await ctx.call({ cmd: "browser", op: { op: "evaluate", script: "document.body.insertAdjacentHTML('beforeend', '<p>fixture again</p>'); return 1" } });
  await win.pressInPage("q");
  const keys = async () => (await ctx.call({ cmd: "browser", op: { op: "evaluate", script: "return window.__keys" } }))?.value ?? [];
  await until(async () => (await keys()).includes("q"), { what: "the page to have the keyboard" });
  await findAndStep(ctx, "fixture", 2, { press: (k) => win.pressInPage(k) });
  assert.ok(!(await keys()).includes("f"), "the page never got ⌘F's F");
  // Escape gave the page its keyboard back.
  await win.pressInPage("z");
  await until(async () => (await keys()).includes("z"), { what: "the page to have the keyboard again" });
});

sawitTest("find: ⌘F on the canvas finds layers by their text", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  await ctx.call({
    cmd: "canvas",
    op: {
      op: "add",
      elements: [
        { type: "text", x: 0, y: 0, text: "needle one" },
        { type: "text", x: 0, y: 200, text: "needle two" },
      ],
    },
  });
  await waitForTab(ctx, "canvas");
  await win.focus({ role: "canvas" }).catch(() => {});
  await findAndStep(ctx, "needle", 2);
});

sawitTest("find: ⌘F in the terminal searches the scrollback and isn't sent to the shell", async (ctx) => {
  const { win } = ctx;
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), { timeoutMs: 20_000, what: "the prompt" });
  await win.focus({ role: /^text/, name: "Terminal" });
  // Two marks, far apart: the first goes into the scrollback.
  await win.typeKeys("echo mark-A; seq 1 200; echo mark-B");
  await win.press("return");
  await until(async () => /^mark-B$/m.test(await terminalText(ctx)), { what: "the output" });
  // "mark-" is twice in the command line too: 4 matches, the newest current first.
  await findAndStep(ctx, "mark-", 4, { first: 4 });
  // The keyboard is back in the terminal.
  await win.typeKeys("echo back-here");
  await win.press("return");
  await until(async () => /^back-here$/m.test(await terminalText(ctx)), { what: "the typed line" });
});
