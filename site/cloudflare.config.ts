import { defineConfig } from 'cf/config'

export default defineConfig({
  // bisa.digital, the account that holds the sawit.dev zone
  accountId: '79831d843efec46bd23d872d5f8052cb',
  worker: {
    name: 'sawit-site',
    entrypoint: '@tanstack/react-start/server-entry',
    compatibilityDate: '2026-10-06',
    domains: ['sawit.dev', 'www.sawit.dev'],
    observability: { enabled: true },
  },
})
