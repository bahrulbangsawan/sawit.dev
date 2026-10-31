import { createFileRoute } from '@tanstack/react-router'
import { Page } from '../components/chrome'
import { DemoVideo, InstallButton, InstanceSwitch } from '../components/interactive'
import { DMG, INSTALL, REPO, SIZE, faq, features, menu } from '../content'

export const Route = createFileRoute('/')({ component: Home })

function Home() {
  return (
    <Page home>
      <Hero />
      <Trio />
      <Stage />
      <Postgres />
      <Features />
      <Binary />
      <Menu />
      <Install />
      <Faq />
      <Cta />
    </Page>
  )
}

function Hero() {
  return (
    <>
      <div className="newsb">
        <p className="news w">
          <span>News:</span> the terminal now splits into panes, and agents address each one.{' '}
          <a href="/changelog">Read the changelog →</a>
        </p>
      </div>
      <header className="sec hero" id="top">
        <div className="w">
          <h1>Local Postgres. No Docker.</h1>
          <p className="lead">Your database, an editor and a terminal in one native Mac app.</p>
          <div className="acts">
            <InstallButton id="install-copy" />
            <a className="btn" href={DMG}>
              <svg aria-hidden="true">
                <use href="#i-down" />
              </svg>
              Download the DMG
            </a>
          </div>
          <pre className="cmd" tabIndex={0} role="group" aria-label="Install command">
            <code>
              <span className="prompt" aria-hidden="true">$ </span>
              {INSTALL}
            </code>
          </pre>
          <small>Apple Silicon · macOS · one {SIZE} binary</small>
        </div>
      </header>
    </>
  )
}

function Trio() {
  return (
    <section className="sec" aria-label="In short">
      <div className="w trio">
        <div>
          <h2>No Docker</h2>
          <p>PostgreSQL 16, 17 or 18 from Homebrew, Postgres.app, your PATH, or a download Sawit fetches itself.</p>
        </div>
        <div>
          <h2>Drawn by the GPU</h2>
          <p>Rust and GPUI in one native binary, with no JVM, Chromium or Node runtime inside.</p>
        </div>
        <div>
          <h2>Agents welcome</h2>
          <p>
            <code>sawit-mcp</code> gives AI agents 117 tools. Destructive ones ask for the name again.
          </p>
        </div>
      </div>
    </section>
  )
}

const sql: [string, string][][] = [
  [['cm', '-- Revenue by status, last 90 days']],
  [['kw', 'select'], ['', ' status,']],
  [['', '       '], ['fn', 'count'], ['', '(*)          '], ['kw', 'as'], ['', ' orders,']],
  [['', '       '], ['fn', 'sum'], ['', '(total)        '], ['kw', 'as'], ['', ' revenue,']],
  [['', '       '], ['fn', 'round'], ['', '('], ['fn', 'avg'], ['', '(total), '], ['nu', '2'], ['', ') '], ['kw', 'as'], ['', ' avg_order']],
  [['kw', 'from'], ['', ' shop.orders']],
  [['kw', 'where'], ['', ' placed_at > '], ['fn', 'now'], ['', '() - interval '], ['st', "'90 days'"]],
  [['kw', 'group by'], ['', ' status']],
  [['kw', 'order by'], ['', ' revenue '], ['kw', 'desc'], ['', ';']],
]

const results = [
  ['paid', '1359', '173743.68', '127.85'],
  ['delivered', '1344', '170494.89', '126.86'],
  ['shipped', '955', '123632.86', '129.46'],
  ['pending', '510', '65960.56', '129.33'],
  ['refunded', '473', '58885.36', '124.49'],
]

