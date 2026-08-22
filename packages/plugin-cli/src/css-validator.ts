import { canonicalRelativePath } from './package-policy.js'

const RUST_WHITESPACE = /^[\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+|[\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+$/gu

export class CssValidationError extends Error {
  constructor(
    message = 'CSS reference is invalid.',
    readonly byteOffset = 0,
  ) {
    super(message)
    this.name = 'CssValidationError'
  }
}

function asciiLower(value: string): string {
  return value.replace(/[A-Z]/gu, (character) => character.toLowerCase())
}

function trimRust(value: string): string {
  return value.replace(RUST_WHITESPACE, '')
}

function fail(css: string, offset: number, message?: string): never {
  throw new CssValidationError(message, Buffer.byteLength(css.slice(0, offset), 'utf8'))
}

function validateReference(
  stylesheet: string,
  reference: string,
  resources: ReadonlySet<string>,
  css: string,
  offset: number,
): void {
  if (
    !reference ||
    /[?#\\:%]/u.test(reference) ||
    reference.startsWith('/') ||
    /\p{Cc}/u.test(reference)
  ) {
    fail(css, offset)
  }
  const components = stylesheet.includes('/')
    ? stylesheet.slice(0, stylesheet.lastIndexOf('/')).split('/')
    : []
  for (const component of reference.split('/')) {
    if (!component) fail(css, offset)
    if (component === '.') continue
    if (component === '..') {
      if (components.pop() === undefined) fail(css, offset)
    } else {
      components.push(component)
    }
  }
  let canonical: string
  try {
    canonical = canonicalRelativePath(components.join('/'))
  } catch {
    fail(css, offset)
  }
  if (!resources.has(canonical)) fail(css, offset, `CSS resource does not exist: ${canonical}`)
}

function takeCssString(value: string, css: string, offset: number): [string, string] {
  const quote = value[0]
  if (quote !== "'" && quote !== '"') fail(css, offset)
  const end = value.indexOf(quote, 1)
  if (end < 0) fail(css, offset)
  return [value.slice(1, end), value.slice(end + 1)]
}

export function validateCssReferences(
  stylesheet: string,
  input: Uint8Array,
  resources: ReadonlySet<string>,
): void {
  let css: string
  try {
    css = new TextDecoder('utf-8', { fatal: true }).decode(input)
  } catch {
    throw new CssValidationError('CSS is not valid UTF-8.', 0)
  }
  const folded = asciiLower(css)
  let offset = 0
  while (true) {
    const index = folded.indexOf('url(', offset)
    if (index < 0) break
    const start = index + 4
    const end = folded.indexOf(')', start)
    if (end < 0) fail(css, start)
    const reference = trimRust(css.slice(start, end)).replace(/^['"]+|['"]+$/gu, '')
    validateReference(stylesheet, reference, resources, css, start)
    offset = end + 1
  }

  offset = 0
  while (true) {
    const index = folded.indexOf('@import', offset)
    if (index < 0) break
    const start = index + '@import'.length
    const end = folded.indexOf(';', start)
    if (end < 0) fail(css, start)
    const imported = trimRust(css.slice(start, end))
    if (!asciiLower(imported).startsWith('url(')) {
      const [reference, remainder] = takeCssString(imported, css, start)
      if (trimRust(remainder)) fail(css, start)
      validateReference(stylesheet, reference, resources, css, start)
    }
    offset = end + 1
  }
}
