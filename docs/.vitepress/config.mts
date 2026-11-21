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

// First-visit language bootstrap: sends a browser-locale-matching visitor to
// the matching site locale (todo 2026-11-14 G2, implemented 2026-11-21).
// Only pages that exist in BOTH locales are ever redirected — the whitelist
// below is the en/zh mirror set, so the redirect can never land on a 404 for
// en-only internal docs (design/desktop-ui, review/*, 审计-文档一致性).
// A manual pick via the nav language links is stored by theme/index.ts and
// always wins over detection; the stored key also makes the redirect
// one-shot, so there is no bounce loop.
const LANG_BOOTSTRAP = `(function () {
  try {
    var BASE = '/faultdeck/';
    var KEY = 'faultdeck-lang';
    var MIRROR = ['', 'quickstart', 'architecture', 'configuration', 'glossary', 'netem-faults', 'remote-agents', 'reports', 'design/mvp-plan'];
    var rel = location.pathname;
    if (rel.indexOf(BASE) === 0) rel = rel.slice(BASE.length);
    rel = rel.replace(/\\.html$/, '').replace(/\\/$/, '');
    var inZh = rel === 'zh' || rel.indexOf('zh/') === 0;
    var rest = inZh ? rel.slice(3) : rel;
    if (MIRROR.indexOf(rest) === -1) return;
    var want = localStorage.getItem(KEY);
    if (want !== 'zh' && want !== 'en') {
      var langs = navigator.languages || [navigator.language || ''];
      want = '';
      for (var i = 0; i < langs.length; i++) {
        var l = String(langs[i] || '').toLowerCase();
        if (l.indexOf('zh') === 0) { want = 'zh'; break; }
        if (l.indexOf('en') === 0) { want = 'en'; break; }
      }
      if (!want) return;
      localStorage.setItem(KEY, want);
    }
    var target = BASE + (want === 'zh' ? 'zh/' : '') + (rest ? rest + '/' : '');
    if (target !== location.pathname) location.replace(target + location.search + location.hash);
  } catch (e) {}
})();`

export default defineConfig({
  base: '/faultdeck/',
  title: 'FaultDeck',
  description: 'Game-dev-oriented fault simulation & testing tool',
  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/faultdeck/favicon.svg' }],
    ['script', {}, LANG_BOOTSTRAP],
  ],
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
