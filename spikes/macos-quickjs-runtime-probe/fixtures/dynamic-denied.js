export async function onCommand(invocation) {
  const targets = ['std', 'os', 'node:fs', 'bare.js', '/etc/passwd', '../../outside.js',
    'https://example.invalid/x.js', './state.js?x', './state.js#x', './%2e%2e/x.js',
    './module.qbc', './library.so', './missing.js']
  const rejected = []
  for (const target of targets) {
    try { await import(target); rejected.push(false) } catch { rejected.push(true) }
  }
  // Warm the module cache: cached identity must not bypass attribute checks.
  await import('./state.js')
  try { await import('./state.js', { with: { type: 'json' } }); rejected.push(false) }
  catch { rejected.push(true) }
  return { requestId: invocation.requestId, data: rejected }
}
