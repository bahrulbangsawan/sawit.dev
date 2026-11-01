import type { ReactNode } from 'react'
import { HeadContent, Outlet, Scripts, createRootRoute } from '@tanstack/react-router'
import appCss from '../styles.css?url'
import { Page } from '../components/chrome'
import { DESCRIPTION, SITE } from '../content'

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { title: 'Sawit — one native app for local development' },
      { name: 'description', content: DESCRIPTION },
      { name: 'theme-color', content: '#1b1d22' },
      // Link previews. Crawlers want absolute URLs; each page adds its own og:url and canonical
      // link, and a page's own og:title or og:description replaces these.
      { property: 'og:type', content: 'website' },
      { property: 'og:site_name', content: 'Sawit' },
      { property: 'og:title', content: 'Sawit' },
      { property: 'og:description', content: DESCRIPTION },
      { property: 'og:image', content: `${SITE}/media/welcome.jpg` },
      { property: 'og:image:width', content: '1600' },
      { property: 'og:image:height', content: '1001' },
      { property: 'og:image:alt', content: 'Sawit’s welcome screen: start actions, recent folders, PostgreSQL instances and database connections' },
      { name: 'twitter:card', content: 'summary_large_image' },
    ],
    links: [
      { rel: 'icon', href: '/favicon.svg', type: 'image/svg+xml' },
      { rel: 'preconnect', href: 'https://fonts.googleapis.com' },
      { rel: 'preconnect', href: 'https://fonts.gstatic.com', crossOrigin: 'anonymous' },
      { rel: 'stylesheet', href: 'https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Serif:ital,wght@0,300;0,400;1,300;1,400&display=swap' },
      { rel: 'stylesheet', href: appCss },
    ],
  }),
  component: () => (
    <RootDocument>
      <Outlet />
    </RootDocument>
  ),
  // An address with no page still gets the site around its message, with the way back.
  notFoundComponent: NotFound,
})

function NotFound() {
  return (
    <Page>
      <header className="sec hero log-hero" id="top">
        <div className="w">
          <h1>Page not found</h1>
          <p className="lead">There’s no page at this address.</p>
          <div className="acts">
            <a className="btn pri" href="/">
              Go to the home page
            </a>
            <a className="btn" href="/changelog">
              Read the changelog
            </a>
          </div>
        </div>
      </header>
    </Page>
  )
}

function RootDocument({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <html lang="en">
      <head>
        <HeadContent />
      </head>
      <body>
        {children}
        <Scripts />
      </body>
    </html>
  )
}
