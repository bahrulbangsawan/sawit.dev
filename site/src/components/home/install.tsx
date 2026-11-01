// Install Sawit: the script and the disk image, side by side.
import { InstallButton } from '../interactive'
import { DMG } from '../../content'

export function Install() {
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
