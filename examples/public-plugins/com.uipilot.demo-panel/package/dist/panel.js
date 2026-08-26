const input = document.querySelector('#input')
const platform = document.querySelector('#platform')
const theme = document.querySelector('#theme')
const request = document.querySelector('#request')
const stored = document.querySelector('#stored')

window.uipilotPluginPanel.onHostKey(() => {})

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
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
    event.preventDefault()
    void window.uipilotPluginPanel.focusHostInput()
  }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'h') {
    event.preventDefault()
    void window.uipilotPluginPanel.requestHide()
  }
})
