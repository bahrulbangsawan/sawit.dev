import { createFileRoute } from '@tanstack/react-router'
import { Page } from '../components/chrome'
import { Binary } from '../components/home/binary'
import { Cta } from '../components/home/cta'
import { Faq } from '../components/home/faq'
import { Features } from '../components/home/features'
import { Hero } from '../components/home/hero'
import { Install } from '../components/home/install'
import { Menu } from '../components/home/menu'
import { Postgres } from '../components/home/postgres'
import { Stage } from '../components/home/stage'
import { Trio } from '../components/home/trio'
import { SITE } from '../content'

export const Route = createFileRoute('/')({
  head: () => ({
    meta: [{ property: 'og:url', content: `${SITE}/` }],
    // www.sawit.dev serves the same pages: search engines keep the apex.
    links: [{ rel: 'canonical', href: `${SITE}/` }],
  }),
  component: Home,
})

function Home() {
  return (
    <Page home>
      <Hero />
      <Trio />
      <Stage />
      <Postgres />
      <Features />
      <Binary />
      <Menu />
      <Install />
      <Faq />
      <Cta />
    </Page>
  )
}
