function recurse(depth) { return depth === 0 ? 0 : 1 + recurse(depth - 1) }
export function onCommand(invocation) {
  return { requestId: invocation.requestId, data: recurse(100) }
}
