import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'Kronroe',
  description: 'Embedded bi-temporal graph database — documentation',
  base: '/docs/',

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/docs/favicon.svg' }],
    ['meta', { name: 'theme-color', content: '#2A1D12' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'Kronroe Docs' }],
  ],

  themeConfig: {
    logo: '/logo.svg',
    siteTitle: 'Kronroe',

    nav: [
      { text: 'Guide', link: '/getting-started/what-is-kronroe' },
      { text: 'API', link: '/api/core' },
      { text: 'kronroe.dev', link: 'https://kronroe.dev' },
      { text: 'GitHub', link: 'https://github.com/kronroe/kronroe' },
    ],

    sidebar: [
      {
        text: 'Getting Started',
        items: [
          { text: 'What is Kronroe?', link: '/getting-started/what-is-kronroe' },
          { text: 'Quick Start: Python', link: '/getting-started/quick-start-python' },
          { text: 'Quick Start: Rust', link: '/getting-started/quick-start-rust' },
          { text: 'Quick Start: MCP', link: '/getting-started/quick-start-mcp' },
        ],
      },
      {
        text: 'Core Concepts',
        items: [
          { text: 'Bi-Temporal Model', link: '/concepts/bi-temporal-model' },
          { text: 'Facts and Entities', link: '/concepts/facts-and-entities' },
        ],
      },
      {
        text: 'API Reference',
        items: [
          { text: 'TemporalGraph (Core)', link: '/api/core' },
          { text: 'AgentMemory', link: '/api/agent-memory' },
          { text: 'MCP Tools', link: '/api/mcp-tools' },
        ],
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/kronroe/kronroe' },
      { icon: 'linkedin', link: 'https://www.linkedin.com/company/kronroe' },
    ],

    footer: {
      message: 'Dual-licensed under AGPL-3.0 and commercial terms.',
      copyright: 'Kronroe — Rebekah Cole',
    },

    search: {
      provider: 'local',
    },

    editLink: {
      pattern: 'https://github.com/kronroe/kronroe/edit/main/site/docs/:path',
      text: 'Edit this page on GitHub',
    },
  },
})
