// Created before package evaluation, held only by Rust. No global completion API.
(function (invocationJSON) {
  'use strict'
  const parse = JSON.parse
  const stringify = JSON.stringify
  const apply = Reflect.apply
  const resolve = Promise.resolve
  const then = Promise.prototype.then
  const PromiseType = Promise
  const invocation = parse(invocationJSON)
  Object.freeze(invocation.context)
  Object.freeze(invocation)
  const api = Object.freeze({})
  const requestId = invocation.requestId
  return function (handler) {
    const result = handler(invocation, api)
    return apply(then, apply(resolve, PromiseType, [result]), [value => {
      if (!value || typeof value !== 'object' || value.requestId !== requestId) throw 'invalidResponse'
      const text = stringify(value)
      if (typeof text !== 'string' || text.length > 4096) throw 'invalidResponse'
      return text
    }])
  }
})
