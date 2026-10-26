import { createList, createDetail, createActions } from './ui/ui.js'

const panel = window.uipilotPluginPanel
const element = (id) => document.getElementById(id)
const ui = Object.fromEntries(['new', 'list', 'filter', 'draft-state', 'preview', 'save', 'empty', 'editor', 'title', 'body', 'variables', 'variable-hint', 'output', 'status', 'reload', 'discard-dialog', 'discard-message'].map((id) => [id, element(id)]))
const messages = {
  'invalid-payload': '请求内容不正确，草稿已保留。',
  'invalid-title': '请输入模板名称，最多 80 个字符。',
  'body-limit': '正文不能超过 8 KiB（按 UTF-8 计算）。',
  'invalid-syntax': '变量格式不正确。请使用 {{name}}；名称以英文字母开头，只含字母、数字和下划线，最多 32 位。',
  'variable-limit': '每个模板最多使用 16 个不同变量。',
  'invalid-values': '样例值无效或重复；每个值最多 256 个字符。',
  'preview-limit': '预览结果超过 32 KiB，请缩短正文或样例。',
  'template-limit': '已达到 20 个模板上限，当前草稿仍保留。',
  'catalog-capacity': '模板总数据超过 512 KiB，未写入；请缩短内容，草稿仍保留。',
  'catalog-invalid': '已保存数据不符合格式，未重置或覆盖。请保留数据并反馈此问题。',
  'storage-unavailable': '无法读取已保存数据，请稍后重新打开；草稿仍保留。',
  'counter-exhausted': '模板编号或版本已达上限，未写入。',
  'conflict': '已保存版本发生变化。请重新载入已保存内容后再编辑。',
  'not-found': '没有找到该模板，请在主输入框按 Enter 刷新列表。',
  'response-limit': '数据超过单次响应上限，草稿仍保留。',
  'unknown-action': '当前操作不可用。',
  'action-failed': '操作未完成，草稿仍保留。',
}
let session = null
let lastUpdateRequestId = null
let draft = null
let nextDraft = 0
let templates = []
let filter = ''
let activeRequest = null
let recovery = null
let pendingDiscard = null
let discardedCloseEvents = 0
const list = createList(ui.list, { onActivate: (id) => choose(() => load(id)) })
const detail = createDetail({ empty: ui.empty, content: ui.editor, label: ui['draft-state'] })
const actions = createActions({
  new: { button: ui.new, onInvoke: newDraft },
  preview: { button: ui.preview, onInvoke: preview },
  save: { button: ui.save, onInvoke: save },
  reload: { button: ui.reload, onInvoke: reload },
})

function status(text, tone = '') {
  ui.status.textContent = text
  ui.status.dataset.tone = tone
}

function dirty() { return draft && draft.editRevision !== draft.savedEditRevision }
function available() { return typeof panel.request === 'function' && session !== null }

function controls() {
  const busy = activeRequest !== null
  actions.update({
    new: { disabled: busy || !available() },
    preview: { disabled: busy || !draft || !available() },
    save: { disabled: busy || !draft || Boolean(recovery) || !available() },
    reload: { disabled: busy || !available() },
  })
  ui.reload.hidden = !recovery
  detail.update({
    hasContent: Boolean(draft), busy,
    text: !draft ? '未选择模板' : busy ? '处理中…' : recovery ? '保存状态待确认 · 草稿保留' : dirty() ? '未保存的更改' : '已保存 · 版本 ' + draft.revision,
  })
}

function renderList() {
  ui.filter.textContent = filter.trim() ? '筛选：' + filter : '全部已保存模板'
  list.update({
    items: templates.filter((item) => item.title.toLocaleLowerCase().includes(filter.trim().toLocaleLowerCase())),
    selectedId: draft?.id ?? null,
    disabled: activeRequest !== null,
    emptyText: filter.trim() ? '没有匹配模板。可清空主输入框后按 Enter。' : '暂无模板，点击“新建模板”开始。',
  })
}

function renderVariables() {
  ui.variables.replaceChildren()
  ui['variable-hint'].textContent = draft.variables.length ? '未填写表示缺值；主动填写后清空表示明确的空字符串。' : '没有已发现变量。修改正文后点击“预览”重新发现。'
  draft.variables.forEach((name, index) => {
    const wrapper = document.createElement('div')
    wrapper.className = 'variable'
    const label = document.createElement('label')
    label.htmlFor = `sample-${index}`
    label.textContent = name
    const input = document.createElement('input')
    input.id = label.htmlFor
    input.type = 'text'
    input.autocomplete = 'off'
    input.placeholder = '未填写（缺值）'
    input.value = draft.values.get(name) ?? ''
    input.addEventListener('input', () => {
      draft.values.set(name, input.value)
      edited()
    })
    wrapper.append(label, input)
    ui.variables.append(wrapper)
  })
}

