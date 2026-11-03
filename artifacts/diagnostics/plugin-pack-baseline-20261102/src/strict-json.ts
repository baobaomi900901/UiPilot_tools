export class StrictJsonError extends Error {
  constructor(
    message: string,
    readonly byteOffset: number,
  ) {
    super(message)
    this.name = 'StrictJsonError'
  }
}

class Parser {
  #index = 0

  constructor(private readonly source: string) {}

  parse(): unknown {
    const value = this.#value(0)
    this.#whitespace()
    if (this.#index !== this.source.length) this.#fail('Unexpected trailing JSON content.')
    return value
  }

  #value(depth: number): unknown {
    this.#whitespace()
    const character = this.source[this.#index]
    if (character === '{') return this.#object(depth + 1)
    if (character === '[') return this.#array(depth + 1)
    if (character === '"') return this.#string()
    if (character === 't') return this.#literal('true', true)
    if (character === 'f') return this.#literal('false', false)
    if (character === 'n') return this.#literal('null', null)
    return this.#number()
  }

  #object(depth: number): Record<string, unknown> {
    this.#depth(depth)
    this.#index += 1
    const value: Record<string, unknown> = {}
    const keys = new Set<string>()
    this.#whitespace()
    if (this.source[this.#index] === '}') {
      this.#index += 1
      return value
    }
    while (true) {
      this.#whitespace()
      if (this.source[this.#index] !== '"') this.#fail('Object key must be a string.')
      const key = this.#string()
      if (keys.has(key)) this.#fail('Duplicate object key.')
      keys.add(key)
      this.#whitespace()
      if (this.source[this.#index] !== ':') this.#fail('Object key must be followed by a colon.')
      this.#index += 1
      Object.defineProperty(value, key, {
        value: this.#value(depth),
        enumerable: true,
        configurable: true,
        writable: true,
      })
      this.#whitespace()
      const delimiter = this.source[this.#index]
      if (delimiter === '}') {
        this.#index += 1
        return value
      }
      if (delimiter !== ',') this.#fail('Object member must be followed by a comma or closing brace.')
      this.#index += 1
    }
  }

  #array(depth: number): unknown[] {
    this.#depth(depth)
    this.#index += 1
    const value: unknown[] = []
    this.#whitespace()
    if (this.source[this.#index] === ']') {
      this.#index += 1
      return value
    }
    while (true) {
      value.push(this.#value(depth))
      this.#whitespace()
      const delimiter = this.source[this.#index]
      if (delimiter === ']') {
        this.#index += 1
        return value
      }
      if (delimiter !== ',') this.#fail('Array item must be followed by a comma or closing bracket.')
      this.#index += 1
    }
  }

  #string(): string {
    this.#index += 1
    let value = ''
    while (this.#index < this.source.length) {
      const character = this.source[this.#index]
      if (character === '"') {
        this.#index += 1
        return value
      }
      if (character === '\\') {
        this.#index += 1
        value += this.#escape()
        continue
      }
      const code = this.source.charCodeAt(this.#index)
      if (code <= 0x1f) this.#fail('JSON string contains a control character.')
      if (code >= 0xd800 && code <= 0xdbff) {
        const low = this.source.charCodeAt(this.#index + 1)
        if (low < 0xdc00 || low > 0xdfff) this.#fail('JSON string contains an unpaired surrogate.')
        value += character + this.source[this.#index + 1]
        this.#index += 2
        continue
      }
      if (code >= 0xdc00 && code <= 0xdfff) this.#fail('JSON string contains an unpaired surrogate.')
      value += character
      this.#index += 1
    }
    this.#fail('JSON string is unterminated.')
  }

  #escape(): string {
    const escape = this.source[this.#index]
    this.#index += 1
    const simple: Record<string, string> = {
      '"': '"',
      '\\': '\\',
      '/': '/',
      b: '\b',
      f: '\f',
      n: '\n',
      r: '\r',
      t: '\t',
    }
    if (escape in simple) return simple[escape]
    if (escape !== 'u') this.#fail('JSON string contains an invalid escape.')
    const high = this.#hexCodeUnit()
    if (high >= 0xd800 && high <= 0xdbff) {
      if (this.source.slice(this.#index, this.#index + 2) !== '\\u') {
        this.#fail('JSON string contains an unpaired surrogate.')
      }
      this.#index += 2
      const low = this.#hexCodeUnit()
      if (low < 0xdc00 || low > 0xdfff) this.#fail('JSON string contains an unpaired surrogate.')
      return String.fromCodePoint(0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00))
    }
    if (high >= 0xdc00 && high <= 0xdfff) this.#fail('JSON string contains an unpaired surrogate.')
    return String.fromCharCode(high)
  }

  #hexCodeUnit(): number {
    const raw = this.source.slice(this.#index, this.#index + 4)
    if (!/^[0-9A-Fa-f]{4}$/u.test(raw)) this.#fail('JSON string contains an invalid Unicode escape.')
    this.#index += 4
    return Number.parseInt(raw, 16)
  }

  #number(): number {
    const match = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/u.exec(
      this.source.slice(this.#index),
    )
    if (!match) this.#fail('Expected a JSON value.')
    this.#index += match[0].length
    const value = Number(match[0])
    if (!Number.isFinite(value)) this.#fail('JSON number must be finite.')
    return value
  }

  #literal<T>(text: string, value: T): T {
    if (this.source.slice(this.#index, this.#index + text.length) !== text) this.#fail('Expected a JSON value.')
    this.#index += text.length
    return value
  }

  #whitespace(): void {
    while (/^[\u0009\u000a\u000d\u0020]$/u.test(this.source[this.#index] ?? '')) this.#index += 1
  }

  #depth(depth: number): void {
    if (depth > 128) this.#fail('JSON nesting exceeds 128 containers.')
  }

  #fail(message: string): never {
    throw new StrictJsonError(message, Buffer.byteLength(this.source.slice(0, this.#index), 'utf8'))
  }
}

export function parseStrictJson(bytes: Uint8Array): unknown {
  let source: string
  try {
    source = new TextDecoder('utf-8', { fatal: true }).decode(bytes)
  } catch {
    throw new StrictJsonError('Manifest is not valid UTF-8.', 0)
  }
  return new Parser(source).parse()
}
