const origin = document.querySelector('#origin')
origin.textContent = 'Loaded ' + location.href
document.querySelector('#attempt').addEventListener('click', async () => {
  const output = document.querySelector('#result')
  const invoke = window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke
  if (!invoke) { output.textContent = 'IPC bridge unavailable'; return }
  output.textContent = 'Waiting for IPC'
  try { output.textContent = await invoke('check_origin') }
  catch (error) { output.textContent = 'IPC rejected: ' + String(error) }
})
