const ICON_ORIGINS = new Set([
  'uipilot-public-plugin://localhost',
  'http://uipilot-public-plugin.localhost',
])
const MAX_ICON_URL_LENGTH = 512

function validPluginId(value: string): boolean {
  return value.length >= 1 && value.length <= 64 &&
    /^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$/.test(value)
}

function validPrepareToken(value: string): boolean {
  return /^public-prepare-[0-9a-f]{16}-[0-9a-f]{16}$/.test(value)
}

export function safePublicPluginIconUrl(value: unknown): string | undefined {
  if (typeof value !== 'string' || value.length === 0 || value.length > MAX_ICON_URL_LENGTH) return undefined
  if (value.includes('\\') || value.includes('%') || value.includes('?') || value.includes('#')) return undefined
  let url: URL
  try {
    url = new URL(value)
  } catch {
    return undefined
  }
  if (!ICON_ORIGINS.has(`${url.protocol}//${url.host}`) || url.username || url.password || url.port || url.search || url.hash) return undefined
  const parts = url.pathname.split('/')
  if (parts[0] !== '' || parts[1] !== '__uipilot_icon') return undefined
  if (parts.length === 6 && parts[2] === 'installed' && parts[5] === 'icon.png') {
    return validPluginId(parts[3]) && /^[1-9][0-9]*$/.test(parts[4]) ? value : undefined
  }
  return parts.length === 5 && parts[2] === 'prepared' && validPrepareToken(parts[3]) && parts[4] === 'icon.png'
    ? value
    : undefined
}