// The product drawn in HTML: instances and tables, a terminal, and the SQL editor with its result.
function Stage() {
  return (
    <section className="sec stage" aria-label="Sawit, drawn">
      <div className="w">
        <a className="demo" href="#demo">
          <svg aria-hidden="true">
            <use href="#i-play" />
          </svg>
          Watch the demo
        </a>
        <div className="ide" aria-hidden="true">
          <div className="side">
            <div className="bar">
              <span className="dots">
                <i />
                <i />
                <i />
              </span>
              <span className="fg">Sawit</span>
              <span>acme-store</span>
            </div>
            <p className="grp">Instances</p>
            <div className="it on">
              <i />
              <b>shop</b>
              <span>PostgreSQL 16 · :55432</span>
            </div>
            <div className="it">
              <i />
              <b>kalimantan</b>
              <span>PostgreSQL 17 · stopped</span>
            </div>
            <div className="it">
              <i />
              <b>billing</b>
              <span>PostgreSQL 18 · stopped</span>
            </div>
            <p className="grp">shop · tables</p>
            {['customers', 'orders', 'order_items', 'products', 'reviews'].map((t) => (
              <div key={t} className={`it tb ${t === 'orders' ? 'sel' : ''}`}>
                <i />
                <b>{t}</b>
              </div>
            ))}
            <p className="foot">
              <i /> 1/3 instances running
            </p>
          </div>
          <div className="term">
            <div className="bar">
              <span className="fg">zsh</span>
              <span>acme-store</span>
            </div>
            <pre>
              <span className="pr">acme-store %</span> sawit shop
              {'\n'} customers   order_items   orders
              {'\n'} products    reviews
              {'\n\n'}
              <span className="pr">acme-store %</span> sawit panen shop orders orders.xlsx
              {'\n'} <span className="st">✓</span> 18,500 rows → orders.xlsx
              {'\n\n'}
              <span className="pr">acme-store %</span> <span className="caret" />
            </pre>
          </div>
          <div className="code">
            <div className="tabs">
              <span>SQL 1 · shop</span>
              <span>shop.orders</span>
              <span>orders.tsx</span>
            </div>
            <p className="crumb">
              SQL 1 · shop (Sawit)<span className="run">▷ Run ⌘↩</span>
            </p>
            <div className="lines">
              {sql.map((line, n) => (
                <p key={n} className={n === 6 ? 'cur' : undefined}>
                  {line.map(([cls, text], k) =>
                    cls ? (
                      <span key={k} className={cls}>
                        {text}
                      </span>
                    ) : (
                      text
                    ),
                  )}
                </p>
              ))}
            </div>
            <p className="rmeta">4.4 ms · 5 rows</p>
            <table>
              <thead>
                <tr>
                  <th>status</th>
                  <th>orders</th>
                  <th>revenue</th>
                  <th>avg_order</th>
                </tr>
              </thead>
              <tbody>
                {results.map((r) => (
                  <tr key={r[0]}>
                    {r.map((c) => (
                      <td key={c}>{c}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </section>
  )
}

const steps = [
  { status: 'New', title: 'shop', line: 'PostgreSQL 16 · :55432', facts: ['Listens on localhost only', 'postgres superuser, trust auth'] },
  { status: 'Checked', title: ':55432 is free', line: 'No other process, Homebrew service or container', facts: ['Shadowed binds caught too', 'Start is held on a conflict'] },
  { status: 'Running', title: 'pg_ctl start', line: 'Live log in the window', facts: ['Stop and restart one click away', 'Data kept when it stops'] },
  { status: '.env', title: 'DATABASE_URL', line: 'postgres://postgres@localhost:55432', facts: ['Written to the project’s .env', 'Next.js, Rails, TanStack Start'] },
]

function Postgres() {
  return (
    <section className="sec" id="postgres" aria-labelledby="pg-title">
      <div className="w">
        <div className="pad head">
          <div>
            <h2 id="pg-title" className="h2">
              A database in four steps
            </h2>
            <p className="lead">Name it, let Sawit check the port, start it, and your app connects. Each instance listens on localhost only.</p>
          </div>
          <InstanceSwitch />
        </div>
        <ol className="steps">
          {steps.map((s, i) => (
            <li key={s.status}>
              <p className="eyebrow">
                <span>0{i + 1}</span>
                <span>{s.status}</span>
              </p>
              <h3>{s.title}</h3>
              <code>{s.line.replace('@', '\u200b@')}</code>
              <ul>
                {s.facts.map((f) => (
                  <li key={f}>{f}</li>
                ))}
              </ul>
            </li>
          ))}
        </ol>
      </div>
    </section>
  )
}

function Keys({ keys }: { keys: readonly string[] }) {
  if (!keys.length) return null
  return (
    <span className="keys">
      <span className="sr-only">, shortcut </span>
      {keys.map((k) => (
        <kbd key={k}>{k}</kbd>
      ))}
    </span>
  )
}

function Features() {
  const cells = features.slice(0, 6)
  const rows = features.slice(6)
  return (
    <section className="sec" id="features" aria-labelledby="features-title">
      <div className="w">
        <div className="pad head">
          <div>
            <h2 id="features-title" className="h2">
              One window, every tool
            </h2>
            <p className="lead">Postgres, an editor, a terminal and git, side by side, in Zed’s One Dark colours.</p>
          </div>
          <a className="btn" href={`${REPO}#readme`}>
            Read the README
            <svg aria-hidden="true">
              <use href="#i-right" />
            </svg>
          </a>
        </div>
        <div className="cells">
          {cells.map((f) => (
            <div key={f.name}>
              <div className="frame">
                <img src={`/media/${f.shot}.jpg`} width={1600} height={1001} alt={f.alt} loading="lazy" />
              </div>
              <h3>
                {f.name}
                <Keys keys={f.keys} />
              </h3>
              <p>{f.line}</p>
            </div>
          ))}
        </div>
        <div className="acc">
          {rows.map((f) => (
            <details key={f.name}>
              <summary>{f.name}</summary>
              <div className="acc-body">
                <p>{f.line}</p>
                <div className="frame">
                  <img src={`/media/${f.shot}.jpg`} width={1600} height={1001} alt={f.alt} loading="lazy" />
                </div>
              </div>
            </details>
          ))}
        </div>
      </div>
    </section>
  )
}

function Binary() {
  return (
    <section className="sec os gr" id="demo" aria-labelledby="binary-title">
      <div className="w c">
        <h2 id="binary-title" className="h2 blue">
          One binary, about {SIZE}
        </h2>
        <p className="lead">Welcome screen, instances, a table, SQL, a schema diagram, code with a terminal, ports, git, dev tools, MCP and the canvas, in one take.</p>
        <figure className="video frame">
          <DemoVideo />
        </figure>
        <dl className="stats">
          <div>
            <dt>Binary</dt>
            <dd>{SIZE}</dd>
          </div>
          <div>
            <dt>PostgreSQL</dt>
            <dd>16–18</dd>
          </div>
          <div>
            <dt>Tools for agents</dt>
            <dd>117</dd>
          </div>
          <div>
            <dt>Editor languages</dt>
            <dd>~200</dd>
          </div>
        </dl>
      </div>
    </section>
  )
}

function Menu() {
  return (
    <section className="sec gr" id="cli" aria-labelledby="menu-title">
      <div className="w pad">
        <div className="letter">
          <div className="sheet">
            <img className="seal" src="/favicon.svg" width={96} height={96} alt="" />
            <p className="eyebrow">Served in any terminal</p>
            <h2 id="menu-title">The sawit menu</h2>
            <p className="sheet-lead">Every command works with or without the app open.</p>
            <div className="groups">
              {menu.map((g) => (
                <div key={g.group}>
                  <h3>
                    <span lang="id">{g.group}</span> · {g.gloss}
                  </h3>
                  <ul>
                    {g.items.map((it) => (
                      <li key={it.cmd}>
                        <code>{it.cmd}</code>
                        <span>{it.note}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </div>
            <hr />
            <p className="sign">
              <span lang="id">“Bakar 2 lahan di atas? Asapnya tanggung sendiri.”</span>
              <small>Burn the two plots above? The smoke is on you.</small>
            </p>
          </div>
        </div>
      </div>
    </section>
  )
}

function Install() {
  return (
    <section className="sec gr" id="install" aria-labelledby="install-title">
      <div className="w pad">
        <div className="c">
          <h2 id="install-title" className="h2 blue">
            Install Sawit
          </h2>
          <p className="lead">Two ways in, the same app. After the first install it updates itself.</p>
        </div>
        <div className="plans">
          <div className="plan hl">
            <p className="eyebrow">
              <span>Script</span>
              <span>Recommended</span>
            </p>
            <p className="price">
              curl | sh <span>one command</span>
            </p>
            <p>Downloads the latest release into Applications.</p>
            <ul>
              <li>
                <svg aria-hidden="true">
                  <use href="#i-check" />
                </svg>
                No “cannot verify” warning
              </li>
              <li>
                <svg aria-hidden="true">
                  <use href="#i-check" />
                </svg>
                Install a given release with SAWIT_VERSION=vX.Y.Z
              </li>
              <li>
                <svg aria-hidden="true">
                  <use href="#i-check" />
                </svg>
                Quits and replaces a running Sawit
              </li>
            </ul>
            <InstallButton className="btn pri" />
          </div>
          <div className="plan">
            <p className="eyebrow">
              <span>Disk image</span>
            </p>
            <p className="price">
              .dmg <span>from GitHub releases</span>
            </p>
            <p>Drag Sawit to Applications, the usual way.</p>
            <ul>
              <li>
                <svg aria-hidden="true">
                  <use href="#i-check" />
                </svg>
                Apple Silicon, macOS only
              </li>
              <li>
                <svg aria-hidden="true">
                  <use href="#i-check" />
                </svg>
                Not notarized yet: Open Anyway in Privacy & Security, once
              </li>
              <li>
                <svg aria-hidden="true">
                  <use href="#i-check" />
                </svg>
                Same app, same self-updates
              </li>
            </ul>
            <a className="btn tint" href={DMG}>
              Download Sawit.dmg
            </a>
          </div>
        </div>
      </div>
    </section>
  )
}

function Faq() {
  return (
    <section className="sec" id="faq" aria-labelledby="faq-title">
      <div className="w pad">
        <h2 id="faq-title" className="h2 c">
          Good to know
        </h2>
        <div className="faq">
          {faq.map((item) => (
            <div key={item.q}>
              <h3>{item.q}</h3>
              <p>{item.a}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}

function Cta() {
  return (
    <section className="sec cta" aria-labelledby="cta-title">
      <div className="w">
        <span className="icon">
          <img src="/media/sawit-icon.png" width={1024} height={1024} alt="The Sawit app icon: a palm tree whose trunk is three database discs" />
        </span>
        <h2 id="cta-title" className="blue" lang="id">
          Pasang Sawit
        </h2>
        <p className="lead">Install it with one command on an Apple Silicon Mac.</p>
        <div className="acts">
          <InstallButton />
          <a className="btn" href={REPO}>
            <svg aria-hidden="true">
              <use href="#i-git" />
            </svg>
            View the source
          </a>
        </div>
        <small>Your passwords stay in the macOS Keychain, never a file.</small>
      </div>
    </section>
  )
}
