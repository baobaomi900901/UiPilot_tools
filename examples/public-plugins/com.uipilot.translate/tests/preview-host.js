// Deliberately fake, offline bridge. Never shipped inside package/.
(() => {
  const query = new URLSearchParams(location.search)
  const theme = query.get('theme') === 'dark' ? 'dark' : 'light'
  const palette = theme === 'dark' ? ['#07080a', '#0d0d0d', '#f4f4f6', '#9c9c9d', '#242728', '#57c1ff', '#ff6161'] : ['#f7f7f8', '#ffffff', '#171719', '#6f6f74', '#d9d9dc', '#1f8dd6', '#c62828']
  ;['background', 'surface', 'text', 'text-muted', 'border', 'accent', 'danger'].forEach((name, index) => document.documentElement.style.setProperty('--uipilot-color-' + name, palette[index]))
  let onUpdate, onReady, requestId = 0, onSpeechState, playbackId = 0, speechTimer, speechState = { playbackId: null, status: 'idle', language: null }
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
    // Fake effects only: no OS clipboard or audio. Status is visible in preview shell.
    clipboard: { async writeText({text}) { parent.postMessage({type:'host-status',message:`预览复制 ${text.length} 个字符（未写入系统剪贴板）`},location.origin) } },
    speech: {
      async voices() { return [{id:'fake-en',name:'Preview English',language:'en'},{id:'fake-zh',name:'Preview Chinese',language:'zh-CHS'}] },
      onState(fn) { onSpeechState=fn;return()=>{if(onSpeechState===fn)onSpeechState=null} },
      async state() { return speechState },
      async play({language}) {
        clearTimeout(speechTimer)
        speechState={playbackId:String(++playbackId),status:'playing',language}
        const id=speechState.playbackId
        speechTimer=setTimeout(()=>{if(speechState.playbackId===id){speechState={...speechState,status:'completed'};onSpeechState?.(speechState)}},5000)
        return speechState
      },
      async stop({playbackId:id}) { if(speechState.playbackId===id){clearTimeout(speechTimer);speechState={...speechState,status:'stopped'};onSpeechState?.(speechState)}return speechState },
    },
    storage: { async set() {} },
    async setStatus({ message }) { parent.postMessage({ type: 'host-status', message }, location.origin) },
  }
  window.addEventListener('message', event => { if (event.origin === location.origin && event.data?.type === 'host-input') update(event.data.text) })
})()
