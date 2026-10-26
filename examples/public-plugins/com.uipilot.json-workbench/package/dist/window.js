const windowApi = window.uipilotPluginWindow
const ui = Object.fromEntries(['input', 'output', 'import', 'format', 'save', 'reload', 'status', 'draft-state', 'byte-count', 'discard', 'discard-message', 'discard-confirm', 'discard-cancel'].map(id => [id, document.getElementById(id)]))
const encoder = new TextEncoder()
const bytes = text => encoder.encode(text).length
const messages = {
  'invalid-payload': '请求内容不正确，当前草稿已保留。',
  'text-limit': '输入超过 8192 字节（UTF-8），请缩短后再操作。',
  'invalid-json': 'JSON 语法不正确，请检查引号、逗号和括号。草稿仍可单独保存。',
  'non-finite-number': 'JSON 包含超出有限数值范围的数字，未格式化。',
  'depth-limit': 'JSON 嵌套超过 16 层，未格式化。',
  'value-limit': 'JSON 超过 2048 个值，未格式化。',
  'formatted-limit': '格式化结果超过 32768 字节，未显示；请缩短输入。',
  'invalid-stored-data': '已保存数据不兼容或损坏，已禁止保存；数据未重置或覆盖。',
  'storage-unavailable': '无法读取已保存数据，草稿已保留；请重新读取核对后再保存。',
  'revision-conflict': '已保存版本发生变化，未覆盖。请重新读取并核对后再编辑。',
  'revision-exhausted': '草稿版本已达上限，未写入。',
  'save-uncertain': '保存结果尚未确认，草稿已保留。请重新读取核对，勿重复保存。',
  'reload-edited': '读取期间输入已更改，当前草稿已保留。请重新读取并确认后再保存。',
  'response-limit': '结果超过单次响应上限，当前草稿已保留。',
  'unknown-action': '当前操作不可用，草稿已保留。',
  'action-failed': '操作未完成，当前草稿已保留。',
}
const bridgeMessages = {
  InvalidWindowRequestError: '请求不符合窗口协议，当前草稿已保留。',
  WindowRequestBusyError: '窗口或 Runtime 正忙，请稍后再操作。',
  ExpiredWindowSessionError: '窗口会话已失效，请重新打开工作台；未保存内容不会恢复。',
  WindowRequestCancelledError: '操作已取消，当前草稿已保留。',
  WindowRequestTimeoutError: '操作超时，当前草稿已保留。',
  InvalidWindowResponseError: '返回数据不符合协议，当前草稿已保留。',
  WindowRequestFailedError: '操作未完成，当前草稿已保留。',
  WindowRequestUnavailableError: '窗口请求暂不可用，当前草稿已保留。',
}
let session = null
let draft = { text: '', savedText: null, revision: null, hasSaved: false, editRevision: 0 }
let activeRequest = null
let recovery = null
let confirming = false
let confirmation = null

function dirty() { return draft.imported === true || draft.text !== draft.savedText }
function available() { return typeof session?.request === 'function' }
function status(text, tone = '') {
  ui.status.textContent = text
  ui.status.dataset.tone = tone
}
function controls() {
  const busy = activeRequest !== null
  const length = bytes(draft.text)
  ui.input.disabled = session === null
  ui.format.disabled = busy || confirming || !available() || length > 8192
  ui.save.disabled = busy || confirming || !available() || recovery !== null || draft.revision === null || length > 8192
  ui.reload.disabled = busy || confirming || !available()
  ui.import.disabled = busy || confirming || typeof session?.files?.openText !== 'function' || recovery !== null
  ui.discard.hidden = !confirming
  ui['byte-count'].textContent = String(length) + ' / 8192 字节'
  ui['byte-count'].dataset.overLimit = String(length > 8192)
  ui['draft-state'].textContent = busy ? '处理中…' : !available() ? '窗口请求不可用'
    : recovery === 'invalid-stored-data' ? '存储数据不兼容 · 仅校验'
    : recovery ? '保存状态待核对 · 草稿保留'
    : dirty() ? '未保存的更改' : draft.hasSaved ? '已保存 · 版本 ' + draft.revision : '新草稿 · 尚未保存'
  document.querySelector('main').setAttribute('aria-busy', String(busy))
}
function isLoaded(data) {
  return data?.ok === true && Number.isSafeInteger(data.revision) && data.revision >= 0
    && typeof data.text === 'string' && bytes(data.text) <= 8192
    && data.hasSaved === (data.revision > 0)
}
function applyLoaded(data) {
  draft = { text: data.text, savedText: data.text, revision: data.revision, hasSaved: data.hasSaved, editRevision: 0 }
  ui.input.value = draft.text
  ui.output.value = ''
  recovery = null
}
function current(owner) { return session === owner.session && activeRequest === owner }
function fail(code, action) {
  if (action === 'load' || ['save-uncertain', 'revision-conflict', 'invalid-stored-data', 'storage-unavailable', 'revision-exhausted'].includes(code)) recovery = code
  status(messages[code] ?? messages['action-failed'], 'error')
}

