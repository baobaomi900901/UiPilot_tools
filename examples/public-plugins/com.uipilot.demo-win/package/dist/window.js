const fields = {
  input: document.querySelector('#input'),
  platform: document.querySelector('#platform'),
  theme: document.querySelector('#theme'),
  instance: document.querySelector('#instance'),
  returnText: document.querySelector('#return-text'),
}

window.uipilotPluginWindow.onUpdate((update) => {
  fields.input.textContent = update.input
  fields.platform.textContent = update.platform
  fields.theme.textContent = update.theme
  fields.instance.textContent = String(update.instanceNumber)
  fields.returnText.textContent = String(update.data.returnText ?? '')
})
