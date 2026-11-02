import { defineConfig } from 'vite'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'
import { cloudflare } from '@cloudflare/vite-plugin'

export default defineConfig({
  // cf dev forwards only --mode, so the port lives here
  server: { port: 4318 },
  plugins: [
    tanstackStart({
      // Route code splitting, through Start's own option: Start adds the router's code splitter
      // itself (there is no autoCodeSplitting to set, and no second router plugin). These are the
      // router's default groupings, named so that a new default can't move them: each route's
      // component, errorComponent and notFoundComponent load on demand; loaders stay eager.
      router: {
        codeSplittingOptions: {
          defaultBehavior: [['component'], ['errorComponent'], ['notFoundComponent']],
        },
      },
    }),
    viteReact(),
    cloudflare({ viteEnvironment: { name: 'ssr' } }),
  ],
})
