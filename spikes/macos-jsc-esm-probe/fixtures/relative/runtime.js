import { count, increment } from './reexport.js'
export async function onCommand(invocation, api) {
  const before = count
  increment()
  await Promise.resolve()
  return {
    requestId: invocation.requestId,
    data: {
      before, after: count,
      retainedPureCall: globalThis.PURE_TOP_LEVEL_CALL === true,
      frozen: Object.isFrozen(invocation) && Object.isFrozen(invocation.context) && Object.isFrozen(api),
      input: invocation.input,
    },
  }
}
