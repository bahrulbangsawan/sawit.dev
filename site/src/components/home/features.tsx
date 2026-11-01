// One window, every tool: six feature cells with screenshots, the rest as expanders.
import { REPO, features } from '../../content'

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
