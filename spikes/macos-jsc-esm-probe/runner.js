// Evaluated BEFORE package code, retained only by native JSValue. This factory
// receives the native block as an argument; it is never stored on globalThis.
(function (complete, invocationJSON) {
  'use strict'
  const apply = Reflect.apply
  const stringify = JSON.stringify
  const parse = JSON.parse
  const resolve = Promise.resolve
  const then = Promise.prototype.then
  const PromiseType = Promise
  const freeze = Object.freeze
  const invocation = parse(invocationJSON)
  freeze(invocation.context)
  freeze(invocation)
  const api = freeze({})
  const requestId = invocation.requestId
  const failure = () => complete('')
  const success = value => {
    try {
      if (!value || typeof value !== 'object' || value.requestId !== requestId) {
        failure()
        return
      }
      const text = stringify(value)
      if (typeof text !== 'string' || text.length > 4096) failure()
      else complete(text)
    } catch { failure() }
  }
  return function (handler) {
    try {
      if (typeof handler !== 'function') { failure(); return }
      const result = handler(invocation, api)
      apply(then, apply(resolve, PromiseType, [result]), [success, failure])
    } catch { failure() }
  }
})
