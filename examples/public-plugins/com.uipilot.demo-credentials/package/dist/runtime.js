const ERROR_NAMES = new Set([
  'InvalidNetworkRequestError', 'PermissionDeniedError', 'NetworkTargetDeniedError',
  'CredentialUnavailableError', 'NetworkTimeoutError', 'NetworkFailureError',
  'NetworkResponseTooLargeError', 'NetworkResponseInvalidError',
  'NetworkLimitExceededError', 'ExpiredRequestError',
])

function result(requestId, title, detail) {
  return { requestId, results: [{ id: 'demo-credentials-result', title, detail }] }
}

// Keep detail below the Host's 16 KiB UTF-8 budget, including the suffix.
function responseDetail(body) {
  if (!body) return '(empty body)'
  let bytes = 0
  let detail = ''
  for (const character of body) {
    const code = character.codePointAt(0)
    bytes += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4
    if (bytes > 8192) return `${detail}\n[truncated]`
    detail += character
  }
  return detail
}

export async function onCommand(invocation, api) {
  const command = invocation.input.trim().toLowerCase() || 'request'
  if (command === 'status') {
    const configured = await api.settings.isSecretConfigured('api-token')
    return result(invocation.requestId,
      configured ? 'Credential configured' : 'Credential not configured',
      'Presence only; configure a synthetic token in Host settings.')
  }
  if (command !== 'request') return result(invocation.requestId, 'Usage', 'request | status')
  if (!api.network) return result(invocation.requestId, 'Network API unavailable', 'Windows Host 0.4.0+ is required.')
  try {
    const response = await api.network.request({
      url: 'https://api.example.com/v1/demo',
      method: 'GET',
      credential: 'service-token',
    })
    // Provider responses are untrusted plain text and can echo what the service received.
    return result(invocation.requestId, `HTTP ${response.status}`, responseDetail(response.body))
  } catch (error) {
    const name = error && ERROR_NAMES.has(error.name) ? error.name : 'NetworkFailureError'
    return result(invocation.requestId, name, name === 'PermissionDeniedError'
      ? 'Network access is off. Enable it for this plugin in Settings → Permissions, then run the command again.'
      : 'Check Host credential configuration and network consent; start a new command to retry.')
  }
}
