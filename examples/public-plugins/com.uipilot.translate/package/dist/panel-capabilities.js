/** Session-owned effects. All system access goes through the captured public Panel SDK. */
export function createPanelCapabilities({ facades, isCurrent, notify, render, getState }) {
  let disposed = false, started = false, voices = null, voiceError = null, unsubscribe
  let active = null, serial = Promise.resolve(), copying = 0
  const alive = revision => !disposed && isCurrent(revision)
  const message = (error, kind) => error?.name === 'PermissionDenied' ? `${kind}权限已关闭，请在设置中授权` : error?.name === 'SpeechVoiceUnavailable' ? '没有对应语言的系统声音' : error?.name === 'ExpiredPanelSession' ? '面板会话已失效，请重新打开' : `${kind}失败，请重新操作`
  const language = (side, state) => side === 'translation' ? state.targetLanguage : state.sourceLanguage === 'auto' ? state.detectedSourceLanguage : state.sourceLanguage
  const text = (side, state) => side === 'source' ? state.text : state.translation
  const owns = op => active === op && alive(op.revision)
  function publish() { if (alive()) render() }
  function availability(kind, side, state) {
    if (!alive(state.revision)) return '面板会话已失效'
    if (side === 'translation' && state.status !== 'success') return '需要本次有效译文'
    if (!text(side, state)) return '没有可用文本'
    if (kind === 'copy') return typeof facades.clipboard?.writeText === 'function' ? null : '宿主未提供复制能力'
    if (typeof facades.speech?.play !== 'function') return '宿主未提供朗读能力'
    if (!language(side, state)) return '请指定源语言或等待本次识别完成'
    if (voiceError) return voiceError
    if (!voices) return '正在检查系统声音'
    return voices.some(voice => voice.language === language(side, state)) ? null : '没有对应语言的系统声音'
  }
  async function stop(op, operation) {
    if (!op?.id || op.stopped) return
    op.stopped = true
    try { await facades.speech.stop({ playbackId: op.id }) }
    catch (error) { if (alive(op.revision) && !active && operation === copying) notify(message(error, '停止朗读')) }
  }
  function invalidate() {
    const operation = ++copying
    const previous = active
    active = null
    // Serializing stop and play avoids SpeechBusy while a prior play reply is pending.
    if (previous?.id) serial = serial.then(() => stop(previous, operation))
    publish()
  }
  function onState(state) {
    const op = active
    if (!op || !owns(op) || !state.playbackId) return
    if (!op.id) {
      // An event can precede its play reply. Retain only terminal IDs for that pending operation.
      if (state.status !== 'playing' && op.terminal.size < 32) op.terminal.set(state.playbackId, state)
      return
    }
    if (op.id !== state.playbackId || state.status === 'playing') return
    active = null
    if (state.status === 'error') notify(message({name:state.errorCode}, '朗读'))
    publish()
  }
  function start() {
    if (started || !alive()) return
    started = true
    if (!facades.speech) return
    try { unsubscribe = facades.speech.onState(onState) }
    catch (error) { voiceError = message(error, '朗读'); publish(); return }
    Promise.resolve().then(() => {
      if (alive()) return facades.speech.voices()
    }).then(value => { if (alive()) { voices = Array.isArray(value) ? value : []; publish() } })
      .catch(error => { if (alive()) { voiceError = message(error, '朗读'); publish() } })
  }
  async function run(kind, side, state = getState()) {
    const reason = availability(kind, side, state)
    if (reason) return
    if (kind === 'copy') {
      const operation = ++copying, revision = state.revision
      try {
        await facades.clipboard.writeText({ text: text(side, state) })
        if (alive(revision) && operation === copying) notify('已复制', 'success')
      } catch (error) { if (alive(revision) && operation === copying) notify(message(error, '复制')) }
      return
    }
    if (active?.side === side && owns(active)) { invalidate(); await serial; return }
    invalidate()
    const op = { side, revision: state.revision, id: null, terminal: new Map(), stopped: false }
    active = op
    publish()
    serial = serial.then(async () => {
      if (!owns(op)) return
      try {
        const result = await facades.speech.play({ text: text(side, state), language: language(side, state) })
        op.id = result.playbackId
        if (!owns(op)) { await stop(op); return }
        const terminal = op.terminal.get(op.id)
        op.terminal.clear()
        if (terminal || result.status !== 'playing') {
          active = null
          const final = terminal ?? result
          if (final.status === 'error') notify(message({name:final.errorCode}, '朗读'))
        }
        publish()
      } catch (error) {
        if (owns(op)) { active = null; notify(message(error, '朗读')); publish() }
      }
    })
    await serial
  }
  function dispose() {
    if (disposed) return
    invalidate()
    disposed = true
    if (typeof unsubscribe === 'function') unsubscribe()
  }
  return { start, availability, run, invalidate, dispose, isPlaying: side => active?.side === side && owns(active) }
}
