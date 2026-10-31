import { defineConfig } from 'vite'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'
import { cloudflare } from '@cloudflare/vite-plugin'

export default defineConfig({
  // cf dev forwards only --mode, so the port lives here
  server: { port: 4318 },
  plugins: [tanstackStart(), viteReact(), cloudflare({ viteEnvironment: { name: 'ssr' } })],
})
