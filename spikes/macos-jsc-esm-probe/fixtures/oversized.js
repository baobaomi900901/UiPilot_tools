export function onCommand(invocation) {
  return { requestId: invocation.requestId, data: 'x'.repeat(9000) }
}
