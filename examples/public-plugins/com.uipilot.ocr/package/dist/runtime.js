import { ERROR_MESSAGES, failure, normalizeRecognition } from './providers.js'
export async function onCommand(invocation) {
 if (invocation.commandId !== undefined && invocation.commandId !== 'default') return { requestId: invocation.requestId, data: failure('InvalidCommand', '无法执行此 OCR 命令') }
 return { requestId: invocation.requestId, data: {} }
}
export async function onPanelAction(invocation, api) {
 const { requestId } = invocation
 const respond = data => ({ requestId, data })
 if (invocation.action !== 'recognize' || invocation.payload?.providerId !== 'baidu') return respond(failure('InvalidAction', '无法执行此 OCR 操作'))
 if (!invocation.image?.id) return respond(failure('InvalidServiceRequestError'))
 if (typeof api.services?.baidu?.ocr !== 'function') return respond(failure('UnsupportedHost', '请升级宿主以使用百度 OCR'))
 try { return respond(normalizeRecognition(await api.services.baidu.ocr({ imageId: invocation.image.id }), requestId)) }
 catch (error) {
  if (error?.name === 'ExpiredRequestError') throw error
  const code = Object.hasOwn(ERROR_MESSAGES, error?.name) ? error.name : 'RecognitionFailed'
  return respond(failure(code))
 }
}
