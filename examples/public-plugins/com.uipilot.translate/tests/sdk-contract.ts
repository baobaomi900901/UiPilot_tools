import type { PluginHandler, PluginPanelActionHandler, PluginYoudaoTranslationLanguage } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const initialize: PluginHandler = async (invocation, api) => {
  const stored = await api.storage.get('translate.preferences.v1')
  return { requestId: invocation.requestId, data: { text: invocation.input, preferences: stored } }
}
const translate: PluginPanelActionHandler = async (invocation, api) => {
  const response = await api.services?.youdao.translate({ text: ' hello\n ', from: 'auto', to: 'zh-CHS' })
  const detected: PluginYoudaoTranslationLanguage | null = response?.sourceLanguage ?? null
  const unchanged: boolean = response?.unchanged ?? false
  void unchanged
  return { requestId: invocation.requestId, data: { ok: true, text: response?.translations.join('\n') ?? '', detectedSourceLanguage: detected, targetLanguage: response?.targetLanguage ?? 'zh-CHS' } }
}
void initialize
void translate

const panel = window.uipilotPluginPanel
async function textEffects() {
  await panel.clipboard?.writeText({ text: 'hello\n你好' })
  const voices = await panel.speech?.voices()
  const state = await panel.speech?.play({text:'hello',language:'en'})
  if (state?.playbackId) await panel.speech?.stop({playbackId:state.playbackId})
  const unsubscribe = panel.speech?.onState(state => { const id: string | null = state.playbackId; void id })
  await panel.speech?.state()
  unsubscribe?.()
  void voices
}
void textEffects

const japanese: PluginYoudaoTranslationLanguage = 'ja'
const traditional: PluginYoudaoTranslationLanguage = 'zh-CHT'
const serbian: PluginYoudaoTranslationLanguage = 'sr-Latn'
// @ts-expect-error automatic detection is not a concrete language
const automatic: PluginYoudaoTranslationLanguage = 'auto'
// @ts-expect-error unknown language codes remain rejected
const unknownLanguage: PluginYoudaoTranslationLanguage = 'not-real'
void [japanese, traditional, serbian, automatic, unknownLanguage]
