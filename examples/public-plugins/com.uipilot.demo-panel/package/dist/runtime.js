export async function onCommand(invocation) {
  return {
    requestId: invocation.requestId,
    data: {
      echo: invocation.input,
      requestId: invocation.requestId,
    },
  }
}
