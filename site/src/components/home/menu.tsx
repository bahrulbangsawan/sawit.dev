// The command line, set as the plantation's menu.
import { menu } from '../../content'

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
