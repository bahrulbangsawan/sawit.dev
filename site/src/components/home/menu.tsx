// The command line section: every sawit command, served as a menu on a letter.

// The CLI, set as the plantation's menu. Commands from `sawit --help`.
const menu = [
  {
    group: 'Kebun',
    gloss: 'the estate',
    items: [
      { cmd: 'sawit kalimantan', means: 'tables', note: 'Lists the tables of a database' },
      { cmd: 'sawit kalimantan hutan', means: 'rows', note: 'The first 100 rows, like psql' },
      { cmd: 'sawit kalimantan hutan "izin is null"', means: 'where', note: 'The text becomes the WHERE clause' },
      { cmd: 'sawit list', means: 'everything', note: 'Instances, databases, ports and connections' },
      { cmd: 'sawit browse', means: 'browser', note: 'The dev server of this project, a port or a URL, in the browser tab' },
    ],
  },
  {
    group: 'Panen',
    gloss: 'the harvest',
    items: [
      { cmd: 'sawit panen kalimantan hutan hutan.xlsx', means: 'export', note: 'A table or a query, to CSV or Excel' },
      { cmd: 'sawit backup kalimantan kalimantan.dump', means: 'pg_dump', note: 'The database, one schema or one table' },
      { cmd: 'sawit tanam kalimantan konsesi konsesi.csv', means: 'import', note: 'Plants a CSV in a table, all rows or none' },
    ],
  },
  {
    group: 'Bakar',
    gloss: 'the burning',
    items: [
      { cmd: 'sawit bakar ports', means: 'stop', note: 'Dev servers only: asks, then SIGTERM, then SIGKILL' },
      { cmd: 'sawit bakar ports 3000 -y', means: 'stop :3000', note: 'Whatever listens there, without asking' },
      { cmd: 'sawit bakar instances', means: 'pg_ctl stop', note: 'Running instances; the data is kept' },
      { cmd: 'sawit nyalakan', means: 'pg_ctl start', note: 'Stopped instances; a port already taken is explained, never forced' },
    ],
  },
  {
    group: 'Pupuk',
    gloss: 'the fertiliser',
    items: [
      { cmd: 'sawit list tools', means: 'tools', note: 'Package managers, runtimes and AI agents' },
      { cmd: 'sawit pasang bun uv', means: 'install', note: 'Official install scripts, prerequisites first' },
      { cmd: 'sawit pupuk', means: 'upgrade', note: 'Every tool with a newer release' },
      { cmd: 'sawit cabut bun', means: 'uninstall', note: 'Only the copy its installer, Homebrew or npm put there' },
    ],
  },
]

export function Menu() {
  return (
    <section className="sec gr" id="cli" aria-labelledby="menu-title">
      <div className="w pad">
        <div className="letter">
          <div className="sheet">
            <img className="seal" src="/favicon.svg" width={96} height={96} alt="" />
            <p className="eyebrow">Served in any terminal</p>
            <h2 id="menu-title">The sawit menu</h2>
            <p className="sheet-lead">Every command works with or without the app open.</p>
            <div className="groups">
              {menu.map((g) => (
                <div key={g.group}>
                  <h3>
                    <span lang="id">{g.group}</span> · {g.gloss}
                  </h3>
                  <ul>
                    {g.items.map((it) => (
                      <li key={it.cmd}>
                        <code>{it.cmd}</code>
                        <span>{it.note}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </div>
            <hr />
            <p className="sign">
              <span lang="id">“Bakar 2 lahan di atas? Asapnya tanggung sendiri.”</span>
              <small>Burn the two plots above? The smoke is on you.</small>
            </p>
          </div>
        </div>
      </div>
    </section>
  )
}
