import { describe, expect, it } from 'vitest'

import { StrictJsonError, parseStrictJson } from '../src/strict-json.js'

describe('parseStrictJson', () => {
  it('parses JSON values without altering valid scalar text', () => {
    expect(parseStrictJson(Buffer.from('{"a":[true,null,"\ud83d\ude00"],"n":1.5}'))).toEqual({
      a: [true, null, '\ud83d\ude00'],
      n: 1.5,
    })
  })

  it.each([
    '{"a":1,"a":2}',
    '{"nested":{"x":1,"x":2}}',
    '{"x":"\\uD800"}',
    '{"x":"\\uDEAD"}',
    '{"x":"\\uDC00\\uD800"}',
    '{"\\uD800":1}',
    '{"x":1e400}',
  ])('rejects duplicate, non-scalar, or non-finite JSON: %s', (source) => {
    expect(() => parseStrictJson(Buffer.from(source))).toThrow(StrictJsonError)
  })

  it('fatal-decodes UTF-8', () => {
    expect(() => parseStrictJson(Buffer.from([0x7b, 0x22, 0x78, 0x22, 0x3a, 0xff, 0x7d]))).toThrow(
      StrictJsonError,
    )
  })

  it('rejects nesting deeper than 128 containers', () => {
    expect(() => parseStrictJson(Buffer.from(`${'['.repeat(129)}0${']'.repeat(129)}`))).toThrow(
      StrictJsonError,
    )
  })
})
