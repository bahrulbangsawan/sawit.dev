// A database in four steps, beside a switch that starts and stops an instance.
import { InstanceSwitch } from '../interactive'

const steps = [
  { status: 'New', title: 'shop', line: 'PostgreSQL 16 · :55432', facts: ['Listens on localhost only', 'postgres superuser, trust auth'] },
  { status: 'Checked', title: ':55432 is free', line: 'No other process, Homebrew service or container', facts: ['Shadowed binds caught too', 'Start is held on a conflict'] },
  { status: 'Running', title: 'pg_ctl start', line: 'Live log in the window', facts: ['Stop and restart one click away', 'Data kept when it stops'] },
  { status: '.env', title: 'DATABASE_URL', line: 'postgres://postgres@localhost:55432', facts: ['Written to the project’s .env', 'Next.js, Rails, TanStack Start'] },
]

export function Postgres() {
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
