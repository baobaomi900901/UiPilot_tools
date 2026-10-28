import { createRoot } from 'react-dom/client'
import { createPluginWindowCore } from './plugin-window-core'
import { PluginWindowView } from './plugin-window-view'

const parameters = new URLSearchParams(window.location.search)
const scheme = parameters.get('theme') === 'light' ? 'light' : 'dark'
document.documentElement.dataset.colorScheme = scheme
const core = createPluginWindowCore({
  getIdentity: async () => ({ name: 'JSON 工作台', iconUrl: null }),
  setPinned: async ({ pinned }) => ({ pinned }),
  close: async () => { document.title = 'JSON 工作台 · 已关闭（预览）' },
})
const host = document.querySelector<HTMLElement>('#app')!
const root = createRoot(host)
root.render(
  <div style={{ display: 'grid', gridTemplateRows: '44px minmax(0, 1fr)', height: '100%' }}>
    <PluginWindowView core={core} />
    <iframe
      title="JSON 工作台"
      src={`/dev/plugin-window-content-preview.html?theme=${scheme}`}
      style={{ width: '100%', height: '100%', border: 0, display: 'block' }}
    />
  </div>,
)
void core.start()
window.addEventListener('pagehide', () => { core.destroy(); root.unmount() }, { once: true })