async function request(action, payload) {
  if (activeRequest || confirming || !available()) return
  const owner = { session, editRevision: draft.editRevision, text: draft.text, revision: draft.revision }
  activeRequest = owner
  if (action === 'format') ui.output.value = ''
  controls()
  status(action === 'save' ? '正在保存…可以继续编辑，后续编辑不会被标记为已保存。' : action === 'load' ? '正在读取…读取成功前保留当前草稿。' : '正在校验并格式化…')
  try {
    const data = await owner.session.request({ action, payload })
    if (!current(owner)) return
    if (data?.ok !== true) { fail(data?.code ?? 'action-failed', action); return }
    if (action === 'format') {
      if (typeof data.formatted !== 'string' || bytes(data.formatted) > 32768) throw { name: 'InvalidWindowResponseError' }
      if (draft.editRevision !== owner.editRevision) {
        status('输入已更改，已忽略旧格式化结果；请再次校验。')
        return
      }
      ui.output.value = data.formatted
      status('JSON 校验通过。结果仅供查看，未更改输入或保存。' + (recovery ? ' ' + (messages[recovery] ?? '') : ''))
    } else if (action === 'save') {
      if (data.revision !== owner.revision + 1 || data.text !== owner.text) throw { name: 'InvalidWindowResponseError' }
      draft.revision = data.revision
      draft.savedText = data.text
      draft.hasSaved = true
      if (draft.editRevision === owner.editRevision) draft.imported = false
      recovery = null
      status(dirty() ? '先前版本已保存；之后的编辑仍未保存。' : '已保存 · 版本 ' + data.revision + '。关闭重开或重启后可读取。')
    } else if (action === 'load') {
      if (!isLoaded(data)) throw { name: 'InvalidWindowResponseError' }
      if (draft.editRevision !== owner.editRevision) { fail('reload-edited', action); return }
      applyLoaded(data)
      status(data.hasSaved ? '已重新读取版本 ' + data.revision + '，当前显示已保存内容。' : '没有已保存草稿。当前为空；输入内容后可显式保存。')
    }
  } catch (error) {
    if (!current(owner)) return
    if (action === 'save') {
      recovery = 'save-uncertain'
      status(messages['save-uncertain'], 'error')
    } else {
      if (action === 'load') recovery = 'storage-unavailable'
      status(bridgeMessages[error?.name] ?? messages['action-failed'], 'error')
    }
  } finally {
    if (current(owner)) { activeRequest = null; controls() }
  }
}

