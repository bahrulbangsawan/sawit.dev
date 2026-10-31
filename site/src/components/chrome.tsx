import type { ReactNode } from 'react'
import { useShortcuts } from './interactive'
import { DMG, REPO } from '../content'

// The page shell every route shares: icon sprite, skip link, nav, main and the blue footer.
export function Page({ home = false, children }: { home?: boolean; children: ReactNode }) {
  useShortcuts()
  return (
    <div className="flavor">
      <Sprite />
      <a className="skip" href="#main">
        Skip to content
      </a>
      <Nav home={home} />
      <main id="main">{children}</main>
      <Footer home={home} />
    </div>
  )
}

function Sprite() {
  return (
    <svg className="sprite" aria-hidden="true">
      <symbol id="i-down" viewBox="0 0 16 16">
        <path d="M8 2v8M4.5 6.5 8 10l3.5-3.5M2.5 11v2.5h11V11" />
      </symbol>
      <symbol id="i-right" viewBox="0 0 16 16">
        <path d="M6 4l4 4-4 4" />
      </symbol>
      <symbol id="i-copy" viewBox="0 0 16 16">
        <rect x="5.5" y="5.5" width="8" height="8" rx="1" />
        <path d="M10.5 3.5v-1h-8v8h1" />
      </symbol>
      <symbol id="i-check" viewBox="0 0 16 16">
        <path d="M3 8.5 6.5 12 13 4.5" />
      </symbol>
      <symbol id="i-pause" viewBox="0 0 10 10">
        <path d="M2 1h2v8H2zM6 1h2v8H6z" />
      </symbol>
      <symbol id="i-play" viewBox="0 0 10 10">
        <path d="M1.5 1v8l7-4z" />
      </symbol>
      <symbol id="i-git" viewBox="0 0 16 16">
        <circle cx="4.5" cy="3.5" r="1.5" />
        <circle cx="4.5" cy="12.5" r="1.5" />
        <circle cx="11.5" cy="5.5" r="1.5" />
        <path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" />
      </symbol>
    </svg>
  )
}

function Nav({ home }: { home: boolean }) {
  const base = home ? '' : '/'
  return (
    <nav className="nav" aria-label="Main">
      <div className="w">
        <a className="mark" href={home ? '#top' : '/'}>
          <img src="/favicon.svg" width={24} height={24} alt="" />
          Sawit
        </a>
        <div className="links">
          <a href={`${base}#features`}>Features</a>
          <a href={`${base}#cli`}>CLI</a>
          <a href={`${base}#install`}>Install</a>
          <a href={`${base}#faq`}>FAQ</a>
          <i aria-hidden="true" />
          <a href="/changelog" aria-current={home ? undefined : 'page'}>
            Changelog
          </a>
        </div>
        <div className="right">
          <a className="btn sm k ghost hide-sm" href={REPO} aria-keyshortcuts="g">
            GitHub<kbd aria-hidden="true">G</kbd>
          </a>
          <a className="btn sm k pri" href={DMG} aria-keyshortcuts="d">
            Download<kbd aria-hidden="true">D</kbd>
          </a>
        </div>
      </div>
    </nav>
  )
}

function Footer({ home }: { home: boolean }) {
  const base = home ? '' : '/'
  return (
    <footer>
      <div className="w">
        <div className="fc">
          <div>
            <h2>Sawit</h2>
            <p>A native macOS workbench for local Postgres. Named after the oil-palm plantations; its command line jokes about their fires.</p>
            <div className="fine">
              <span>Screenshots and video recorded from a release build with demo data.</span>
              <span>Built with Rust and GPUI.</span>
            </div>
          </div>
          <div>
            <h2>Product</h2>
            <ul>
              <li><a href={`${base}#features`}>Features</a></li>
              <li><a href={`${base}#cli`}>Command line</a></li>
              <li><a href={`${base}#install`}>Install</a></li>
              <li><a href={`${base}#faq`}>FAQ</a></li>
            </ul>
          </div>
          <div>
            <h2>Releases</h2>
            <ul>
              <li><a href={`${REPO}/releases`}>Releases ↗</a></li>
              <li><a href="/changelog">Changelog</a></li>
              <li><a href={`${REPO}#readme`}>README ↗</a></li>
            </ul>
          </div>
          <div>
            <h2>Source</h2>
            <ul>
              <li><a href={REPO}>GitHub ↗</a></li>
              <li><a href={`${REPO}/issues`}>Issues ↗</a></li>
              <li><a href={`${REPO}#mcp-for-ai-agents`}>MCP for agents ↗</a></li>
              <li><a href="#top">Back to top</a></li>
            </ul>
          </div>
        </div>
        <div className="big" aria-hidden="true">
          <svg viewBox="0 0 1120 220" preserveAspectRatio="xMidYMin slice">
            <text x="40" y="200">
              Sawit
            </text>
          </svg>
        </div>
      </div>
    </footer>
  )
}