function renderDraft() {
  if (draft) {
    ui.title.value = draft.title
    ui.body.value = draft.body
    ui.output.value = ''
    renderVariables()
  }
  controls()
  renderList()
}

function edited() {
  draft.editRevision += 1
  ui.output.value = ''
  controls()
  if (!recovery) status('草稿已更改。预览不会保存；Escape 或失焦关闭后，未保存内容不会保留。')
}

function newDraft() {
  if (activeRequest || !available()) return
  if (recovery?.kind === 'new' && !recovery.refreshed) {
    status('请先点击“重新载入已保存”，在主输入框清空筛选并按 Enter，检查模板是否已保存。', 'error')
    return
  }
  choose(() => {
    draft = { token: ++nextDraft, id: null, revision: null, title: '', body: '', values: new Map(), variables: [], editRevision: 1, savedEditRevision: 0 }
    recovery = null
    renderDraft()
    status('新草稿尚未保存。填写名称和正文，再点击“预览”发现变量。')
    ui.title.focus()
  })
}

function choose(action) {
  if (activeRequest || pendingDiscard) return
  if (!dirty() && !recovery) { action(); return }
  pendingDiscard = action
  ui['discard-message'].textContent = recovery
    ? '先检查已保存内容。继续会放弃当前草稿；此前保存可能已经提交，不能当作未保存。'
    : '继续会放弃当前未保存的编辑。'
  ui['discard-dialog'].returnValue = 'cancel'
  ui['discard-dialog'].showModal()
}

for (const id of ['discard-cancel', 'discard-close']) {
  element(id).addEventListener('click', () => ui['discard-dialog'].close('cancel'))
}

ui['discard-dialog'].addEventListener('close', () => {
  if (discardedCloseEvents) { discardedCloseEvents -= 1; return }
  const action = pendingDiscard
  pendingDiscard = null
  if (ui['discard-dialog'].returnValue === 'discard') action?.()
})

function current(owner, includeText = false) {
  return session === owner.session && (draft?.token ?? null) === owner.draftToken
    && (!includeText || (draft?.editRevision ?? null) === owner.editRevision)
}

async function request(action, payload, apply) {
  if (activeRequest || !available()) return
  const owner = { session, draftToken: draft?.token ?? null, editRevision: draft?.editRevision ?? null }
  activeRequest = owner
  controls()
  renderList()
  status(action === 'save' ? '正在保存…可以继续编辑，新编辑不会被标记为已保存。' : '正在处理…')
  try {
    const data = await panel.request({ action, payload })
    if (!current(owner)) return
    if (data.ok) apply(data, owner)
    else if (action === 'preview' && data.code === 'missing-variable') apply(data, owner)
    else {
      if (action === 'save' && (data.code === 'save-uncertain' || data.code === 'conflict' || data.code === 'response-limit')) {
        recovery = { kind: draft.id === null ? 'new' : 'known', refreshRequested: false, refreshed: false }
      }
      status(data.code === 'save-uncertain' ? '保存结果待确认，可能已写入。请重新载入已保存内容，勿直接重试。' : messages[data.code] ?? '操作未完成，草稿仍保留。', 'error')
    }
  } catch (error) {
    if (!current(owner)) return
    const name = error?.name
    if (action === 'save' && name !== 'PanelRequestBusyError' && name !== 'InvalidPanelRequestError') {
      recovery = { kind: draft.id === null ? 'new' : 'known', refreshRequested: false, refreshed: false }
      status('保存结果待确认，可能已写入。请重新载入已保存内容，勿直接重试。', 'error')
    } else {
      status(name === 'PanelRequestBusyError' ? '宿主正在处理其他操作，请稍后再点击。草稿仍保留。' : name === 'InvalidPanelRequestError' ? '请求超过限制或格式不正确，未提交；请缩短内容。' : '操作已中断或会话失效。草稿仍保留，请重新打开后检查已保存内容。', 'error')
    }
  } finally {
    if (activeRequest === owner) { activeRequest = null; controls(); renderList() }
  }
}

async function load(id) {
  await request('load', { id }, (data, owner) => {
    if (!current(owner, true)) { status('加载期间已有新编辑，草稿已保留。请再次选择模板。'); return }
    const item = data.template
    draft = { token: ++nextDraft, id: item.id, revision: item.revision, title: item.title, body: item.body, values: new Map(item.values.map(({ name, value }) => [name, value])), variables: data.variables, editRevision: 0, savedEditRevision: 0 }
    recovery = null
    renderDraft()
    status('已载入保存内容。编辑后请点击“保存”。')
  })
}

