// Three cells under the hero: no Docker, drawn by the GPU, agents welcome.

export function Trio() {
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
            <code>sawit-mcp</code> gives AI agents 133 tools. Destructive ones ask for the name again.
          </p>
        </div>
      </div>
    </section>
  )
}
