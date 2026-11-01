// One window, every tool: six feature cells with screenshots, the rest as expanders.
import { REPO } from '../../content'

// Capabilities, shortcuts and wording from the README; screenshots from docs/media.
const features = [
  {
    name: 'SQL editor',
    keys: ['⌘', '↩'],
    line: 'Runs the selection, or the whole editor. Results stream in, with DETAIL and HINT on errors.',
    shot: 'sql-editor',
    alt: 'SQL editor with an aggregation query and its result table',
  },
  {
    name: 'Table browser',
    keys: [],
    line: '50 rows a page over a separate read-only connection. Export what matches to CSV or Excel.',
    shot: 'table-grid',
    alt: 'Table grid for shop.orders with 18,500 rows, row checkboxes, search and Export',
  },
  {
    name: 'Schema diagrams',
    keys: [],
    line: 'Tables and foreign keys drawn as a diagram, or read as DBML. Save it as .dbml or .svg.',
    shot: 'schema-diagram',
    alt: 'Schema diagram of customers, orders, order_items, products and reviews',
  },
  {
    name: 'Code editor',
    keys: ['⇧', '⌘', 'I'],
    line: 'About 200 languages, and Prettier, rustfmt, gofmt or Ruff on one shortcut.',
    shot: 'markdown-preview',
    alt: 'Markdown file next to its rendered preview',
  },
  {
    name: 'Terminal',
    keys: ['⌘', 'J'],
    line: 'Full alacritty emulation in a dock under your code, with split panes.',
    shot: 'editor-terminal',
    alt: 'Code editor with a TSX file and a terminal running psql underneath',
  },
  {
    name: 'Listening ports',
    keys: [],
    line: 'Every dev server you started, the project it belongs to, and a button to stop it.',
    shot: 'ports',
    alt: 'List of listening ports with their processes and projects',
  },
  {
    name: 'Git',
    keys: [],
    line: 'Status, diff, stage and commit, and a graph of the branches.',
    shot: 'git',
    alt: 'Git panel with changed files and a commit graph',
  },
  {
    name: 'Browser for agents',
    keys: [],
    line: 'A tab per project on your dev server, with Web Inspector, that AI agents drive over MCP.',
    shot: 'browser',
    alt: 'Browser tab showing a local dev server inside Sawit',
  },
] as const

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

export function Features() {
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
