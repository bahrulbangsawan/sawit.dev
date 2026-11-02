// One binary: the demo video and four numbers.
import { DemoVideo } from '../interactive'
import { SIZE } from '../../content'

export function Binary() {
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
            <dd>141</dd>
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
