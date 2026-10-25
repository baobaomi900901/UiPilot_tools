const BUILD = '1.0.0'

export async function onCommand(invocation, api) {
  const greeting = await api.settings.get('greeting')
  const count = await api.settings.get('count')
  const uppercase = await api.settings.get('uppercase')
  const style = await api.settings.get('style')
  const text = `${uppercase ? greeting.toUpperCase() : greeting} × ${count}`
  return {
    requestId: invocation.requestId,
    results: [{
      id: 'settings-current',
      title: style === 'boxed' ? `【${text}】` : text,
      subtitle: `uppercase=${uppercase} · style=${style} · build ${BUILD}`,
    }],
  }
}
