const encoder = new TextEncoder()
const errorTitles = Object.freeze({
  CredentialNotConfiguredError: '请先配置百度 OCR',
  CredentialDisabledError: '请在设置中启用百度 OCR',
  CredentialUnavailableError: '百度 OCR 凭据不可用，请重新配置',
  ServicePermissionDeniedError: '请在设置中授权百度 OCR',
  PermissionDeniedError: '请在设置中授权网络访问',
  NetworkTargetDeniedError: '百度 OCR 网络目标未获授权',
  ProviderAuthenticationError: '百度 OCR 认证失败',
  ProviderQuotaError: '百度 OCR 额度不足或请求受限',
  NetworkTimeoutError: '百度 OCR 请求超时',
  NetworkFailureError: '无法连接百度 OCR',
  NetworkLimitExceededError: '请求过于频繁，请稍后重试',
  NetworkResponseTooLargeError: '识别结果过长，请分区域识别',
  NetworkResponseInvalidError: '百度 OCR 返回了无效结果',
  InvalidServiceRequestError: '图片不可用，请重新粘贴',
})

function notice(requestId, title) {
  return { requestId, results: [{ id: 'baidu-ocr-notice', title }] }
}

export async function onCommand(invocation, api) {
  const { requestId } = invocation
  if (invocation.commandId !== undefined && invocation.commandId !== 'default') {
    return notice(requestId, '无法执行此 OCR 命令')
  }
  if (!invocation.image?.id) return notice(requestId, '请粘贴一张图片')
  if (typeof api.services?.baidu?.ocr !== 'function') {
    return notice(requestId, '请升级宿主以使用百度 OCR')
  }
  let recognized
  try {
    recognized = await api.services.baidu.ocr({ imageId: invocation.image.id })
  } catch (error) {
    if (error?.name === 'ExpiredRequestError') throw error
    const title = Object.hasOwn(errorTitles, error?.name) ? errorTitles[error.name] : '百度 OCR 识别失败'
    return notice(requestId, title)
  }
  const text = recognized?.text
  if (typeof text !== 'string' || text.includes('\0')) return notice(requestId, '百度 OCR 返回了无效结果')
  if (!text.trim()) return notice(requestId, '未识别到文字')
  if (encoder.encode(text).length > 16 * 1024) return notice(requestId, '识别结果过长，请分区域识别')
  const response = {
    requestId,
    results: [{
      id: 'baidu-ocr-text',
      title: [...text.replace(/\s+/gu, ' ').trim()].slice(0, 120).join(''),
      subtitle: '百度 OCR',
      detail: text,
      defaultAction: { type: 'copyText', text },
    }],
  }
  if (encoder.encode(JSON.stringify(response)).length > 64 * 1024) {
    return notice(requestId, '识别结果过长，请分区域识别')
  }
  return response
}
