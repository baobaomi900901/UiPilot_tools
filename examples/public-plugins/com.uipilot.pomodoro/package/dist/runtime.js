const DEFAULT_COMPLETION_MESSAGE = '番茄钟完成'

export async function onCommand(invocation) {
  const completionMessage = invocation.input.trim() || DEFAULT_COMPLETION_MESSAGE
  return {
    requestId: invocation.requestId,
    data: {
      completionMessage,
    },
  }
}
