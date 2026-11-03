import { isLanguage, PREFERENCES_STORAGE_KEY, sanitizePreferences } from './languages.js'
import { getProvider } from './providers.js'

const errors = Object.freeze({
  ServiceUnavailableError: ['SERVICE_UNAVAILABLE', '翻译服务不可用，需要 UiPilot 0.19.0 或更高版本'],
  NetworkFailureError: ['NETWORK_FAILURE', '翻译网络请求失败，请检查网络后重试'],
  NetworkTargetDeniedError: ['NETWORK_TARGET_DENIED', '翻译网络目标不可用，请检查插件网络配置'],
  NetworkResponseTooLargeError: ['RESPONSE_TOO_LARGE', '翻译响应过大，请缩短输入后重试'],
  NetworkResponseInvalidError: ['INVALID_RESPONSE', '翻译服务响应异常，请稍后重试'],
  NetworkLimitExceededError: ['RATE_LIMITED', '翻译调用过于频繁，请稍后重试'],
  ProviderFailureError: ['PROVIDER_FAILURE', '翻译服务暂时不可用，请稍后重试'],
  ServicePermissionDeniedError: ['SERVICE_PERMISSION_DENIED', '有道翻译未授权，请在设置 → 权限中开启有道翻译服务'],
  PermissionDeniedError: ['PERMISSION_DENIED', '网络访问已关闭，请在设置 → 权限中开启网络访问'],
  CredentialNotConfiguredError: ['CREDENTIAL_NOT_CONFIGURED', '有道翻译未配置，请在设置 → 密钥配置中配置有道翻译'],
  CredentialDisabledError: ['CREDENTIAL_DISABLED', '有道翻译配置已关闭，请在设置 → 密钥配置中启用有道翻译'],
  CredentialUnavailableError: ['CREDENTIAL_UNAVAILABLE', '有道翻译密钥不可用，请在设置 → 密钥配置中检查有道翻译配置'],
  ProviderAuthenticationError: ['PROVIDER_AUTHENTICATION', '有道翻译认证失败，请在设置 → 密钥配置中检查有道翻译配置'],
  ProviderLanguageUnsupportedError: ['LANGUAGE_UNSUPPORTED', '有道暂不支持当前语言方向，请手动选择其他源语言或目标语言'],
  ProviderQuotaError: ['PROVIDER_QUOTA', '有道翻译额度不足或调用受限，请检查有道翻译服务额度'],
  NetworkTimeoutError: ['NETWORK_TIMEOUT', '翻译请求超时，请稍后重试'],
  ExpiredRequestError: ['REQUEST_EXPIRED', '翻译请求已失效，请重新运行翻译'],
  InvalidServiceRequestError: ['INVALID_REQUEST', '翻译输入无效，请输入不超过 8192 UTF-8 字节的文本并选择支持的语言'],
})
const encoder = new TextEncoder()
const payloadKeys = ['providerId', 'text', 'sourceLanguage', 'targetLanguage']

function isValidPayload(payload) {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return false
  const descriptors = Object.getOwnPropertyDescriptors(payload)
  const keys = Reflect.ownKeys(descriptors)
  // Check data properties before reading values: no inherited fields or getters.
  if (keys.length !== payloadKeys.length || !payloadKeys.every(key =>
    Object.hasOwn(descriptors, key) && descriptors[key].enumerable && Object.hasOwn(descriptors[key], 'value'))) return false
  return typeof payload.providerId === 'string' && Boolean(getProvider(payload.providerId))
    && typeof payload.text === 'string' && payload.text.trim().length > 0
    && !payload.text.includes('\0') && encoder.encode(payload.text).length <= 8192
    && (payload.sourceLanguage === 'auto' || isLanguage(payload.sourceLanguage))
    && isLanguage(payload.targetLanguage)
}

function failure(error) {
  const [code, message] = Object.hasOwn(errors, error?.name)
    ? errors[error.name]
    : ['TRANSLATION_FAILED', '翻译失败，请稍后重试']
  return { ok: false, code, message }
}

export async function onCommand(invocation, api) {
  let stored = null
  try { stored = await api.storage.get(PREFERENCES_STORAGE_KEY) } catch { /* Default preferences remain usable. */ }
  return { requestId: invocation.requestId, data: { text: invocation.input, preferences: sanitizePreferences(stored) } }
}

export async function onPanelAction(invocation, api) {
  let data
  try {
    if (invocation.action !== 'translate' || !isValidPayload(invocation.payload)) {
      throw { name: 'InvalidServiceRequestError' }
    }
    data = await getProvider(invocation.payload.providerId)(invocation.payload, api)
  } catch (error) {
    data = failure(error)
  }
  return { requestId: invocation.requestId, data }
}
