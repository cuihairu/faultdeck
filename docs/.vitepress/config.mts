import { defineConfig } from 'vitepress'

export default defineConfig({
  base: '/faultdeck/',
  title: 'FaultDeck',
  description: 'Game-dev-oriented fault simulation & testing tool',
  themeConfig: {
    nav: [
      { text: 'English', link: '/' },
      { text: '中文', link: '/zh/' },
    ],
    sidebar: {
      '/': [
        {
          text: 'Design',
          items: [
            { text: 'MVP Plan', link: '/design/mvp-plan' },
          ],
        },
      ],
      '/zh/': [
        {
          text: '设计',
          items: [
            { text: 'MVP 计划', link: '/zh/design/mvp-plan' },
          ],
        },
      ],
    },
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
        sidebar: {
          '/zh/': [
            {
              text: '设计',
              items: [
                { text: 'MVP 计划', link: '/zh/design/mvp-plan' },
              ],
            },
          ],
        },
      },
    },
  },
})