ui.input.addEventListener('input', () => {
  draft.text = ui.input.value
  draft.editRevision += 1
  ui.output.value = ''
  if (confirming) { confirming = false; confirmation = null }
  controls()
  if (!activeRequest) status(bytes(draft.text) > 8192 ? messages['text-limit'] : recovery ? messages[recovery] ?? messages['action-failed'] : '草稿已更改。格式化不会保存；关闭或隐藏后，未保存内容不会恢复。', bytes(draft.text) > 8192 || recovery ? 'error' : '')
})
ui.format.addEventListener('click', () => request('format', { text: draft.text }))
ui.save.addEventListener('click', () => {
  if (recovery || draft.revision === null) return
  void request('save', { text: draft.text, expectedRevision: draft.revision })
})
ui.reload.addEventListener('click', () => {
  if (activeRequest || confirming || !available()) return
  if (dirty() || recovery === 'save-uncertain') {
    confirming = true
    confirmation = { action: 'load', editRevision: draft.editRevision }
    ui['discard-confirm'].textContent = '放弃草稿并重新读取'
    ui['discard-message'].textContent = recovery === 'save-uncertain'
      ? '此前保存可能已写入。重新读取成功后将以已保存内容替换当前草稿，未保存编辑会丢失。'
      : '重新读取成功后将以已保存内容替换当前草稿，未保存编辑会丢失。'
    controls()
    ui['discard-confirm'].focus()
    return
  }
  void request('load', null)
})
ui['discard-confirm'].addEventListener('click', () => {
  if (!confirming || activeRequest) return
  const allowed = confirmation
  confirming = false
  confirmation = null
  if (!allowed || allowed.editRevision !== draft.editRevision) { controls(); return }
  if (allowed.action === 'import') void importFile()
  else void request('load', null)
})
ui['discard-cancel'].addEventListener('click', () => {
  const origin = confirmation?.action
  confirming = false
  confirmation = null
  controls()
  ;(origin === 'import' ? ui.import : ui.reload).focus()
})

const fileMessages = {
  FileOpenBusyError: '文件选择器正忙，请稍后重试。',
  ExpiredWindowSessionError: '窗口会话已失效，请重新打开工作台。',
  FileOpenPermissionError: '缺少导入文件权限，请确认插件授权。',
  FileOpenUnsupportedError: '请选择本地普通文件（UTF-8，最多 8 KiB）。',
  FileOpenTooLargeError: '文件超过 8 KiB，当前草稿已保留。',
  FileOpenEncodingError: '文件须为 UTF-8 文本且不含 NUL，当前草稿已保留。',
  FileOpenFailedError: '文件导入未完成，当前草稿已保留。',
}
async function importFile() {
  if (activeRequest || confirming || recovery || typeof session?.files?.openText !== 'function') return
  const owner = { session, editRevision: draft.editRevision }
  activeRequest = owner
  controls()
  status('请选择 UTF-8 文本文件（最多 8 KiB）；可以继续编辑。')
  try {
    const result = await owner.session.files.openText()
    if (!current(owner)) return
    if (result === null) { status('已取消导入，当前草稿已保留。'); return }
    if (draft.editRevision !== owner.editRevision) { status('选择期间输入已更改，已忽略导入并保留当前草稿。'); return }
    draft.text = result.text
    draft.imported = true
    draft.editRevision += 1
    ui.input.value = draft.text
    ui.output.value = ''
    status('已导入 ' + result.name + ' · 未保存。请显式校验或保存草稿。')
  } catch (error) {
    if (current(owner)) status(fileMessages[error?.name] ?? fileMessages.FileOpenFailedError, 'error')
  } finally { if (current(owner)) { activeRequest = null; controls() } }
}
ui.import.addEventListener('click', () => {
  if (activeRequest || confirming || recovery || typeof session?.files?.openText !== 'function') return
  if (dirty()) {
    confirming = true
    confirmation = { action: 'import', editRevision: draft.editRevision }
    ui['discard-message'].textContent = '导入成功后将替换当前输入；取消或错误会保留草稿和格式化结果。'
    ui['discard-confirm'].textContent = '选择文件并替换草稿'
    controls()
    ui['discard-confirm'].focus()
  } else void importFile()
})
windowApi.onUpdate(update => {
  // Capture after PREPARE, return synchronously, and invoke only on a later user action.
  session = { files: windowApi.files, request: windowApi.request, requestId: update.requestId }
  activeRequest = null
  confirming = false
  confirmation = null
  recovery = null
  draft = { text: '', savedText: null, revision: null, hasSaved: false, editRevision: 0 }
  ui.input.value = ''
  ui.output.value = ''
  document.documentElement.dataset.theme = update.theme
  if (isLoaded(update.data)) {
    applyLoaded(update.data)
    status(update.data.hasSaved ? '已读取版本 ' + draft.revision + '。编辑后请点击“保存草稿”。' : '尚无已保存草稿。输入 JSON 后校验，或直接保存未完成的草稿。')
  } else fail(update.data?.code ?? 'action-failed', 'load')
  if (!available()) status('当前宿主不支持窗口请求，请使用 UiPilot 0.9.0 或更新版本。', 'error')
  controls()
})
controls()
