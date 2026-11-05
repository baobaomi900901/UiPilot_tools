/** Reject oversized event payloads before expanding them into a JSON byte array. */
export async function imageBytes(file: Blob): Promise<number[]> {
  if (file.size > 8 * 1024 * 1024) throw { code: 'imageTooLarge' }
  const buffer = typeof file.arrayBuffer === 'function' ? await file.arrayBuffer() : await new Promise<ArrayBuffer>((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as ArrayBuffer)
    reader.onerror = () => reject({ code: 'invalidImage' })
    reader.readAsArrayBuffer(file)
  })
  return Array.from(new Uint8Array(buffer))
}

/** Fixed host codes only; never echo provider diagnostics or clipboard contents. */
export function imageImportError(value: unknown): string {
  const messages: Record<string, string> = {
    invalidImage: '图片格式无效或无法解码',
    imageTooLarge: '图片超限：最多 8 MiB，最长边 4096px，1600 万像素，标准化后最多 3 MiB',
    expiredImage: '图片已失效，请重新粘贴',
    imageUnavailable: '图片不可用，请重新粘贴',
    clipboardImageUnavailable: '剪贴板中没有可用的图片，请重新复制截图',
    invalidCaller: '图片入口不可用',
  }
  if (typeof value !== 'object' || value === null || !Object.prototype.hasOwnProperty.call(value, 'code')) return '图片导入失败，请重试'
  const code = (value as { code?: unknown }).code
  return typeof code === 'string' ? messages[code] ?? '图片导入失败，请重试' : '图片导入失败，请重试'
}
