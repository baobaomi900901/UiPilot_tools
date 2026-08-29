const YOUDAO_ENDPOINT = 'https://openapi.youdao.com/api'

// Development-only credentials are intentionally inspectable in this package.
const DEVELOPMENT_ONLY_CREDENTIALS = Object.freeze({
  appId: '4b5319dadbd9a48e',
  appSecret: 'Nfn56NiVLBGWGcHEuuiDkW0aZJLSx9X7',
})

const HOST_ERROR_RESULTS = Object.freeze({
  InvalidNetworkRequestError: ['翻译请求无效', '插件请求配置需要修正'],
  PermissionDeniedError: ['网络权限未授权', '请在插件设置中开启网络访问'],
  NetworkTargetDeniedError: ['翻译服务地址被拒绝', '请检查插件声明的 HTTPS 域名'],
  NetworkTimeoutError: ['翻译请求超时', '请稍后重试'],
  NetworkFailureError: ['网络连接失败', '请检查网络后重试'],
  NetworkResponseTooLargeError: ['翻译响应过大', '服务返回内容超过 Host 限制'],
  NetworkResponseInvalidError: ['翻译响应无效', '服务返回了无法处理的内容'],
  NetworkLimitExceededError: ['翻译请求过于频繁', '请稍后重试'],
  ExpiredRequestError: ['本次翻译已取消', '输入已变化或插件状态已更新'],
})

const PROVIDER_ERROR_RESULTS = Object.freeze({
  '101': ['翻译请求参数错误', '请检查输入后重试'],
  '102': ['暂不支持该语言方向', '目前仅支持中英互译'],
  '103': ['输入内容过长', '请缩短文本后重试'],
  '108': ['翻译服务认证失败', '请检查临时测试凭据'],
  '110': ['翻译服务尚未开通', '请检查有道应用的服务绑定'],
  '113': ['请输入需要翻译的内容', '命令参数不能为空'],
  '202': ['翻译服务认证失败', '请检查临时测试凭据'],
  '203': ['当前网络未获供应商授权', '请检查有道应用的 IP 限制'],
  '1411': ['翻译请求过于频繁', '请稍后重试'],
  '2411': ['翻译请求过于频繁', '请稍后重试'],
  '9411': ['翻译请求过于频繁', '请稍后重试'],
})

function oneResult(requestId, result) {
  return { requestId, results: [result] }
}

function errorResult(requestId, title, subtitle) {
  return oneResult(requestId, {
    id: 'translate-error',
    title,
    subtitle,
  })
}

function directionFor(query) {
  return /[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]/u.test(query)
    ? { from: 'zh-CHS', to: 'en', label: '中译英' }
    : { from: 'en', to: 'zh-CHS', label: '英译中' }
}

export function truncateQuery(query) {
  if (query.length <= 20) return query
  return `${query.slice(0, 10)}${query.length}${query.slice(-10)}`
}

async function sha256Hex(value) {
  const bytes = new TextEncoder().encode(value)
  const digest = await globalThis.crypto.subtle.digest('SHA-256', bytes)
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('')
}

export async function signV3({ appId, appSecret, query, salt, curtime }, digest = sha256Hex) {
  return digest(`${appId}${truncateQuery(query)}${salt}${curtime}${appSecret}`)
}

function translatedResult(requestId, text, direction) {
  const title = Array.from(text).slice(0, 256).join('')
  return oneResult(requestId, {
    id: 'translate-result',
    title,
    subtitle: `${direction.label} · 按 Enter 复制`,
    defaultAction: { type: 'copyText', text },
  })
}

function providerResult(requestId, response, direction) {
  if (response.status < 200 || response.status >= 300) {
    return errorResult(requestId, '翻译服务暂时不可用', `HTTP ${response.status}，请稍后重试`)
  }

  let payload
  try {
    payload = JSON.parse(response.body)
  } catch {
    return errorResult(requestId, '翻译服务响应异常', '服务返回了无法解析的内容')
  }

  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
    return errorResult(requestId, '翻译服务响应异常', '服务返回了无法处理的内容')
  }

  const errorCode = typeof payload.errorCode === 'string' ? payload.errorCode : ''
  if (errorCode !== '0') {
    const [title, subtitle] = PROVIDER_ERROR_RESULTS[errorCode] ?? [
      '翻译服务返回错误',
      errorCode ? `供应商错误码 ${errorCode}` : '请稍后重试',
    ]
    return errorResult(requestId, title, subtitle)
  }

  const translations = Array.isArray(payload.translation)
    ? payload.translation.filter((value) => typeof value === 'string' && value.length > 0)
    : []
  if (translations.length === 0) {
    return errorResult(requestId, '翻译服务响应异常', '响应中没有可用译文')
  }
  return translatedResult(requestId, translations.join('\n'), direction)
}

function hostErrorResult(requestId, error) {
  const [title, subtitle] = HOST_ERROR_RESULTS[error?.name] ?? ['翻译失败', '请稍后重试']
  return errorResult(requestId, title, subtitle)
}

function defaultSalt() {
  return globalThis.crypto.randomUUID()
}

function defaultCurrentTimeSeconds() {
  return Math.floor(Date.now() / 1000).toString()
}

export function createTranslator({
  appId,
  appSecret,
  createSalt = defaultSalt,
  currentTimeSeconds = defaultCurrentTimeSeconds,
}) {
  return async function translate(invocation, api) {
    const query = invocation.input
    if (!query) {
      return errorResult(invocation.requestId, '请输入需要翻译的内容', '支持中文和英文')
    }
    if (!api.network) {
      return errorResult(invocation.requestId, '网络能力不可用', '需要 UiPilot 0.3.2 或更高版本')
    }

    try {
      const direction = directionFor(query)
      const salt = createSalt()
      const curtime = currentTimeSeconds()
      const sign = await signV3({ appId, appSecret, query, salt, curtime })
      const response = await api.network.request({
        url: YOUDAO_ENDPOINT,
        method: 'POST',
        headers: { accept: 'application/json' },
        body: {
          type: 'form',
          value: {
            q: query,
            from: direction.from,
            to: direction.to,
            appKey: appId,
            salt,
            sign,
            signType: 'v3',
            curtime,
          },
        },
      })
      return providerResult(invocation.requestId, response, direction)
    } catch (error) {
      return hostErrorResult(invocation.requestId, error)
    }
  }
}

export const onCommand = createTranslator(DEVELOPMENT_ONLY_CREDENTIALS)
