export async function onCommand(invocation, _api) {
  return {
    requestId: invocation.requestId,
    data: {},
  }
}
