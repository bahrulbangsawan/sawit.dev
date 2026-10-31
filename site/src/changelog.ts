// Reads CHANGELOG.md as scripts/release.sh writes it:
//   ## [0.0.7] - 2026-10-29
//   ### Features
//   - **mcp**: run sql, start or stop an instance
export type Entry = { scope?: string; text: string }
export type Section = { title: string; entries: Entry[] }
export type Release = { version: string; date?: string; sections: Section[] }
export type Changelog = { releases: Release[] }

// Reads CHANGELOG.md from `source`: its releases, or why it couldn't. Cloudflare keeps the file for
// five minutes. A GitHub that doesn't answer within `timeoutMs` is an error too, so the page shows
// its link instead of hanging.
export async function fetchChangelog(source: string, timeoutMs = 5000): Promise<Changelog | { error: string }> {
  try {
    const res = await fetch(source, {
      cf: { cacheTtl: 300, cacheEverything: true },
      signal: AbortSignal.timeout(timeoutMs),
    } as RequestInit)
    if (!res.ok) return { error: `GitHub answered ${res.status}` }
    return parseChangelog(await res.text())
  } catch (e) {
    const late = e instanceof DOMException && e.name === 'TimeoutError'
    return { error: late ? 'GitHub took too long to answer' : 'GitHub could not be reached' }
  }
}

export function parseChangelog(md: string): Changelog {
  const releases: Release[] = []
  let release: Release | undefined
  let section: Section | undefined
  let entry: Entry | undefined

  for (const raw of md.split('\n')) {
    const line = raw.trimEnd()
    const head = /^## \[?([^\]\s]+)\]?(?:\s+-\s+(\S+))?/.exec(line)
    if (head) {
      release = { version: head[1], date: head[2], sections: [] }
      releases.push(release)
      section = entry = undefined
      continue
    }
    if (!release) continue
    const sub = /^### (.+)/.exec(line)
    if (sub) {
      section = { title: sub[1], entries: [] }
      release.sections.push(section)
      entry = undefined
      continue
    }
    const item = /^[-*] (.+)/.exec(line)
    if (item) {
      if (!section) {
        section = { title: 'Changes', entries: [] }
        release.sections.push(section)
      }
      const scoped = /^\*\*(.+?)\*\*:\s*(.*)/.exec(item[1])
      entry = scoped ? { scope: scoped[1], text: scoped[2] } : { text: item[1] }
      section.entries.push(entry)
      continue
    }
    // a wrapped bullet continues on an indented line
    if (entry && /^\s+\S/.test(raw)) entry.text += ` ${line.trim()}`
  }
  return { releases }
}
