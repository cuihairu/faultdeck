import { defineConfig } from 'vitepress'

const enSidebar = [
  {
    text: 'Getting Started',
    items: [
      { text: 'Quickstart', link: '/quickstart' },
    ],
  },
  {
    text: 'Guide',
    items: [
      { text: 'Architecture', link: '/architecture' },
      { text: 'Netem Faults', link: '/netem-faults' },
      { text: 'Agents', link: '/remote-agents' },
      { text: 'Reports', link: '/reports' },
      { text: 'Configuration', link: '/configuration' },
      { text: 'Glossary', link: '/glossary' },
    ],
  },
  {
    text: 'Design',
    items: [
      { text: 'MVP Plan', link: '/design/mvp-plan' },
    ],
  },
]

const zhSidebar = [
  {
    text: '上手',
    items: [
      { text: '快速开始', link: '/zh/quickstart' },
    ],
  },
  {
    text: '指南',
    items: [
      { text: '架构', link: '/zh/architecture' },
      { text: 'Netem 故障', link: '/zh/netem-faults' },
      { text: 'Agent 与注册', link: '/zh/remote-agents' },
      { text: '场景报告', link: '/zh/reports' },
      { text: '配置', link: '/zh/configuration' },
      { text: '术语表', link: '/zh/glossary' },
    ],
  },
  {
    text: '设计',
    items: [
      { text: 'MVP 计划', link: '/zh/design/mvp-plan' },
    ],
  },
]

export default defineConfig({
  base: '/faultdeck/',
  title: 'FaultDeck',
  description: 'Game-dev-oriented fault simulation & testing tool',
  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: '/faultdeck/favicon.svg' }]],
  themeConfig: {
    nav: [
      { text: 'English', link: '/' },
      { text: '中文', link: '/zh/' },
    ],
    sidebar: enSidebar,
    footer: {
      message: 'Released under the Apache-2.0 License.',
      copyright: 'FaultDeck Contributors',
    },
  },
  locales: {
    root: {
      label: 'English',
      lang: 'en',
    },
    zh: {
      label: '中文',
      lang: 'zh-CN',
      themeConfig: {
        nav: [
          { text: 'English', link: '/' },
          { text: '中文', link: '/zh/' },
        ],
        sidebar: zhSidebar,
      },
    },
  },
})
