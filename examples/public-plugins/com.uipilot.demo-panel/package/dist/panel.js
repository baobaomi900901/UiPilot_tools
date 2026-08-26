const input = document.querySelector('#input')
const platform = document.querySelector('#platform')
const theme = document.querySelector('#theme')
const request = document.querySelector('#request')
const stored = document.querySelector('#stored')
const focusState = document.querySelector('#focus-state')
const latestKey = document.querySelector('#latest-key')
const keySource = document.querySelector('#key-source')
const keyCount = document.querySelector('#key-count')
const routeSequence = document.querySelector('#route-sequence')
const keyHistory = document.querySelector('#key-history')

const MAX_KEY_HISTORY = 5
const recentKeys = []
let recordedKeyCount = 0

function renderFocusState() {
  const focused = document.hasFocus()
  focusState.textContent = focused ? 'Focused' : 'Not focused'
  focusState.dataset.focused = String(focused)
}

function renderKeyHistory() {
  keyHistory.replaceChildren()
  if (recentKeys.length === 0) {
    const empty = document.createElement('li')
    empty.className = 'key-history-empty'
    empty.textContent = 'No key events yet'
    keyHistory.append(empty)
    return
  }

  for (const entry of recentKeys) {
    const item = document.createElement('li')
    const route = entry.routeSequence === null ? '' : ` | route ${entry.routeSequence}`
    item.textContent = `${entry.key} | ${entry.source}${route}`
    keyHistory.append(item)
  }
}

function recordKeyEvent({ key, source, routeSequence: nextRouteSequence = null }) {
  recordedKeyCount += 1
  recentKeys.unshift({ key, source, routeSequence: nextRouteSequence })
  recentKeys.length = Math.min(recentKeys.length, MAX_KEY_HISTORY)

  latestKey.textContent = key
  keySource.textContent = source
  keyCount.textContent = String(recordedKeyCount)
  routeSequence.textContent = nextRouteSequence ?? 'None'
  renderKeyHistory()
}

function formatHostKey(key) {
  return key === 'n' ? 'Ctrl+N' : key
}

function formatContentKey(event) {
  const modifiers = []
  if (event.ctrlKey && event.key !== 'Control') modifiers.push('Ctrl')
  if (event.metaKey && event.key !== 'Meta') modifiers.push('Meta')
  if (event.altKey && event.key !== 'Alt') modifiers.push('Alt')
  if (event.shiftKey && event.key !== 'Shift') modifiers.push('Shift')

  const key = event.key === ' '
    ? 'Space'
    : event.key.length === 1
      ? event.key.toUpperCase()
      : event.key
  return [...modifiers, key].join('+')
}

renderFocusState()
renderKeyHistory()
window.addEventListener('focus', renderFocusState)
window.addEventListener('blur', renderFocusState)

window.uipilotPluginPanel.onHostKey((event) => {
  recordKeyEvent({
    key: formatHostKey(event.key),
    source: 'Host route',
    routeSequence: event.routeSequence,
  })
})

window.uipilotPluginPanel.onUpdate(async (update) => {
  document.documentElement.dataset.theme = update.theme
  input.textContent = update.input
  platform.textContent = update.platform
  theme.textContent = update.theme
  request.textContent = update.requestId

  const previous = await window.uipilotPluginPanel.storage.get('demo-panel.last-input')
  stored.textContent = typeof previous === 'string' ? previous : ''
  await window.uipilotPluginPanel.storage.set('demo-panel.last-input', update.input)
})

window.addEventListener('keydown', (event) => {
  recordKeyEvent({ key: formatContentKey(event), source: 'Panel content' })
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
    event.preventDefault()
    void window.uipilotPluginPanel.focusHostInput()
  }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'h') {
    event.preventDefault()
    void window.uipilotPluginPanel.requestHide()
  }
}, true)
