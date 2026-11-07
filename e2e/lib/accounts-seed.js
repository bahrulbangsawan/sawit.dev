// Accounts written into a throwaway home before Sawit starts, for the Accounts page's specs.

import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

/**
 * Three tracked Claude Code accounts with usage, written before Sawit starts: what the design
 * screenshots show. Sample data: made-up names and figures.
 */
export function seedAccounts(world) {
  const now = Date.now() / 1000;
  const accounts = [
    { id: "a1", name: "work", color: "#D97757", five: [23, 2.2], seven: [41, 76] },
    { id: "a2", name: "personal", color: "#4F8EF7", five: [72, 3.5], seven: [88, 30] },
    { id: "a3", name: "client", color: "#3DBE8B", five: [100, 0.6], seven: [64, 120] },
  ];
  const dir = path.join(world.sawitHome, "accounts");
  mkdirSync(path.join(dir, "usage"), { recursive: true, mode: 0o700 });
  const records = [];
  for (const [order, a] of accounts.entries()) {
    const folder = path.join(world.home, `.claude-${a.name}`);
    mkdirSync(folder, { recursive: true });
    writeFileSync(
      path.join(folder, "settings.json"),
      JSON.stringify({ statusLine: { type: "command", command: `/x/sawit-statusline --account ${a.id}` } }),
    );
    records.push({ id: a.id, kind: "claude_code", name: a.name, dir: folder, color: a.color, order, tracking: true, created_at: Math.floor(now) });
    const win = ([used, hours]) => ({ usedPercentage: used, resetsAt: now + hours * 3600 });
    writeFileSync(
      path.join(dir, "usage", `${a.id}.json`),
      JSON.stringify({ schema: 1, profileId: a.id, observedAt: now - 120, lastSeenAt: now - 120, fiveHour: win(a.five), sevenDay: win(a.seven) }),
    );
  }
  writeFileSync(path.join(dir, "accounts.json"), JSON.stringify({ accounts: records }));
  writeFileSync(path.join(world.sawitHome, "accounts", "active-claude_code"), path.join(world.home, ".claude-work"));
}

