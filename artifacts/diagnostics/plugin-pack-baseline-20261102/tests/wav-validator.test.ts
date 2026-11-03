import { describe, expect, it } from 'vitest'

import { WavValidationError, validateTimerAlarmWav } from '../src/wav-validator.js'
import { createWav } from './wav-fixture.js'

describe('validateTimerAlarmWav', () => {
  it.each([
    createWav(1, 1, 44_100, 16),
    createWav(1, 1, 44_100, 24),
    createWav(100, 2, 48_000, 24),
    createWav(44_100 * 15, 1, 44_100, 24),
  ])('accepts supported PCM boundaries', (bytes) => {
    expect(() => validateTimerAlarmWav(bytes)).not.toThrow()
  })

  it('rejects noncanonical chunks, PCM fields, trailing bytes, and duration', () => {
    const valid = createWav(100)
    const cases: Buffer[] = []
    for (const [offset, value, width] of [
      [0, 0x58, 1],
      [16, 18, 4],
      [20, 3, 2],
      [22, 3, 2],
      [24, 22_050, 4],
      [28, 1, 4],
      [32, 1, 2],
      [34, 8, 2],
    ] as const) {
      const bytes = Buffer.from(valid)
      if (width === 1) bytes.writeUInt8(value, offset)
      else if (width === 2) bytes.writeUInt16LE(value, offset)
      else bytes.writeUInt32LE(value, offset)
      cases.push(bytes)
    }
    const unknown = Buffer.from(valid)
    unknown.write('JUNK', 36, 'ascii')
    cases.push(unknown, Buffer.concat([valid, Buffer.from([0])]), createWav(0), createWav(44_100 * 15 + 1))
    for (const bytes of cases) expect(() => validateTimerAlarmWav(bytes)).toThrow(WavValidationError)
  })

  it('rejects incorrect odd-data padding', () => {
    const bad = createWav(1, 1, 44_100, 24)
    bad[bad.length - 1] = 1
    expect(() => validateTimerAlarmWav(bad)).toThrow(WavValidationError)
  })
})
