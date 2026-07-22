export function calculate(source) {
  const parser = {
    source,
    index: 0,
    skip() {
      while (this.source[this.index] === ' ') this.index += 1
    },
    expression() {
      let value = this.term()
      if (value === null) return null
      for (;;) {
        this.skip()
        const operator = this.source[this.index]
        if (operator !== '+' && operator !== '-') return value
        this.index += 1
        const right = this.term()
        if (right === null) return null
        value = operator === '+' ? value + right : value - right
        if (!Number.isFinite(value)) return null
      }
    },
    term() {
      let value = this.factor()
      if (value === null) return null
      for (;;) {
        this.skip()
        const operator = this.source[this.index]
        if (operator !== '*' && operator !== '/') return value
        this.index += 1
        const right = this.factor()
        if (right === null || (operator === '/' && right === 0)) return null
        value = operator === '*' ? value * right : value / right
        if (!Number.isFinite(value)) return null
      }
    },
    factor() {
      this.skip()
      let sign = 1
      while (this.source[this.index] === '+' || this.source[this.index] === '-') {
        if (this.source[this.index] === '-') sign *= -1
        this.index += 1
        this.skip()
      }
      if (this.source[this.index] === '(') {
        this.index += 1
        const value = this.expression()
        this.skip()
        if (value === null || this.source[this.index] !== ')') return null
        this.index += 1
        return sign * value
      }
      const value = this.number()
      return value === null ? null : sign * value
    },
    number() {
      const start = this.index
      while (isDigit(this.source[this.index])) this.index += 1
      if (this.source[this.index] === '.') {
        this.index += 1
        while (isDigit(this.source[this.index])) this.index += 1
      }
      const literal = this.source.slice(start, this.index)
      if (literal === '' || literal === '.') return null
      const value = Number(literal)
      return Number.isFinite(value) ? value : null
    },
  }

  const value = parser.expression()
  parser.skip()
  if (value === null || parser.index !== source.length || !Number.isFinite(value)) return null
  return Object.is(value, -0) ? '0' : value.toString()
}

function isDigit(character) {
  return character >= '0' && character <= '9'
}

if (globalThis.uipilot) {
  globalThis.uipilot.onQuery((query) => {
    const result = calculate(query)
    globalThis.uipilot.publishResults({
      items: result === null
        ? []
        : [{
            title: result,
            subtitle: 'Copy result',
            action: { type: 'copyText', text: result },
          }],
    })
  })
}
