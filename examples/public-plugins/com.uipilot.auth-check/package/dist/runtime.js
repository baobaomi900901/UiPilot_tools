const ERROR_NAMES = new Set([
  'InvalidNetworkRequestError', 'PermissionDeniedError', 'NetworkTargetDeniedError',
  'CredentialUnavailableError', 'NetworkTimeoutError', 'NetworkFailureError',
  'NetworkResponseTooLargeError', 'NetworkResponseInvalidError',
  'NetworkLimitExceededError', 'ExpiredRequestError',
])

function result(requestId, title, detail) {
  return { requestId, results: [{ id: 'auth-check-result', title, detail }] }
}

export async function onCommand(invocation, api) {
  const command = invocation.input.trim().toLowerCase()
  if (!command || command === 'status') {
    try {
      const configured = await api.settings.isSecretConfigured('test-token')
      return result(invocation.requestId,
        configured ? '测试凭据已配置' : '测试凭据未配置',
        '仅检查配置状态；请在 Host 设置中保存公开测试值，勿填真实密钥。')
    } catch {
      return result(invocation.requestId, '测试凭据状态读取失败', '请重新打开 Host 插件设置并检查配置；勿填真实密钥。')
    }
  }
  if (command !== 'request') return result(invocation.requestId, '请输入 status 或 request', 'status 检查本地配置；request 发起一次认证测试。')
  if (!api.network) return result(invocation.requestId, '网络能力不可用', '需要 Windows Host 0.4.0 或更高版本及网络授权。')
  try {
    const response = await api.network.request({
      url: 'https://httpbun.com/bearer/uipilot-acceptance-token',
      method: 'GET',
      credential: 'acceptance-token',
    })
    const status = response.status
    const title = status === 200 ? '认证成功（HTTP 200）'
      : status === 401 ? '认证被拒绝（HTTP 401）'
      : `收到其他 HTTP 状态（HTTP ${status}）`
    return result(invocation.requestId, title, '仅依据 HTTP 状态；未读取服务响应内容。')
  } catch (error) {
    const name = error?.name
    return result(invocation.requestId, ERROR_NAMES.has(name) ? name : 'NetworkFailureError',
      '请检查 Host 测试凭据和网络授权，重新运行 request；勿填真实密钥。')
  }
}