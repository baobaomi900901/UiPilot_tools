export const LANGUAGES = Object.freeze([{ id: 'en', label: '英语' }, { id: 'zh-CHS', label: '简体中文' }].map(Object.freeze))
export const PROVIDERS = Object.freeze([Object.freeze({ id: 'youdao', label: '有道翻译' })])
export const PREFERENCES_STORAGE_KEY = 'translate.preferences.v1'
export const isLanguage = value => LANGUAGES.some(language => language.id === value)
export function sanitizePreferences(value) {
 const input = value && typeof value === 'object' ? value : {}
 return {
  sourceLanguage: input.sourceLanguage === 'auto' || isLanguage(input.sourceLanguage) ? input.sourceLanguage : 'auto',
  targetLanguage: isLanguage(input.targetLanguage) ? input.targetLanguage : 'zh-CHS',
  providerId: PROVIDERS.some(provider => provider.id === input.providerId) ? input.providerId : 'youdao',
 }
}
export function filterLanguages(query = '', includeAuto = false) {
 const choices = includeAuto ? [{ id: 'auto', label: '自动检测' }, ...LANGUAGES] : LANGUAGES
 const needle = String(query).trim().toLowerCase()
 return choices.filter(item => `${item.id} ${item.label}`.toLowerCase().includes(needle))
}
