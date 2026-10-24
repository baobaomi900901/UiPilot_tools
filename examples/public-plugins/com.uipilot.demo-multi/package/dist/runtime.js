export async function onCommand(invocation, api) {
  let converted
  switch (invocation.commandId) {
    case 'upper':
      converted = invocation.input.toUpperCase()
      break
    case 'lower':
      converted = invocation.input.toLowerCase()
      break
    default:
      throw new Error('Unsupported command ID')
  }

  const configuredPrefix = await api.settings.get('prefix')
  const prefix = typeof configuredPrefix === 'string' ? configuredPrefix : ''
  const previous = await api.storage.get('invocation-count')
  const count = (typeof previous === 'number' && Number.isSafeInteger(previous) && previous >= 0 && previous < Number.MAX_SAFE_INTEGER ? previous : 0) + 1
  await api.storage.set('invocation-count', count)
  await api.storage.set('last-command', invocation.commandId)
  const text = prefix + converted
  return {
    requestId: invocation.requestId,
    results: [{
      id: 'text-copy',
      title: text,
      subtitle: 'Shared invocation ' + count,
      defaultAction: { type: 'copyText', text },
    }],
  }
}
