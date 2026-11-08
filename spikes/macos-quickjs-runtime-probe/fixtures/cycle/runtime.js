import { a, snapshot, advance } from './reexport.js'
// Actual module evaluation must wait for this top-level Promise.
let ready = false
await Promise.resolve().then(() => { ready = true })
export async function onCommand(invocation, api) {
  if (!ready) throw new Error('module evaluation was not awaited')
  const before = snapshot()
  advance()
  const dynamic = await import('./a.js')
  return {
    requestId: invocation.requestId,
    data: {
      before, after: snapshot(), live: a, dynamicLive: dynamic.a, ready,
      frozen: Object.isFrozen(invocation) && Object.isFrozen(invocation.context) && Object.isFrozen(api),
    },
  }
}
