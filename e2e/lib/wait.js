// Waiting on conditions, never on fixed sleeps: each wait polls what it waits for and gives up
// with a message saying what never happened, and what was last seen.

export class WaitTimeout extends Error {}

/**
 * Polls `probe` until it returns a truthy value (which `until` resolves with) or `timeoutMs`
 * passes. A probe that throws counts as "not yet"; its last error goes into the timeout message.
 */
export async function until(probe, { timeoutMs = 10_000, intervalMs = 150, what = "condition" } = {}) {
  const deadline = Date.now() + timeoutMs;
  let last;
  for (;;) {
    try {
      const value = await probe();
      if (value) return value;
      last = value;
    } catch (e) {
      last = e;
    }
    if (Date.now() > deadline) {
      const seen = last instanceof Error ? last.message : JSON.stringify(last);
      throw new WaitTimeout(`timed out after ${timeoutMs} ms waiting for ${what} (last: ${seen})`);
    }
    await new Promise((r) => setTimeout(r, intervalMs));
  }
}
