// Deliberately fake, offline bridge. Never shipped inside package/.
(() => {
  const query = new URLSearchParams(location.search)
  const theme = query.get('theme') === 'dark' ? 'dark' : 'light'
  const palette = theme === 'dark' ? ['#07080a', '#0d0d0d', '#f4f4f6', '#9c9c9d', '#242728', '#57c1ff', '#ff6161'] : ['#f7f7f8', '#ffffff', '#171719', '#6f6f74', '#d9d9dc', '#1f8dd6', '#c62828']
  ;['background', 'surface', 'text', 'text-muted', 'border', 'accent', 'danger'].forEach((name, index) => document.documentElement.style.setProperty('--uipilot-color-' + name, palette[index]))
  let onUpdate, onReady, requestId = 0
  const update = text => {
    const id = String(++requestId)
    onUpdate({ sessionEpoch: '1', requestId: id, input: text, theme, data: { text, preferences: { sourceLanguage: 'auto', targetLanguage: 'zh-CHS', providerId: 'youdao' } } })
    queueMicrotask(() => onReady?.({ sessionEpoch: '1', requestId: id }))
  }
  window.uipilotPluginPanel = {
    onReady(fn) { onReady = fn; return () => { onReady = null } },
    onUpdate(fn) { onUpdate = fn; queueMicrotask(() => update(query.get('state') === 'empty' ? '' : query.get('state') === 'long' ? 'A'.repeat(120) + '\nLong multiline text\n第二段内容' : 'hello')); return () => { onUpdate = null } },
    onActions() { return () => {} }, onHostKey() { return () => {} },
    async request({ payload }) {
      if (query.get('state') === 'loading') return new Promise(() => {})
      await new Promise(resolve => setTimeout(resolve, 350))
      if (query.get('state') === 'error') return { ok: false, code: 'SERVICE_UNAVAILABLE', message: '翻译服务暂不可用，请重新提交' }
      return { ok: true, text: payload.targetLanguage === 'en' ? 'Hello\nAn offline sample translation.' : query.get('state') === 'long' ? '长文本译文\n' + 'LongUnbrokenWord'.repeat(24) : '你好', detectedSourceLanguage: 'en', targetLanguage: payload.targetLanguage }
    },
    storage: { async set() {} },
    async setStatus({ message }) { parent.postMessage({ type: 'host-status', message }, location.origin) },
  }
  window.addEventListener('message', event => { if (event.origin === location.origin && event.data?.type === 'host-input') update(event.data.text) })
})()
