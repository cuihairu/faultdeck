import DefaultTheme from 'vitepress/theme'
import './custom.css'

// Remember a manual language pick made through the nav language links
// ("English" / "中文"). The head bootstrap script in config.mts redirects
// first-time visitors to their browser locale; without this capture a visitor
// switching back through the nav would be bounced to the stored/detected
// locale on the next full page load (locale-switch loop). Runs in the capture
// phase so the choice is stored before the router follows the link. The
// window guard keeps the static build (Node) from touching document.
const LANG_KEY = 'faultdeck-lang'
const BASE = '/faultdeck/' // keep in sync with config.mts `base`

if (typeof window !== 'undefined') {
  document.addEventListener(
    'click',
    (e) => {
      const target = e.target instanceof Element ? e.target : null
      const anchor = target?.closest('a')
      if (!anchor) return
      const text = (anchor.textContent || '').trim()
      const href = anchor.getAttribute('href') || ''
      const rel = href.startsWith(BASE) ? href.slice(BASE.length) : href
      if (text === 'English' && (rel === '' || rel === '/')) {
        localStorage.setItem(LANG_KEY, 'en')
      } else if (text === '中文' && (rel === 'zh/' || rel === '/zh/')) {
        localStorage.setItem(LANG_KEY, 'zh')
      }
    },
    true,
  )
}

export default DefaultTheme
