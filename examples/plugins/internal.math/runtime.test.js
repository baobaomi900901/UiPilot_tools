import { readFileSync } from 'node:fs'

import { afterEach, describe, expect, it, vi } from 'vitest'

import { calculate } from './runtime.js'

describe('internal math runtime', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    delete globalThis.uipilot
  })

  it('calculates only the frozen arithmetic grammar', () => {
    expect(calculate('1+1')).toBe('2')
    expect(calculate('2+3*4')).toBe('14')
    expect(calculate('(2+3)*4')).toBe('20')
    expect(calculate('8/4/2')).toBe('1')
    expect(calculate('1-2-3')).toBe('-4')
    expect(calculate('--2')).toBe('2')
    expect(calculate('-(2+3)')).toBe('-5')
    expect(calculate('.5 + 1.25')).toBe('1.75')
    expect(calculate(' 2 * ( 3 + 4 ) ')).toBe('14')
    expect(calculate('-0')).toBe('0')
  })

  it('rejects incomplete, invalid, and non-finite expressions', () => {
    expect(calculate('1+')).toBeNull()
    expect(calculate('(1+2')).toBeNull()
    expect(calculate('1a')).toBeNull()
    expect(calculate('1/0')).toBeNull()
    expect(calculate('NaN')).toBeNull()
    expect(calculate('1'.padEnd(401, '0'))).toBeNull()
  })

  it('publishes a copy-text item only for calculable queries', async () => {
    const publishResults = vi.fn()
    let handler
    globalThis.uipilot = Object.freeze({
      onQuery(next) {
        handler = next
      },
      publishResults,
    })

    await import('./runtime.js?register')
    expect(handler).toBeTypeOf('function')

    handler('2+3*4')
    expect(publishResults).toHaveBeenLastCalledWith({
      items: [
        {
          title: '14',
          subtitle: 'Copy result',
          action: { type: 'copyText', text: '14' },
        },
      ],
    })

    handler('1/0')
    expect(publishResults).toHaveBeenLastCalledWith({ items: [] })
  })

  it('declares the exact removable plugin manifest', () => {
    expect(JSON.parse(readFileSync(new URL('./plugin.json', import.meta.url), 'utf8'))).toEqual({
      manifest: 1,
      id: 'internal.math',
      version: '0.2.0',
      minHostVersion: '0.2.0',
      runtime: 'runtime.html',
      feature: { id: 'calculate', trigger: '/math' },
      permissions: ['clipboard.writeText'],
    })
  })
})
