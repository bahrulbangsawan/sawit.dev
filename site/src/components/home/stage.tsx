// The stage under the trio: the query and rows the drawn window shows, and the window itself.

const sql: [string, string][][] = [
  [['cm', '-- Revenue by status, last 90 days']],
  [['kw', 'select'], ['', ' status,']],
  [['', '       '], ['fn', 'count'], ['', '(*)          '], ['kw', 'as'], ['', ' orders,']],
  [['', '       '], ['fn', 'sum'], ['', '(total)        '], ['kw', 'as'], ['', ' revenue,']],
  [['', '       '], ['fn', 'round'], ['', '('], ['fn', 'avg'], ['', '(total), '], ['nu', '2'], ['', ') '], ['kw', 'as'], ['', ' avg_order']],
  [['kw', 'from'], ['', ' shop.orders']],
  [['kw', 'where'], ['', ' placed_at > '], ['fn', 'now'], ['', '() - interval '], ['st', "'90 days'"]],
  [['kw', 'group by'], ['', ' status']],
  [['kw', 'order by'], ['', ' revenue '], ['kw', 'desc'], ['', ';']],
]

const results = [
  ['paid', '1359', '173743.68', '127.85'],
  ['delivered', '1344', '170494.89', '126.86'],
  ['shipped', '955', '123632.86', '129.46'],
  ['pending', '510', '65960.56', '129.33'],
  ['refunded', '473', '58885.36', '124.49'],
]

// The product drawn in HTML: instances and tables, a terminal, and the SQL editor with its result.
export function Stage() {
  return (
    <section className="sec stage" aria-label="Sawit, drawn">
      <div className="w">
        <a className="demo" href="#demo">
          <svg aria-hidden="true">
            <use href="#i-play" />
          </svg>
          Watch the demo
        </a>
        <div className="ide" aria-hidden="true">
          <div className="side">
            <div className="bar">
              <span className="dots">
                <i />
                <i />
                <i />
              </span>
              <span className="fg">Sawit</span>
              <span>acme-store</span>
            </div>
            <p className="grp">Instances</p>
            <div className="it on">
              <i />
              <b>shop</b>
              <span>PostgreSQL 16 · :55432</span>
            </div>
            <div className="it">
              <i />
              <b>kalimantan</b>
              <span>PostgreSQL 17 · stopped</span>
            </div>
            <div className="it">
              <i />
              <b>billing</b>
              <span>PostgreSQL 18 · stopped</span>
            </div>
            <p className="grp">shop · tables</p>
            {['customers', 'orders', 'order_items', 'products', 'reviews'].map((t) => (
              <div key={t} className={`it tb ${t === 'orders' ? 'sel' : ''}`}>
                <i />
                <b>{t}</b>
              </div>
            ))}
            <p className="foot">
              <i /> 1/3 instances running
            </p>
          </div>
          <div className="term">
            <div className="bar">
              <span className="fg">zsh</span>
              <span>acme-store</span>
            </div>
            <pre>
              <span className="pr">acme-store %</span> sawit shop
              {'\n'} customers   order_items   orders
              {'\n'} products    reviews
              {'\n\n'}
              <span className="pr">acme-store %</span> sawit panen shop orders orders.xlsx
              {'\n'} <span className="st">✓</span> 18,500 rows → orders.xlsx
              {'\n\n'}
              <span className="pr">acme-store %</span> <span className="caret" />
            </pre>
          </div>
          <div className="code">
            <div className="tabs">
              <span>SQL 1 · shop</span>
              <span>shop.orders</span>
              <span>orders.tsx</span>
            </div>
            <p className="crumb">
              SQL 1 · shop (Sawit)<span className="run">▷ Run ⌘↩</span>
            </p>
            <div className="lines">
              {sql.map((line, n) => (
                <p key={n} className={n === 6 ? 'cur' : undefined}>
                  {line.map(([cls, text], k) =>
                    cls ? (
                      <span key={k} className={cls}>
                        {text}
                      </span>
                    ) : (
                      text
                    ),
                  )}
                </p>
              ))}
            </div>
            <p className="rmeta">4.4 ms · 5 rows</p>
            <table>
              <thead>
                <tr>
                  <th>status</th>
                  <th>orders</th>
                  <th>revenue</th>
                  <th>avg_order</th>
                </tr>
              </thead>
              <tbody>
                {results.map((r) => (
                  <tr key={r[0]}>
                    {r.map((c) => (
                      <td key={c}>{c}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </section>
  )
}
