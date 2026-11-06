const encoder = new TextEncoder()
export const PROVIDERS = Object.freeze([{ id: 'baidu', label: '百度 OCR' }])
export const ERROR_MESSAGES = Object.freeze({
 CredentialNotConfiguredError: '请先配置百度 OCR', CredentialDisabledError: '请在设置中启用百度 OCR', CredentialUnavailableError: '百度 OCR 凭据不可用，请重新配置', ServicePermissionDeniedError: '请在设置中授权百度 OCR', PermissionDeniedError: '请在设置中授权网络访问', NetworkTargetDeniedError: '百度 OCR 网络目标未获授权', ProviderAuthenticationError: '百度 OCR 认证失败', ProviderQuotaError: '百度 OCR 额度不足或请求受限', NetworkTimeoutError: '百度 OCR 请求超时', NetworkFailureError: '无法连接百度 OCR', NetworkLimitExceededError: '请求过于频繁，请稍后重试', NetworkResponseTooLargeError: '识别结果过长，请分区域识别', NetworkResponseInvalidError: '百度 OCR 返回了无效结果', InvalidServiceRequestError: '图片不可用，请重新粘贴',
})
export function failure(code, message = ERROR_MESSAGES[code] ?? '百度 OCR 识别失败') { return { ok: false, code, message } }
export function validText(text) { return typeof text === 'string' && Boolean(text.trim()) && !text.includes('\0') && encoder.encode(text).length <= 16384 }
export function normalizeRecognition(result, requestId) {
 const text = result?.text
 if (typeof text !== 'string' || text.includes('\0')) return failure('NetworkResponseInvalidError')
 if (!text.trim()) return failure('EmptyRecognition', '未识别到文字')
 if (encoder.encode(text).length > 16384) return failure('NetworkResponseTooLargeError')
 const lines = text.split('\n')
 const data = { ok: true, text, lines }
 return encoder.encode(JSON.stringify({ requestId, data })).length > 65536 ? failure('NetworkResponseTooLargeError') : data
}
