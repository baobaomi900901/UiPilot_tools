import { PNG } from 'pngjs'

import { crc32 } from './crc32.js'

const SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
const MAX_ICON_BYTES = 128 * 1024

export class PngValidationError extends Error {
  constructor(message = 'icon.png is invalid.') {
    super(message)
    this.name = 'PngValidationError'
  }
}

function invalid(message?: string): never {
  throw new PngValidationError(message)
}

export function validatePluginIcon(input: Uint8Array): void {
  const bytes = Buffer.from(input)
  if (bytes.byteLength > MAX_ICON_BYTES || bytes.byteLength < 20 || !bytes.subarray(0, 8).equals(SIGNATURE)) {
    invalid()
  }
  let offset = 8
  let chunkIndex = 0
  let sawIend = false
  while (offset < bytes.length) {
    if (offset + 12 > bytes.length) invalid()
    const length = bytes.readUInt32BE(offset)
    const end = offset + 12 + length
    if (!Number.isSafeInteger(end) || end > bytes.length) invalid()
    const typeBytes = bytes.subarray(offset + 4, offset + 8)
    const type = typeBytes.toString('ascii')
    if (!/^[A-Za-z]{4}$/u.test(type)) invalid()
    const data = bytes.subarray(offset + 8, offset + 8 + length)
    const expectedCrc = bytes.readUInt32BE(offset + 8 + length)
    if (crc32(Buffer.concat([typeBytes, data])) !== expectedCrc) invalid('PNG chunk CRC is invalid.')
    if (chunkIndex === 0 && (type !== 'IHDR' || length !== 13)) invalid()
    if (chunkIndex > 0 && type === 'IHDR') invalid()
    if (type === 'acTL' || type === 'fcTL' || type === 'fdAT') invalid('Animated PNG is unsupported.')
    if (type === 'IEND') {
      if (length !== 0 || sawIend || end !== bytes.length) invalid()
      sawIend = true
    } else if (sawIend) {
      invalid()
    }
    offset = end
    chunkIndex += 1
  }
  if (!sawIend || offset !== bytes.length) invalid()

  try {
    const decoded = PNG.sync.read(bytes, { checkCRC: true })
    if (decoded.width !== 128 || decoded.height !== 128) invalid('icon.png must be 128 x 128.')
  } catch (error) {
    if (error instanceof PngValidationError) throw error
    invalid('PNG pixels cannot be decoded.')
  }
}