function values() { return [...draft.values].map(([name, value]) => ({ name, value })) }

function preview() {
  if (!draft) return
  void request('preview', { body: draft.body, values: values() }, (data, owner) => {
    if (!current(owner, true)) { status('预览期间已有新编辑，请再次预览。'); return }
    draft.variables = data.variables
    draft.values = new Map(data.values.map(({ name, value }) => [name, value]))
    renderVariables()
    ui.output.value = data.ok ? data.text : ''
    status(data.ok ? '预览已更新，尚未执行保存。' : `缺少变量样例：${data.missing.join('、')}。请填写后再次预览。`, data.ok ? '' : 'error')
  })
}

function save() {
  if (!draft || recovery) return
  void request('save', { id: draft.id, expectedRevision: draft.revision, title: draft.title, body: draft.body, values: values() }, (data, owner) => {
    // Identity survives later typing in this SAME draft. Only unchanged text receives normalized samples.
    draft.id = data.id
    draft.revision = data.revision
    draft.savedEditRevision = owner.editRevision
    templates = data.templates
    if (current(owner, true)) {
      draft.variables = data.variables
      draft.values = new Map(data.values.map(({ name, value }) => [name, value]))
      renderVariables()
    }
    status(dirty() ? '先前版本已保存；之后的编辑仍未保存。' : '已保存。关闭重开或重启后可重新载入。')
  })
}

ui.title.addEventListener('input', () => { if (draft) { draft.title = ui.title.value; edited() } })
ui.body.addEventListener('input', () => { if (draft) { draft.body = ui.body.value; edited() } })
function reload() {
  if (!recovery || activeRequest) return
  if (draft.id !== null) { choose(() => load(draft.id)); return }
  recovery.refreshRequested = true
  recovery.afterRequestId = lastUpdateRequestId
  status('请在主输入框清空筛选并按 Enter 刷新列表，然后选择已保存模板；确认未保存后也可主动新建草稿。当前草稿仍保留。')
  void panel.focusHostInput().catch(() => status('无法聚焦主输入框。请手动点击，清空筛选并按 Enter 刷新。', 'error'))
}

function focusable() {
  return [...document.querySelectorAll('button, input, textarea')].filter((node) => !node.disabled && node.getClientRects().length && !node.closest('dialog'))
}

panel.onHostKey((event) => {
  if (ui['discard-dialog'].open) return
  if (event.key === 'ArrowDown') list.move(1)
  if (event.key === 'ArrowUp') list.move(-1)
  if (event.key === 'n') newDraft()
  if (event.key === 'Tab') {
    const entries = focusable()
    ;(event.shiftKey ? entries.at(-1) : entries[0])?.focus()
  }
})

panel.onUpdate((update) => {
  document.documentElement.dataset.theme = update.theme
  if (session !== update.sessionEpoch) {
    list.update({ items: [], disabled: true })
    session = update.sessionEpoch
    draft = null
    recovery = null
    activeRequest = null
    // Native close events are queued. Ignore the old event even if a new
    // choice already belongs to the replacement session.
    if (pendingDiscard) discardedCloseEvents += 1
    pendingDiscard = null
    if (ui['discard-dialog'].open) ui['discard-dialog'].close('cancel')
  }
  filter = update.input
  if (update.data.ok) {
    templates = update.data.templates
    if (recovery?.kind === 'new' && recovery.refreshRequested && !filter.trim() && update.requestId !== recovery.afterRequestId) {
      recovery.refreshed = true
      status('列表已刷新。请先检查已保存模板，再主动选择一项；若确认未保存，可新建草稿。当前草稿仍保留。')
    } else if (!recovery) status(dirty() ? '筛选已更新，未保存的草稿仍保留。' : '在主输入框按 Enter 筛选模板名称。')
  } else status(messages[update.data.code] ?? '无法加载模板列表。', 'error')
  if (!available()) status('当前宿主不支持模板操作，请使用 UiPilot 0.6.0 或更新版本。', 'error')
  lastUpdateRequestId = update.requestId
  controls()
  renderList()
})

document.addEventListener('keydown', (event) => {
  if (event.isComposing || ui['discard-dialog'].open) return
  if (event.ctrlKey || event.metaKey) {
    const key = event.key.toLowerCase()
    if (key === 'f') { event.preventDefault(); void panel.focusHostInput().catch(() => status('无法聚焦主输入框。', 'error')) }
    if (key === 'n') { event.preventDefault(); newDraft() }
    if (key === 's') { event.preventDefault(); save() }
  }
})
controls()
