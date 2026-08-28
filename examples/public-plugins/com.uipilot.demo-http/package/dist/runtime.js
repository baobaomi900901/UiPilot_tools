const HTTPBIN = 'https://httpbin.org'
const MAX_DETAIL_CHARACTERS = 8_192
const USAGE = 'get | post TEXT | text TEXT | form TEXT | status 503 | timeout | denied | protected'
const NETWORK_ERROR_NAMES = new Set([
  'InvalidNetworkRequestError',
  'PermissionDeniedError',
  'NetworkTargetDeniedError',
  'NetworkTimeoutError',
  'NetworkFailureError',
  'NetworkResponseTooLargeError',
  'NetworkResponseInvalidError',
  'NetworkLimitExceededError',
  'ExpiredRequestError',
])

function parseCommand(input) {
  const value = input.trim()
  if (!value) return { name: 'get', argument: '' }

  const separator = value.search(/\s/)
  if (separator === -1) return { name: value.toLowerCase(), argument: '' }
  return {
    name: value.slice(0, separator).toLowerCase(),
    argument: value.slice(separator).trimStart(),
  }
}

function requestFor(command) {
  switch (command.name) {
    case 'get':
      return {
        url: command.argument
          ? `${HTTPBIN}/get?text=${encodeURIComponent(command.argument)}`
          : `${HTTPBIN}/get`,
        method: 'GET',
      }
    case 'post':
      return {
        url: `${HTTPBIN}/anything`,
        method: 'POST',
        body: { type: 'json', value: { text: command.argument } },
      }
    case 'text':
      return {
        url: `${HTTPBIN}/anything`,
        method: 'POST',
        body: { type: 'text', value: command.argument },
      }
    case 'form':
      return {
        url: `${HTTPBIN}/anything`,
        method: 'POST',
        body: { type: 'form', value: { text: command.argument } },
      }
    case 'status':
      if (!/^[1-5][0-9]{2}$/.test(command.argument)) return null
      return { url: `${HTTPBIN}/status/${command.argument}`, method: 'GET' }
    case 'timeout':
      if (command.argument) return null
      return { url: `${HTTPBIN}/delay/15`, method: 'GET' }
    case 'denied':
      if (command.argument) return null
      return { url: 'https://example.com/', method: 'GET' }
    case 'protected':
      if (command.argument) return null
      return {
        url: `${HTTPBIN}/anything`,
        method: 'GET',
        headers: { Host: 'example.com' },
      }
    default:
      return null
  }
}

function oneResult(requestId, result) {
  return { requestId, results: [result] }
}

function usageResult(requestId) {
  return oneResult(requestId, {
    id: 'demo-http-usage',
    title: 'Usage',
    subtitle: 'Choose one Host HTTPS acceptance command',
    detail: USAGE,
  })
}

function responseDetail(body) {
  if (!body) return '(empty body)'
  if (body.length <= MAX_DETAIL_CHARACTERS) return body
  return `${body.slice(0, MAX_DETAIL_CHARACTERS)}\n[truncated]`
}

function errorName(error) {
  if (error && NETWORK_ERROR_NAMES.has(error.name)) return error.name
  return 'NetworkFailureError'
}

export async function onCommand(invocation, api) {
  const command = parseCommand(invocation.input)
  const request = requestFor(command)
  if (!request) return usageResult(invocation.requestId)

  if (!api.network) {
    return oneResult(invocation.requestId, {
      id: 'demo-http-error',
      title: 'Network API unavailable',
      subtitle: 'Host 0.3.2+ is required',
      detail: `Command: ${command.name}`,
    })
  }

  try {
    const response = await api.network.request(request)
    return oneResult(invocation.requestId, {
      id: 'demo-http-success',
      title: `${request.method} ${response.status}`,
      subtitle: 'Host HTTPS request completed',
      detail: responseDetail(response.body),
    })
  } catch (error) {
    return oneResult(invocation.requestId, {
      id: 'demo-http-error',
      title: errorName(error),
      subtitle: 'Host HTTPS request rejected',
      detail: `Command: ${command.name}`,
    })
  }
}
