const OUTPUT_MODE = 'window'

function localDate(invokedAt) {
  return invokedAt.slice(0, 10)
}

function returnedText(invocation) {
  return `${invocation.input} ${localDate(invocation.context.invokedAt)}`
}

export async function onCommand(invocation, _api) {
  const text = returnedText(invocation)
  if (OUTPUT_MODE === 'mainResult') {
    return {
      requestId: invocation.requestId,
      results: [
        {
          id: 'demo-copy',
          title: text,
          subtitle: 'Demo result',
          defaultAction: { type: 'copyText', text },
        },
      ],
    }
  }
  return {
    requestId: invocation.requestId,
    data: { returnText: text },
  }
}
