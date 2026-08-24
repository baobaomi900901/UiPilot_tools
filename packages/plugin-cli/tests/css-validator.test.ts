import { describe, expect, it } from 'vitest'

import { CssValidationError, validateCssReferences } from '../src/css-validator.js'

const resources = new Set(['dist/window.css', 'dist/base.css', 'icon.png'])

describe('validateCssReferences', () => {
  it('accepts package-local url() and quoted imports', () => {
    const css = Buffer.from('@import "./base.css"; .icon { background: url("../icon.png") }')
    expect(() => validateCssReferences('dist/window.css', css, resources)).not.toThrow()
  })

  it('matches url() inside comments and strings like the host scanner', () => {
    expect(() =>
      validateCssReferences('dist/window.css', Buffer.from('/* url("../missing.png") */'), resources),
    ).toThrow(CssValidationError)
    expect(() =>
      validateCssReferences('dist/window.css', Buffer.from('x::after { content: "url(../missing.png)" }'), resources),
    ).toThrow(CssValidationError)
  })

  it('does not recognize whitespace between url and the opening parenthesis', () => {
    expect(() =>
      validateCssReferences('dist/window.css', Buffer.from('x { background: url (https://example.test/a) }'), resources),
    ).not.toThrow()
  })

  it('rejects import media remainders, missing delimiters, and first-parenthesis nesting', () => {
    for (const css of [
      '@import "./base.css" screen;',
      '@import "./base.css"',
      'x { background: url(calc(1 + 2)) }',
    ]) {
      expect(() => validateCssReferences('dist/window.css', Buffer.from(css), resources)).toThrow(
        CssValidationError,
      )
    }
  })

  it('fatal-decodes UTF-8', () => {
    expect(() => validateCssReferences('dist/window.css', Buffer.from([0xff]), resources)).toThrow(
      CssValidationError,
    )
  })
})
