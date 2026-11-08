// These mutations must not replace the privately captured test mechanisms.
JSON.stringify = () => 'FORGED'
JSON.parse = () => ({ forged: true })
Promise.resolve = () => { throw new Error('changed resolve') }
Promise.prototype.then = () => { throw new Error('changed then') }
Object.freeze = () => { throw new Error('changed freeze') }
Reflect.apply = () => { throw new Error('changed apply') }
export function onCommand(invocation) {
  return { requestId: invocation.requestId, data: 'captured intrinsics' }
}
