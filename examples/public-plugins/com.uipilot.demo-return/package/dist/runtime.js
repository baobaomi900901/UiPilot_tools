function localDate(invokedAt) {
  return invokedAt.slice(0, 10)
}

function returnedText(invocation) {
  return `${invocation.input} ${localDate(invocation.context.invokedAt)}`
}

export async function onCommand(invocation, _api) {
  const text = returnedText(invocation)
  return {
    requestId: invocation.requestId,
    results: [
      {
        id: 'demo-return-copy',
        title: text,
        subtitle: 'Demo return result',
        defaultAction: { type: 'copyText', text },
      },
    ],
  }
}
