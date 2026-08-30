export async function onCommand(invocation) {
  return {
    requestId: invocation.requestId,
    data: {
      fixture: 'clipboard-history-host',
      input: invocation.input,
    },
  }
}
