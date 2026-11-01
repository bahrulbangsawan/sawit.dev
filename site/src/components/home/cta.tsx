// The closing call to install: the app icon, the line in Indonesian and the buttons again.
import { InstallButton } from '../interactive'
import { REPO } from '../../content'

export function Cta() {
  return (
    <section className="sec cta" aria-labelledby="cta-title">
      <div className="w">
        <span className="icon">
          {/* lazy, or React preloads it in <head>, ahead of the stylesheets, on every visit */}
          <img src="/media/sawit-icon.png" width={512} height={512} alt="The Sawit app icon: a palm tree whose trunk is three database discs" loading="lazy" />
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
