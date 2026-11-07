// One window, every tool: six feature cells with screenshots, the rest as expanders (a screenshot where there is one).
import { MEDIA, REPO } from '../../content'

type Feature = { name: string; keys: readonly string[]; line: string; shot?: string; alt?: string }

// Capabilities, shortcuts and wording from the README; screenshots from docs/media, served from R2.
const features: Feature[] = [
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
    line: 'Full alacritty emulation in a dock under your code, with split panes, tabs you lock or pin, and Copy Last Output or Copy Error in one step.',
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
    line: 'A tab per project, with tabs of its own, the dev servers running a click away, a loading bar, true-size device previews and Web Inspector. AI agents drive it over MCP.',
    shot: 'browser',
    alt: 'Browser tab showing a local dev server inside Sawit',
  },
  {
    name: 'Find anywhere',
    keys: ['⌘', 'F'],
    line: 'One find bar in code, Markdown previews, PDFs, the browser’s page, the canvas and the terminal’s scrollback, with a match count, Match Case, Whole Word and Regex.',
  },
  {
    name: 'Full screen',
    keys: ['⇧', '⌘', '↩'],
    line: 'The terminal, the browser or the canvas fills the window with one key, and the layout comes back as it was. ⌃⌘1, ⌃⌘2 and ⌃⌘3 go straight to each.',
  },
  {
    name: 'Canvas to code',
    keys: [],
    line: 'Whiteboards and designs in tabs, with prototypes, Dev Mode’s Inspect, variables and comments. Export an artboard as HTML or React, or send it to your coding agent, and watch agents build on it live, each with its own cursor.',
  },
  {
    name: 'Agent Accounts',
    keys: [],
    line: 'Several Claude Code accounts side by side, with their sign-in, 5-hour and 7-day usage, sessions and shared settings. Switch with cp1–cp9 or sawit account. It takes over from Claude Pool, and imports what Claude Pool had.',
  },
  {
    name: 'Permissions',
    keys: [],
    line: 'See and change what your agents may do on this Mac: Claude Code’s rules in every settings file, Sawit’s confirm tools and the macOS permissions. Each change shows its diff and keeps a backup.',
  },
  {
    name: 'GitHub',
    keys: [],
    line: 'One sign-in that survives restarts. Back up the prompt library to a private repository, and keep ~/.zshrc and the other shell files in sync both ways.',
  },
  {
    name: 'Asks before it destroys',
    keys: [],
    line: 'Every action that deletes, stops or discards asks first, in one dialog that names the thing, with Cancel as the default. Safe Close All keeps pinned and unsaved tabs; Force Close All names what it discards.',
  },
  {
    name: 'Dev tools and skills',
    keys: [],
    line: 'Install the GitHub CLI, MinIO, Herdr, HyperFrames, voxl and more with their official scripts, and the shadcn/ui and Design to Code skills into every agent on your Mac.',
  },
]

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
            <p className="lead">Postgres, an editor, a terminal, git, a browser and a canvas, side by side, in Zed’s One Dark colours.</p>
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
                <img src={`${MEDIA}/${f.shot}.jpg`} width={1600} height={1001} alt={f.alt} loading="lazy" />
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
              <summary>
                <span>
                  {f.name}
                  <Keys keys={f.keys} />
                </span>
              </summary>
              <div className={f.shot ? 'acc-body' : 'acc-body text'}>
                <p>{f.line}</p>
                {f.shot && (
                  <div className="frame">
                    <img src={`${MEDIA}/${f.shot}.jpg`} width={1600} height={1001} alt={f.alt} loading="lazy" />
                  </div>
                )}
              </div>
            </details>
          ))}
        </div>
      </div>
    </section>
  )
}
