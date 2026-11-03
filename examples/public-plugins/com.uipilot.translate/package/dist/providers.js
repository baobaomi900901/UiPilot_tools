import { isLanguage } from './languages.js'

// Providers receive only validated business input and the request-bound Host API.
// Adding a provider requires an explicit adapter here; caller input is never a URL.
async function translateYoudao(input, api) {
  if (typeof api.services?.youdao?.translate !== 'function') {
    throw { name: 'ServiceUnavailableError' }
  }
  const response = await api.services.youdao.translate({
    text: input.text,
    from: input.sourceLanguage,
    to: input.targetLanguage,
  })
  if (!response || !Array.isArray(response.translations)
    || response.translations.length === 0
    || !Array.from(response.translations).every(text => typeof text === 'string' && text.length > 0 && !text.includes('\0'))
    || (response.sourceLanguage !== null && !isLanguage(response.sourceLanguage))
    || response.targetLanguage !== input.targetLanguage
    || typeof response.unchanged !== 'boolean') {
    throw { name: 'NetworkResponseInvalidError' }
  }
  return {
    ok: true,
    text: response.translations.join('\n'),
    detectedSourceLanguage: response.sourceLanguage,
    targetLanguage: response.targetLanguage,
  }
}

const providers = new Map([['youdao', translateYoudao]])
export function getProvider(id) { return providers.get(id) }
