// Reads CHANGELOG.md as scripts/release.sh writes it:
//   ## [0.0.7] - 2026-10-29
//   ### Features
//   - **mcp**: run sql, start or stop an instance
export type Entry = { scope?: string; text: string }
export type Section = { title: string; entries: Entry[] }
export type Release = { version: string; date?: string; sections: Section[] }
export type Changelog = { releases: Release[] }

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
