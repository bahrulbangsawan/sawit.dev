import { createFileRoute } from '@tanstack/react-router'
import { createServerFn } from '@tanstack/react-start'
import { Page } from '../components/chrome'
import { DMG, REPO, SITE } from '../content'
import { fetchChangelog } from '../changelog'

const SOURCE = 'https://raw.githubusercontent.com/bahrulbangsawan/sawit.dev/main/CHANGELOG.md'

// Read from GitHub on the server, so a release shows up here without redeploying the site.
// A failed fetch renders a link instead of the list.
const getChangelog = createServerFn({ method: 'GET' }).handler(() => fetchChangelog(SOURCE))

export const Route = createFileRoute('/changelog')({
  loader: () => getChangelog(),
  head: () => ({
    meta: [
      { title: 'Changelog — Sawit' },
      { name: 'description', content: 'Every change to Sawit, by version, read from CHANGELOG.md on GitHub.' },
      { property: 'og:title', content: 'Changelog — Sawit' },
      { property: 'og:description', content: 'Every change to Sawit, by version, read from CHANGELOG.md on GitHub.' },
      { property: 'og:url', content: `${SITE}/changelog` },
    ],
    links: [{ rel: 'canonical', href: `${SITE}/changelog` }],
  }),
  component: ChangelogPage,
})

// `code` spans in commit messages become <code>
function Inline({ text }: { text: string }) {
  return (
    <>
      {text.split(/(`[^`]+`)/).map((part, i) =>
        part.startsWith('`') && part.endsWith('`') && part.length > 1 ? <code key={i}>{part.slice(1, -1)}</code> : part,
      )}
    </>
  )
}

function ChangelogPage() {
  const data = Route.useLoaderData()
  return (
    <Page>
      <header className="sec hero log-hero" id="top">
        <div className="w">
          <h1>Changelog</h1>
          <p className="lead">What’s new in Sawit, release by release.</p>
          <div className="acts">
            <a className="btn pri" href={DMG}>
              <svg aria-hidden="true">
                <use href="#i-down" />
              </svg>
              {/* releases/latest, which isn't CHANGELOG.md's newest section while that one builds,
                  or if its build failed (v0.0.4): so no version here */}
              Download the latest
            </a>
            <a className="btn" href={`${REPO}/blob/main/CHANGELOG.md`}>
              CHANGELOG.md on GitHub ↗
            </a>
          </div>
        </div>
      </header>

      {'error' in data ? (
        <section className="sec">
          <div className="w pad c">
            <h2 className="h2">The changelog didn’t load</h2>
            <p className="lead">
              {data.error}. Read it on <a className="link" href={`${REPO}/blob/main/CHANGELOG.md`}>GitHub</a> instead,
              or reload in a minute.
            </p>
          </div>
        </section>
      ) : (
        data.releases.map((r, i) => (
          <section key={r.version} className="sec" id={`v${r.version}`} aria-labelledby={`v${r.version}-title`}>
            <div className="w release">
              <div className="rel-head">
                <h2 id={`v${r.version}-title`}>
                  <a href={`#v${r.version}`}>v{r.version}</a>
                </h2>
                {r.date && <time dateTime={r.date}>{r.date}</time>}
                {i === 0 && <span className="tag">Latest</span>}
                <a className="link" href={`${REPO}/releases/tag/v${r.version}`}>
                  Release ↗
                </a>
              </div>
              <div className="rel-body">
                {r.sections.map((s) => (
                  <div key={s.title}>
                    <h3 className="eyebrow">{s.title}</h3>
                    <ul>
                      {s.entries.map((e, k) => (
                        <li key={k}>
                          <span>
                            {e.scope && <span className="scope">{e.scope}</span>}
                            {/* as written: a capital would make pg_ctl "Pg_ctl" and mcp "Mcp" */}
                            <Inline text={e.text} />
                          </span>
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </div>
            </div>
          </section>
        ))
      )}
    </Page>
  )
}
