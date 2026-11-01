// The top of the home page: the news bar, the headline, the two ways to install and the command.
import { COMMAND_ID, InstallButton } from '../interactive'
import { DMG, INSTALL, SIZE } from '../../content'

export function Hero() {
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
          <h1>One native app for local development.</h1>
          <p className="lead">
            Databases, code, terminals, a browser and a canvas in one window. Your AI agents can use it too.
          </p>
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
              <span id={COMMAND_ID}>{INSTALL}</span>
            </code>
          </pre>
          <small>Apple Silicon · macOS · one {SIZE} binary</small>
        </div>
      </header>
    </>
  )
}
