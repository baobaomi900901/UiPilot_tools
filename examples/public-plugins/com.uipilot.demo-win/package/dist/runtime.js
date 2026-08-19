function localDate(invokedAt) {
  return invokedAt.slice(0, 10)
}

function returnedText(invocation) {
  return `${invocation.input} ${localDate(invocation.context.invokedAt)}`
}

export async function onCommand(invocation, api) {
  const returnText = returnedText(invocation)
  await api.notifications.publish({ content: returnText })
  return {
    requestId: invocation.requestId,
    data: { returnText },
  }
}
