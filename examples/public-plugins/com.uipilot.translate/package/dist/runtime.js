const errors = Object.freeze({
 NetworkFailureError: ['翻译网络请求失败', '请检查网络后重试'],
 NetworkTargetDeniedError: ['翻译网络目标不可用', '请检查插件网络配置'],
 NetworkResponseTooLargeError: ['翻译响应过大', '请缩短输入后重试'],
 NetworkResponseInvalidError: ['翻译服务响应异常', '请稍后重试'],
 NetworkLimitExceededError: ['翻译调用过于频繁', '请稍后重试'],
 ProviderFailureError: ['翻译服务暂时不可用', '请稍后重试'],
 ServicePermissionDeniedError: ['有道翻译未授权', '请在设置 → 权限中开启有道翻译服务'],
 PermissionDeniedError: ['网络访问已关闭', '请在设置 → 权限中开启网络访问'],
 CredentialNotConfiguredError: ['有道翻译未配置', '请在设置 → 密钥配置中配置有道翻译'],
 CredentialDisabledError: ['有道翻译配置已关闭', '请在设置 → 密钥配置中启用有道翻译'],
 CredentialUnavailableError: ['有道翻译密钥不可用', '请在设置 → 密钥配置中检查有道翻译配置'],
 ProviderAuthenticationError: ['有道翻译认证失败', '请在设置 → 密钥配置中检查有道翻译配置'],
 ProviderQuotaError: ['有道翻译额度不足或调用受限', '请检查有道翻译服务额度'],
 NetworkTimeoutError: ['翻译请求超时', '请稍后重试'],
 ExpiredRequestError: ['翻译请求已失效', '请重新运行翻译'],
 InvalidServiceRequestError: ['翻译输入无效', '请输入不超过 8192 UTF-8 字节的中文或英文'],
})
function result(requestId, title, subtitle) {
 return { requestId, results: [{ id: 'translate-error', title, subtitle }] }
}
export async function onCommand(invocation, api) {
 const text = invocation.input
 if (!text.trim()) return result(invocation.requestId, '请输入需要翻译的内容', '支持中文和英文')
 if (!api.services?.youdao) return result(invocation.requestId, '翻译服务不可用', '需要 UiPilot 0.15.0 或更高版本')
 const chinese = /[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]/u.test(text)
 try {
  const response = await api.services.youdao.translate({ text, from: chinese ? 'zh-CHS' : 'en', to: chinese ? 'en' : 'zh-CHS' })
  const translated = response.translations.join('\n')
  return { requestId: invocation.requestId, results: [{ id: 'translate-result', title: Array.from(translated).slice(0, 256).join(''), subtitle: `${chinese ? '中译英' : '英译中'}`, defaultAction: { type: 'copyText', text: translated } }] }
 } catch (error) {
  const [title, subtitle] = (Object.hasOwn(errors, error?.name) ? errors[error.name] : undefined) ?? ['翻译失败', '请稍后重试']
  return result(invocation.requestId, title, subtitle)
 }
}
