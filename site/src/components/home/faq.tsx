// Good to know: the questions people ask first.
import { faq } from '../../content'

export function Faq() {
  return (
    <section className="sec" id="faq" aria-labelledby="faq-title">
      <div className="w pad">
        <h2 id="faq-title" className="h2 c">
          Good to know
        </h2>
        <div className="faq">
          {faq.map((item) => (
            <div key={item.q}>
              <h3>{item.q}</h3>
              <p>{item.a}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
