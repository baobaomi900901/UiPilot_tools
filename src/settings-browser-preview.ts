// Reuse the real settings view and the existing in-memory preview client.
const previewUrl = new URL(window.location.href)
previewUrl.searchParams.set('mode', 'settings')
window.history.replaceState(null, '', previewUrl)

await import('./launcher-browser-preview')

export {}
