import { PNG } from 'pngjs'
import { describe, expect, it } from 'vitest'

import { crc32 } from '../src/crc32.js'
import { PngValidationError, validatePluginIcon } from '../src/png-validator.js'

function png(width = 128, height = 128): Buffer {
  const image = new PNG({ width, height })
  image.data.fill(0xff)
  return PNG.sync.write(image)
}

function chunk(type: string, data = Buffer.alloc(0)): Buffer {
  const typeBytes = Buffer.from(type, 'ascii')
  const output = Buffer.alloc(12 + data.length)
  output.writeUInt32BE(data.length, 0)
  typeBytes.copy(output, 4)
  data.copy(output, 8)
  output.writeUInt32BE(crc32(Buffer.concat([typeBytes, data])), 8 + data.length)
  return output
}

describe('validatePluginIcon', () => {
  it('accepts a complete static 128 x 128 PNG', () => {
    expect(() => validatePluginIcon(png())).not.toThrow()
  })

  it('rejects a bad chunk CRC and trailing bytes', () => {
    const corrupted = png()
    corrupted[20] ^= 1
    expect(() => validatePluginIcon(corrupted)).toThrow(PngValidationError)
    expect(() => validatePluginIcon(Buffer.concat([png(), Buffer.from([0])]))).toThrow(PngValidationError)
  })

  it('rejects APNG chunks before pixel decoding', () => {
    const source = png()
    const iend = source.length - 12
    const animated = Buffer.concat([source.subarray(0, iend), chunk('acTL', Buffer.alloc(8)), source.subarray(iend)])
    expect(() => validatePluginIcon(animated)).toThrow(PngValidationError)
  })

  it('rejects incorrect dimensions and oversized input', () => {
    expect(() => validatePluginIcon(png(16, 16))).toThrow(PngValidationError)
    expect(() => validatePluginIcon(Buffer.alloc(128 * 1024 + 1))).toThrow(PngValidationError)
  })
})
